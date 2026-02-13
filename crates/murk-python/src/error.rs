//! MurkStatus -> Python exception mapping.

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::PyResult;

/// Check an FFI status code. Returns `Ok(())` on success, raises a Python
/// exception on error.
pub(crate) fn check_status(code: i32) -> PyResult<()> {
    if code == 0 {
        return Ok(());
    }
    let msg = match code {
        -1 => "invalid handle (already destroyed?)",
        -2 => "observation plan invalidated (space topology changed)",
        -3 => "timeout waiting for tick",
        -4 => "snapshot not available (evicted from ring buffer)",
        -5 => "invalid observation composition (valid_ratio below threshold)",
        -6 => "command queue full",
        -7 => "command is stale (basis_tick_id too old)",
        -8 => "tick was rolled back",
        -9 => "arena allocation failed (OOM)",
        -10 => "propagator step function failed",
        -11 => "observation execution failed",
        -12 => "invalid observation spec",
        -13 => "dt out of range for propagator constraint",
        -14 => "egress worker stalled (exceeded max_epoch_hold)",
        -15 => "world is shutting down",
        -16 => "ticking disabled (consecutive rollbacks)",
        -17 => "configuration error",
        -18 => "invalid argument",
        -19 => "caller-provided buffer too small",
        _ => "unknown murk error",
    };
    match code {
        -17 | -18 | -12 | -19 => Err(PyValueError::new_err(format!("murk error {code}: {msg}"))),
        _ => Err(PyRuntimeError::new_err(format!("murk error {code}: {msg}"))),
    }
}
