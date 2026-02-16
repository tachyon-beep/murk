//! PyBatchedWorld: Python wrapper for the batched engine.
//!
//! Single GIL release covers the entire step+observe hot path.

use numpy::{PyArray1, PyArrayMethods, PyUntypedArrayMethods};
use pyo3::prelude::*;

use murk_ffi::batched::{
    murk_batched_create, murk_batched_destroy, murk_batched_num_worlds,
    murk_batched_obs_mask_len, murk_batched_obs_output_len, murk_batched_observe_all,
    murk_batched_reset_all, murk_batched_reset_world, murk_batched_step_and_observe,
};
use murk_ffi::{MurkCommand, MurkObsEntry};

use crate::command::Command;
use crate::config::Config;
use crate::error::check_status;
use crate::obs::ObsEntry;

/// A batched simulation engine wrapping N lockstep worlds.
///
/// Steps all worlds and extracts observations in a single call with
/// one GIL release. This is the high-performance alternative to
/// stepping individual World objects in a Python loop.
///
/// Args:
///     configs: List of Config objects (all consumed).
///     obs_entries: List of ObsEntry for observation extraction (empty = no obs).
#[pyclass]
pub(crate) struct BatchedWorld {
    handle: Option<u64>,
    cached_num_worlds: usize,
    cached_obs_output_len: usize,
    cached_obs_mask_len: usize,
    /// Stored as usize (cast from *mut TrampolineData) for Send+Sync.
    trampoline_data: Vec<usize>,
}

#[pymethods]
impl BatchedWorld {
    /// Create a batched engine from a list of configs.
    #[new]
    #[pyo3(signature = (configs, obs_entries=None))]
    fn new(
        py: Python<'_>,
        configs: Vec<PyRefMut<'_, Config>>,
        obs_entries: Option<Vec<PyRef<'_, ObsEntry>>>,
    ) -> PyResult<Self> {
        if configs.is_empty() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "configs must not be empty",
            ));
        }

        // Take handles from all configs (consuming them).
        let mut config_handles = Vec::with_capacity(configs.len());
        let mut all_trampoline_data = Vec::new();
        for mut cfg in configs {
            let (ch, td) = cfg.take_handle()?;
            config_handles.push(ch);
            all_trampoline_data.extend(td);
        }

        // Convert obs entries.
        let ffi_entries: Vec<MurkObsEntry> = obs_entries
            .as_ref()
            .map(|entries| entries.iter().map(|e| e.inner).collect())
            .unwrap_or_default();
        let n_entries = ffi_entries.len();

        let entries_addr = if ffi_entries.is_empty() {
            0usize
        } else {
            ffi_entries.as_ptr() as usize
        };
        let handles_addr = config_handles.as_ptr() as usize;
        let n_worlds = config_handles.len();

        // Release GIL: murk_batched_create locks CONFIGS + BATCHED.
        let (status, batch_handle) = py.detach(|| {
            let mut bh: u64 = 0;
            let entries_ptr = if entries_addr == 0 {
                std::ptr::null()
            } else {
                entries_addr as *const MurkObsEntry
            };
            let s = murk_batched_create(
                handles_addr as *const u64,
                n_worlds,
                entries_ptr,
                n_entries,
                &mut bh,
            );
            (s, bh)
        });

        if let Err(e) = check_status(status) {
            // Free trampoline allocations.
            free_trampolines(&mut all_trampoline_data);
            return Err(e);
        }

        // Cache dimensions (brief lock, no GIL contention).
        let cached_num_worlds = murk_batched_num_worlds(batch_handle);
        let cached_obs_output_len = murk_batched_obs_output_len(batch_handle);
        let cached_obs_mask_len = murk_batched_obs_mask_len(batch_handle);

        Ok(BatchedWorld {
            handle: Some(batch_handle),
            cached_num_worlds,
            cached_obs_output_len,
            cached_obs_mask_len,
            trampoline_data: all_trampoline_data,
        })
    }

    /// Step all worlds and extract observations.
    ///
    /// Args:
    ///     commands_per_world: List of N lists of Command objects (one per world).
    ///         Use empty lists for worlds with no commands.
    ///     obs_output: Pre-allocated C-contiguous float32 array,
    ///         shape (num_worlds * obs_output_len,).
    ///     obs_mask: Pre-allocated C-contiguous uint8 array,
    ///         shape (num_worlds * obs_mask_len,).
    ///
    /// Returns:
    ///     List of per-world tick IDs.
    #[allow(unsafe_code)]
    fn step_and_observe<'py>(
        &self,
        py: Python<'py>,
        commands_per_world: Vec<Vec<PyRef<'py, Command>>>,
        obs_output: &Bound<'py, PyArray1<f32>>,
        obs_mask: &Bound<'py, PyArray1<u8>>,
    ) -> PyResult<Vec<u64>> {
        let h = self.require_handle()?;
        let n = self.cached_num_worlds;

        if commands_per_world.len() != n {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "commands_per_world has {} entries, expected {n}",
                commands_per_world.len()
            )));
        }
        if !obs_output.is_c_contiguous() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "obs_output must be C-contiguous",
            ));
        }
        if !obs_mask.is_c_contiguous() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "obs_mask must be C-contiguous",
            ));
        }

        // Convert commands for each world.
        let mut all_ffi_cmds: Vec<Vec<MurkCommand>> = Vec::with_capacity(n);
        for world_cmds in &commands_per_world {
            let ffi_cmds: Vec<MurkCommand> = world_cmds.iter().map(|c| c.inner).collect();
            all_ffi_cmds.push(ffi_cmds);
        }

        // Build pointer arrays for FFI.
        let cmd_ptrs: Vec<*const MurkCommand> = all_ffi_cmds
            .iter()
            .map(|cmds| {
                if cmds.is_empty() {
                    std::ptr::null()
                } else {
                    cmds.as_ptr()
                }
            })
            .collect();
        let n_cmds: Vec<usize> = all_ffi_cmds.iter().map(|cmds| cmds.len()).collect();
        let mut tick_ids = vec![0u64; n];

        // Pointer addresses as usize for Ungil closure.
        let cmd_ptrs_addr = cmd_ptrs.as_ptr() as usize;
        let n_cmds_addr = n_cmds.as_ptr() as usize;
        let out_addr = unsafe { obs_output.as_array_mut().as_mut_ptr() } as usize;
        let out_len = obs_output.len();
        let mask_addr = unsafe { obs_mask.as_array_mut().as_mut_ptr() } as usize;
        let mask_len = obs_mask.len();
        let tick_ids_addr = tick_ids.as_mut_ptr() as usize;

        // Release GIL: single detach covers step + observe for all worlds.
        let status = py.detach(|| {
            murk_batched_step_and_observe(
                h,
                cmd_ptrs_addr as *const *const MurkCommand,
                n_cmds_addr as *const usize,
                out_addr as *mut f32,
                out_len,
                mask_addr as *mut u8,
                mask_len,
                tick_ids_addr as *mut u64,
            )
        });
        check_status(status)?;

        Ok(tick_ids)
    }

    /// Extract observations from all worlds without stepping.
    #[allow(unsafe_code)]
    fn observe_all<'py>(
        &self,
        py: Python<'py>,
        obs_output: &Bound<'py, PyArray1<f32>>,
        obs_mask: &Bound<'py, PyArray1<u8>>,
    ) -> PyResult<()> {
        let h = self.require_handle()?;

        if !obs_output.is_c_contiguous() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "obs_output must be C-contiguous",
            ));
        }
        if !obs_mask.is_c_contiguous() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "obs_mask must be C-contiguous",
            ));
        }

        let out_addr = unsafe { obs_output.as_array_mut().as_mut_ptr() } as usize;
        let out_len = obs_output.len();
        let mask_addr = unsafe { obs_mask.as_array_mut().as_mut_ptr() } as usize;
        let mask_len = obs_mask.len();

        let status = py.detach(|| {
            murk_batched_observe_all(
                h,
                out_addr as *mut f32,
                out_len,
                mask_addr as *mut u8,
                mask_len,
            )
        });
        check_status(status)
    }

    /// Reset one world by index.
    fn reset_world(&self, py: Python<'_>, index: usize, seed: u64) -> PyResult<()> {
        let h = self.require_handle()?;
        let status = py.detach(|| murk_batched_reset_world(h, index, seed));
        check_status(status)
    }

    /// Reset all worlds with per-world seeds.
    #[allow(unsafe_code)]
    fn reset_all(&self, py: Python<'_>, seeds: Vec<u64>) -> PyResult<()> {
        let h = self.require_handle()?;
        let n_seeds = seeds.len();
        let seeds_addr = seeds.as_ptr() as usize;

        let status = py.detach(|| {
            murk_batched_reset_all(h, seeds_addr as *const u64, n_seeds)
        });
        check_status(status)
    }

    /// Number of worlds in the batch.
    #[getter]
    fn num_worlds(&self) -> usize {
        self.cached_num_worlds
    }

    /// Per-world observation output length (f32 elements).
    #[getter]
    fn obs_output_len(&self) -> usize {
        self.cached_obs_output_len
    }

    /// Per-world observation mask length (bytes).
    #[getter]
    fn obs_mask_len(&self) -> usize {
        self.cached_obs_mask_len
    }

    /// Explicitly destroy the batched engine.
    fn destroy(&mut self, py: Python<'_>) {
        free_trampolines(&mut self.trampoline_data);
        if let Some(h) = self.handle.take() {
            py.detach(|| murk_batched_destroy(h));
        }
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (_exc_type=None, _exc_val=None, _exc_tb=None))]
    fn __exit__(
        &mut self,
        py: Python<'_>,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_val: Option<&Bound<'_, PyAny>>,
        _exc_tb: Option<&Bound<'_, PyAny>>,
    ) {
        self.destroy(py);
    }
}

impl BatchedWorld {
    fn require_handle(&self) -> PyResult<u64> {
        self.handle.ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("BatchedWorld already destroyed")
        })
    }
}

impl Drop for BatchedWorld {
    fn drop(&mut self) {
        free_trampolines(&mut self.trampoline_data);
        if let Some(h) = self.handle.take() {
            Python::attach(|py| {
                py.detach(|| murk_batched_destroy(h));
            });
        }
    }
}

/// Free trampoline data boxes (used on error cleanup).
#[allow(unsafe_code)]
fn free_trampolines(data: &mut Vec<usize>) {
    for addr in data.drain(..) {
        if addr != 0 {
            unsafe {
                drop(Box::from_raw(
                    addr as *mut crate::propagator::TrampolineData,
                ));
            }
        }
    }
}
