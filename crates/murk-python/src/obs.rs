//! PyObsPlan: observation plan compilation and execution with NumPy zero-copy.

use numpy::{PyArray1, PyArrayMethods};
use pyo3::prelude::*;

use murk_ffi::{
    murk_obsplan_compile, murk_obsplan_destroy, murk_obsplan_execute, murk_obsplan_mask_len,
    murk_obsplan_output_len, MurkObsEntry, MurkObsResult,
};

use crate::error::check_status;
use crate::world::World;

/// A single observation entry describing what to observe.
#[pyclass]
#[derive(Clone)]
pub(crate) struct ObsEntry {
    pub(crate) inner: MurkObsEntry,
}

#[pymethods]
impl ObsEntry {
    /// Create an observation entry.
    ///
    /// Args:
    ///     field_id: Field index to observe.
    ///     region_type: 0 = All (only option in v1).
    ///     transform_type: 0 = Identity, 1 = Normalize.
    ///     normalize_min: Lower bound for Normalize transform.
    ///     normalize_max: Upper bound for Normalize transform.
    ///     dtype: 0 = F32.
    #[new]
    #[pyo3(signature = (field_id, region_type=0, transform_type=0, normalize_min=0.0, normalize_max=1.0, dtype=0))]
    fn new(
        field_id: u32,
        region_type: i32,
        transform_type: i32,
        normalize_min: f32,
        normalize_max: f32,
        dtype: i32,
    ) -> Self {
        ObsEntry {
            inner: MurkObsEntry {
                field_id,
                region_type,
                transform_type,
                normalize_min,
                normalize_max,
                dtype,
            },
        }
    }
}

/// A compiled observation plan for efficient observation extraction.
///
/// Compile once, execute many times. Fills caller-allocated numpy buffers.
#[pyclass]
pub(crate) struct ObsPlan {
    handle: Option<u64>,
    cached_output_len: usize,
    cached_mask_len: usize,
}

#[pymethods]
impl ObsPlan {
    /// Compile an observation plan against a world.
    ///
    /// Args:
    ///     world: The world to compile against (for space topology).
    ///     entries: List of ObsEntry describing what to observe.
    #[new]
    fn new(py: Python<'_>, world: &World, entries: Vec<PyRef<'_, ObsEntry>>) -> PyResult<Self> {
        let world_h = world.handle()?;
        let ffi_entries: Vec<MurkObsEntry> = entries.iter().map(|e| e.inner).collect();

        let entries_addr = if ffi_entries.is_empty() {
            0usize
        } else {
            ffi_entries.as_ptr() as usize
        };
        let n_entries = ffi_entries.len();

        // Release GIL: murk_obsplan_compile locks OBS_PLANS + WORLDS.
        let (status, plan_h) = py.allow_threads(|| {
            let mut ph: u64 = 0;
            let ptr = if entries_addr == 0 {
                std::ptr::null()
            } else {
                entries_addr as *const MurkObsEntry
            };
            let s = murk_obsplan_compile(world_h, ptr, n_entries, &mut ph);
            (s, ph)
        });
        check_status(status)?;

        // These lock OBS_PLANS briefly but don't touch WORLDS.
        let output_len = murk_obsplan_output_len(plan_h);
        let mask_len = murk_obsplan_mask_len(plan_h);
        if output_len < 0 || mask_len < 0 {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "failed to query obs plan dimensions",
            ));
        }

        Ok(ObsPlan {
            handle: Some(plan_h),
            cached_output_len: output_len as usize,
            cached_mask_len: mask_len as usize,
        })
    }

    /// Execute the observation plan, filling pre-allocated numpy buffers.
    ///
    /// Args:
    ///     world: The world to observe.
    ///     output: Pre-allocated float32 array of shape (output_len,).
    ///     mask: Pre-allocated uint8 array of shape (mask_len,).
    ///
    /// Returns:
    ///     Tuple of (tick_id, age_ticks).
    #[allow(unsafe_code)]
    fn execute<'py>(
        &self,
        py: Python<'py>,
        world: &World,
        output: &Bound<'py, PyArray1<f32>>,
        mask: &Bound<'py, PyArray1<u8>>,
    ) -> PyResult<(u64, u64)> {
        let plan_h = self.require_handle()?;
        let world_h = world.handle()?;

        // Convert pointers to usize so the closure is Ungil.
        let out_addr = unsafe { output.as_array_mut().as_mut_ptr() } as usize;
        let out_len = output.len()?;
        let mask_addr = unsafe { mask.as_array_mut().as_mut_ptr() } as usize;
        let mask_len = mask.len()?;

        let mut result = MurkObsResult::default();
        let result_addr = &mut result as *mut MurkObsResult as usize;

        let status = py.allow_threads(|| {
            murk_obsplan_execute(
                world_h,
                plan_h,
                out_addr as *mut f32,
                out_len,
                mask_addr as *mut u8,
                mask_len,
                result_addr as *mut MurkObsResult,
            )
        });
        check_status(status)?;

        Ok((result.tick_id, result.age_ticks))
    }

    /// Number of f32 elements in the output buffer.
    #[getter]
    fn output_len(&self) -> usize {
        self.cached_output_len
    }

    /// Number of bytes in the mask buffer.
    #[getter]
    fn mask_len(&self) -> usize {
        self.cached_mask_len
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
        if let Some(h) = self.handle.take() {
            py.allow_threads(|| murk_obsplan_destroy(h));
        }
    }
}

impl ObsPlan {
    fn require_handle(&self) -> PyResult<u64> {
        self.handle.ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("ObsPlan already destroyed")
        })
    }
}

impl Drop for ObsPlan {
    fn drop(&mut self) {
        if let Some(h) = self.handle.take() {
            // Release GIL: murk_obsplan_destroy locks OBS_PLANS.
            Python::with_gil(|py| {
                py.allow_threads(|| murk_obsplan_destroy(h));
            });
        }
    }
}
