//! PyWorld: Python wrapper around the lockstep world FFI.
//!
//! All FFI calls release the GIL via `py.detach()` so other Python
//! threads can run while the simulation ticks. This prevents lock-ordering
//! deadlocks between the GIL and FFI-internal mutexes (WORLDS, CONFIGS).

use numpy::{PyArray1, PyArrayMethods, PyUntypedArrayMethods};
use pyo3::prelude::*;

use murk_ffi::{
    murk_current_tick, murk_is_tick_disabled, murk_lockstep_create, murk_lockstep_destroy,
    murk_lockstep_reset, murk_lockstep_step, murk_seed, murk_snapshot_read_field, MurkCommand,
    MurkReceipt, MurkStepMetrics,
};

use crate::command::{Command, Receipt};
use crate::config::Config;
use crate::error::check_status;
use crate::metrics::StepMetrics;

/// A lockstep simulation world.
///
/// Wraps the C FFI world handle. Created by consuming a `Config`.
/// All operations release the GIL before touching FFI global mutexes.
#[pyclass]
pub(crate) struct World {
    handle: Option<u64>,
    /// Stored as usize (cast from *mut TrampolineData) for Send+Sync.
    trampoline_data: Vec<usize>,
}

#[pymethods]
impl World {
    /// Create a new lockstep world from a Config (consumes the config).
    #[new]
    fn new(py: Python<'_>, config: &mut Config) -> PyResult<Self> {
        let (cfg_handle, trampoline_data) = config.take_handle()?;
        // Release GIL: murk_lockstep_create locks CONFIGS + WORLDS.
        let (status, world_handle) = py.detach(|| {
            let mut wh: u64 = 0;
            let s = murk_lockstep_create(cfg_handle, &mut wh);
            (s, wh)
        });
        if let Err(e) = check_status(status) {
            // Free trampoline allocations that were taken from config.
            let mut world = World {
                handle: None,
                trampoline_data,
            };
            world.free_trampolines();
            return Err(e);
        }
        Ok(World {
            handle: Some(world_handle),
            trampoline_data,
        })
    }

    /// Execute one simulation tick.
    ///
    /// Args:
    ///     commands: Optional list of Command objects to submit.
    ///
    /// Returns:
    ///     Tuple of (list[Receipt], StepMetrics).
    #[pyo3(signature = (commands=None))]
    fn step(
        &self,
        py: Python<'_>,
        commands: Option<Vec<PyRef<'_, Command>>>,
    ) -> PyResult<(Vec<Receipt>, StepMetrics)> {
        let h = self.require_handle()?;

        // Convert Python commands to FFI commands.
        let ffi_cmds: Vec<MurkCommand> = match &commands {
            Some(cmds) => cmds.iter().map(|c| c.inner).collect(),
            None => Vec::new(),
        };
        let n_cmds = ffi_cmds.len();

        // Allocate receipt buffer.
        let receipts_cap = n_cmds.max(1);
        let mut receipts_buf: Vec<MurkReceipt> = vec![
            MurkReceipt {
                accepted: 0,
                applied_tick_id: 0,
                reason_code: 0,
                command_index: 0,
            };
            receipts_cap
        ];
        let mut n_receipts: usize = 0;
        let mut metrics = MurkStepMetrics::default();

        // Convert raw pointers to usize so the closure is Ungil.
        let cmds_addr = if ffi_cmds.is_empty() {
            0usize
        } else {
            ffi_cmds.as_ptr() as usize
        };
        let receipts_addr = receipts_buf.as_mut_ptr() as usize;
        let n_receipts_addr = &mut n_receipts as *mut usize as usize;
        let metrics_addr = &mut metrics as *mut MurkStepMetrics as usize;

        // Release GIL: murk_lockstep_step locks WORLDS.
        let status = py.detach(|| {
            let cmds_ptr = if cmds_addr == 0 {
                std::ptr::null()
            } else {
                cmds_addr as *const MurkCommand
            };
            murk_lockstep_step(
                h,
                cmds_ptr,
                n_cmds,
                receipts_addr as *mut MurkReceipt,
                receipts_cap,
                n_receipts_addr as *mut usize,
                metrics_addr as *mut MurkStepMetrics,
            )
        });
        check_status(status)?;

        let receipts: Vec<Receipt> = receipts_buf[..n_receipts]
            .iter()
            .map(|r| Receipt::from_ffi(*r))
            .collect();
        let step_metrics = StepMetrics::from_ffi(&metrics, h);

        Ok((receipts, step_metrics))
    }

    /// Reset the world to tick 0 with a new seed.
    fn reset(&self, py: Python<'_>, seed: u64) -> PyResult<()> {
        let h = self.require_handle()?;
        // Release GIL: murk_lockstep_reset locks WORLDS.
        let status = py.detach(|| murk_lockstep_reset(h, seed));
        check_status(status)
    }

    /// Read a field from the current snapshot into a numpy array.
    ///
    /// Args:
    ///     field_id: Field index to read.
    ///     output: Pre-allocated **C-contiguous** numpy float32 array to fill.
    ///
    /// Raises:
    ///     ValueError: If `output` is not C-contiguous.
    #[allow(unsafe_code)]
    fn read_field<'py>(
        &self,
        py: Python<'py>,
        field_id: u32,
        output: &Bound<'py, PyArray1<f32>>,
    ) -> PyResult<()> {
        let h = self.require_handle()?;
        if !output.is_c_contiguous() {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "output array must be C-contiguous",
            ));
        }
        let buf_addr = unsafe { output.as_array_mut().as_mut_ptr() } as usize;
        let buf_len = output.len();
        // Release GIL: murk_snapshot_read_field locks WORLDS.
        let status = py
            .detach(|| murk_snapshot_read_field(h, field_id, buf_addr as *mut f32, buf_len));
        check_status(status)
    }

    /// Current tick ID (0 after construction or reset).
    #[getter]
    fn current_tick(&self, py: Python<'_>) -> PyResult<u64> {
        let h = self.require_handle()?;
        // Release GIL: murk_current_tick locks WORLDS.
        Ok(py.detach(|| murk_current_tick(h)))
    }

    /// The world's RNG seed.
    #[getter]
    fn seed(&self, py: Python<'_>) -> PyResult<u64> {
        let h = self.require_handle()?;
        // Release GIL: murk_seed locks WORLDS.
        Ok(py.detach(|| murk_seed(h)))
    }

    /// Whether ticking is disabled (consecutive rollbacks).
    #[getter]
    fn is_tick_disabled(&self, py: Python<'_>) -> PyResult<bool> {
        let h = self.require_handle()?;
        // Release GIL: murk_is_tick_disabled locks WORLDS.
        Ok(py.detach(|| murk_is_tick_disabled(h) != 0))
    }

    /// Explicitly destroy the world handle.
    fn destroy(&mut self, py: Python<'_>) {
        self.do_destroy_with_gil(py);
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
        self.do_destroy_with_gil(py);
    }
}

impl World {
    fn require_handle(&self) -> PyResult<u64> {
        self.handle
            .ok_or_else(|| pyo3::exceptions::PyRuntimeError::new_err("World already destroyed"))
    }

    pub(crate) fn handle(&self) -> PyResult<u64> {
        self.require_handle()
    }

    /// Destroy with GIL token available (explicit destroy / __exit__).
    #[allow(unsafe_code)]
    fn do_destroy_with_gil(&mut self, py: Python<'_>) {
        self.free_trampolines();
        if let Some(h) = self.handle.take() {
            // Release GIL: murk_lockstep_destroy locks WORLDS.
            py.detach(|| murk_lockstep_destroy(h));
        }
    }

    /// Free trampoline data boxes (plain heap dealloc, no mutex).
    #[allow(unsafe_code)]
    fn free_trampolines(&mut self) {
        for addr in self.trampoline_data.drain(..) {
            if addr != 0 {
                unsafe {
                    drop(Box::from_raw(
                        addr as *mut crate::propagator::TrampolineData,
                    ));
                }
            }
        }
    }
}

impl Drop for World {
    fn drop(&mut self) {
        self.free_trampolines();
        if let Some(h) = self.handle.take() {
            // In Drop, use with_gil to get a token then release it.
            // PyO3 Drop for #[pyclass] runs with GIL held.
            Python::attach(|py| {
                py.detach(|| murk_lockstep_destroy(h));
            });
        }
    }
}
