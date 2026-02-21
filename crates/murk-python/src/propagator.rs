//! Python propagator trampoline: enables Python callables as propagators.
//!
//! A C trampoline function re-acquires the GIL to call Python, copying
//! engine buffers to/from numpy arrays.

use std::ffi::{c_void, CString};

use numpy::PyArrayMethods;
use pyo3::prelude::*;
use pyo3::types::PyList;

use murk_ffi::{murk_propagator_create, MurkPropagatorDef, MurkStepContext, MurkWriteDecl};

use crate::command::WriteMode;
use crate::config::Config;
use crate::error::check_status;

/// Data stored per Python propagator. Boxed and passed as `user_data`.
pub(crate) struct TrampolineData {
    callable: Py<PyAny>,
    reads: Vec<u32>,
    reads_previous: Vec<u32>,
    writes: Vec<u32>,
}

// SAFETY: Py<PyAny> is only accessed inside Python::attach() in the trampoline.
// The trampoline is only ever called from the same thread that released the GIL.
#[allow(unsafe_code)]
unsafe impl Send for TrampolineData {}
#[allow(unsafe_code)]
unsafe impl Sync for TrampolineData {}

/// Python propagator definition.
///
/// Wraps a Python callable as a propagator step function. The callable
/// receives (reads, reads_previous, writes, tick_id, dt, cell_count)
/// where reads/writes are lists of numpy arrays.
#[pyclass]
pub(crate) struct PropagatorDef {
    name: String,
    step_fn: Py<PyAny>,
    reads: Vec<u32>,
    reads_previous: Vec<u32>,
    writes: Vec<(u32, WriteMode)>, // (field_id, write_mode)
}

#[pymethods]
impl PropagatorDef {
    /// Create a propagator definition.
    ///
    /// Args:
    ///     name: Propagator name.
    ///     step_fn: Python callable `(reads, reads_prev, writes, tick_id, dt, cell_count) -> None`.
    ///     reads: List of field IDs to read (current tick).
    ///     reads_previous: List of field IDs to read (previous tick).
    ///     writes: List of (field_id, WriteMode) tuples.
    #[new]
    #[pyo3(signature = (name, step_fn, reads=vec![], reads_previous=vec![], writes=vec![]))]
    fn new(
        name: String,
        step_fn: Py<PyAny>,
        reads: Vec<u32>,
        reads_previous: Vec<u32>,
        writes: Vec<(u32, WriteMode)>,
    ) -> Self {
        PropagatorDef {
            name,
            step_fn,
            reads,
            reads_previous,
            writes,
        }
    }

    /// Register this propagator with a Config.
    ///
    /// Creates the C trampoline and adds the propagator to the config.
    /// The trampoline data is transferred to the World when it is created.
    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?; // Verify config still alive

        // Validate the name before allocating TrampolineData — CString::new
        // is fallible (interior NUL bytes) and we don't want to leak the Box
        // if it fails.
        let cname = CString::new(self.name.as_str())
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("invalid propagator name"))?;

        // Build TrampolineData and box it.
        let data = Box::new(TrampolineData {
            callable: self.step_fn.clone_ref(py),
            reads: self.reads.clone(),
            reads_previous: self.reads_previous.clone(),
            writes: self.writes.iter().map(|(fid, _)| *fid).collect(),
        });
        let data_ptr = Box::into_raw(data) as *mut c_void;

        // Build FFI write declarations.
        let ffi_writes: Vec<MurkWriteDecl> = self
            .writes
            .iter()
            .map(|(fid, mode)| MurkWriteDecl {
                field_id: *fid,
                mode: *mode as i32,
            })
            .collect();

        let def = MurkPropagatorDef {
            name: cname.as_ptr(),
            reads: if self.reads.is_empty() {
                std::ptr::null()
            } else {
                self.reads.as_ptr()
            },
            n_reads: self.reads.len(),
            reads_previous: if self.reads_previous.is_empty() {
                std::ptr::null()
            } else {
                self.reads_previous.as_ptr()
            },
            n_reads_previous: self.reads_previous.len(),
            writes: if ffi_writes.is_empty() {
                std::ptr::null()
            } else {
                ffi_writes.as_ptr()
            },
            n_writes: ffi_writes.len(),
            step_fn: Some(python_trampoline),
            user_data: data_ptr,
            scratch_bytes: 0,
        };

        let mut prop_handle: u64 = 0;
        let status = murk_propagator_create(&def, &mut prop_handle);
        if status != 0 {
            // Clean up on failure.
            #[allow(unsafe_code)]
            unsafe {
                drop(Box::from_raw(data_ptr as *mut TrampolineData));
            }
            check_status(status)?;
        }

        // Add propagator to config and store trampoline data address for cleanup.
        if let Err(e) = config.add_propagator_handle(py, prop_handle) {
            #[allow(unsafe_code)]
            unsafe {
                drop(Box::from_raw(data_ptr as *mut TrampolineData));
            }
            return Err(e);
        }
        config.trampoline_data.push(data_ptr as usize);

        Ok(())
    }
}

/// Create and register a Python propagator with a config.
///
/// Convenience function that combines PropagatorDef creation and registration.
#[pyfunction]
#[pyo3(signature = (config, name, step_fn, reads=vec![], reads_previous=vec![], writes=vec![]))]
pub(crate) fn add_propagator(
    py: Python<'_>,
    config: &mut Config,
    name: String,
    step_fn: Py<PyAny>,
    reads: Vec<u32>,
    reads_previous: Vec<u32>,
    writes: Vec<(u32, WriteMode)>,
) -> PyResult<()> {
    let def = PropagatorDef {
        name,
        step_fn,
        reads,
        reads_previous,
        writes,
    };
    def.register(py, config)
}

/// C trampoline: called by the engine for each tick.
///
/// Re-acquires the GIL (safe because the caller released it via `detach`),
/// copies engine buffers to numpy arrays, calls the Python callable, then
/// copies write results back to engine buffers.
///
/// # Safety
///
/// - `user_data` must be a valid `*mut TrampolineData` created by `register()`.
/// - `ctx` must be a valid `*const MurkStepContext` provided by the engine.
#[allow(unsafe_code)]
unsafe extern "C" fn python_trampoline(user_data: *mut c_void, ctx: *const MurkStepContext) -> i32 {
    if user_data.is_null() || ctx.is_null() {
        return -10; // PropagatorFailed
    }

    let data = unsafe { &*(user_data as *const TrampolineData) };
    let ctx = unsafe { &*ctx };

    // catch_unwind prevents panic from unwinding across the extern "C" boundary (UB).
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Python::attach(|py| match trampoline_inner(py, data, ctx) {
            Ok(()) => 0,
            Err(e) => {
                e.print(py);
                -10 // PropagatorFailed
            }
        })
    }))
    .unwrap_or(-10) // Panic caught; report as PropagatorFailed
}

#[allow(unsafe_code)]
fn trampoline_inner(py: Python<'_>, data: &TrampolineData, ctx: &MurkStepContext) -> PyResult<()> {
    // Build read arrays (copies from engine buffers).
    let reads = PyList::empty(py);
    for &field_id in &data.reads {
        let mut ptr: *const f32 = std::ptr::null();
        let mut len: usize = 0;
        let rc = unsafe { (ctx.read_fn)(ctx.opaque, field_id, &mut ptr, &mut len) };
        if rc != 0 {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(format!(
                "read field {field_id} failed: {rc}"
            )));
        }
        let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
        let arr = numpy::PyArray1::from_slice(py, slice);
        reads.append(arr)?;
    }

    // Build reads_previous arrays (copies).
    let reads_prev = PyList::empty(py);
    for &field_id in &data.reads_previous {
        let mut ptr: *const f32 = std::ptr::null();
        let mut len: usize = 0;
        let rc = unsafe { (ctx.read_previous_fn)(ctx.opaque, field_id, &mut ptr, &mut len) };
        if rc != 0 {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(format!(
                "read_previous field {field_id} failed: {rc}"
            )));
        }
        let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
        let arr = numpy::PyArray1::from_slice(py, slice);
        reads_prev.append(arr)?;
    }

    // Build write arrays (copies that Python can modify).
    let write_arrays = PyList::empty(py);
    let mut write_addrs: Vec<(usize, usize)> = Vec::with_capacity(data.writes.len());
    for &field_id in &data.writes {
        let mut ptr: *mut f32 = std::ptr::null_mut();
        let mut len: usize = 0;
        let rc = unsafe { (ctx.write_fn)(ctx.opaque, field_id, &mut ptr, &mut len) };
        if rc != 0 {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(format!(
                "write field {field_id} failed: {rc}"
            )));
        }
        write_addrs.push((ptr as usize, len));
        let slice = unsafe { std::slice::from_raw_parts(ptr, len) };
        let arr = numpy::PyArray1::from_slice(py, slice);
        write_arrays.append(arr)?;
    }

    // Call the Python function.
    data.callable.call1(
        py,
        (
            reads,
            reads_prev,
            &write_arrays,
            ctx.tick_id,
            ctx.dt,
            ctx.cell_count,
        ),
    )?;

    // Write back modified data from numpy arrays to engine buffers.
    for (i, &(addr, len)) in write_addrs.iter().enumerate() {
        let item = write_arrays.get_item(i)?;
        let arr = item.cast::<numpy::PyArray1<f32>>()?;
        let readonly = unsafe { arr.as_slice()? };
        if readonly.len() != len {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "propagator writes[{i}]: expected length {len}, got {}. \
                 Do not resize write arrays inside the propagator.",
                readonly.len()
            )));
        }
        let dest = unsafe { std::slice::from_raw_parts_mut(addr as *mut f32, len) };
        dest.copy_from_slice(readonly);
    }

    Ok(())
}
