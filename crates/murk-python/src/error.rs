//! MurkStatus -> Python exception mapping with recovery hints.

use pyo3::exceptions::{PyMemoryError, PyRuntimeError, PyTimeoutError, PyValueError};
use pyo3::PyResult;

/// Error reference URL for detailed error descriptions.
const ERROR_REF_URL: &str =
    "https://github.com/tachyon-beep/murk/blob/main/docs/error-reference.md";

/// Check an FFI status code. Returns `Ok(())` on success, raises a typed
/// Python exception with recovery hints on error.
pub(crate) fn check_status(code: i32) -> PyResult<()> {
    if code == 0 {
        return Ok(());
    }
    let (msg, hint, section) = error_detail(code);
    let full =
        format!("murk error {code}: {msg}\n  Hint: {hint}\n  Ref:  {ERROR_REF_URL}#{section}");
    match code {
        // Validation / configuration (caller's fault) → ValueError
        -12 | -17 | -18 | -19 => Err(PyValueError::new_err(full)),

        // CFL / dt constraint → ValueError (caller can fix by adjusting config)
        -13 => Err(PyValueError::new_err(full)),

        // Allocation failure → MemoryError
        -9 => Err(PyMemoryError::new_err(full)),

        // Timeouts → TimeoutError
        -3 => Err(PyTimeoutError::new_err(full)),

        // Everything else → RuntimeError
        _ => Err(PyRuntimeError::new_err(full)),
    }
}

/// Returns `(message, recovery_hint, error_reference_anchor)` for each FFI
/// status code.
fn error_detail(code: i32) -> (&'static str, &'static str, &'static str) {
    match code {
        -1 => (
            "invalid handle (already destroyed?)",
            "The World or ObsPlan object has been destroyed. \
             Don't call .destroy() and then continue using the object. \
             If using a context manager, access is only valid inside the `with` block.",
            "configerror",
        ),
        -2 => (
            "observation plan invalidated (space or fields changed since compilation)",
            "Recompile the ObsPlan after calling world.reset(). \
             Create a new ObsPlan(world, entries) after any reset.",
            "obserror",
        ),
        -3 => (
            "timeout waiting for tick (RealtimeAsync only)",
            "The tick thread hasn't produced the requested tick yet. \
             Increase the timeout or check that the tick thread is running. \
             This error does not occur in Lockstep mode.",
            "obserror",
        ),
        -4 => (
            "snapshot not available (evicted from ring buffer)",
            "The snapshot was consumed before you read it. \
             Read observations promptly after each step(). In RL loops, \
             ensure no slow work (rendering, logging) happens between \
             step() and observation extraction.",
            "obserror",
        ),
        -5 => (
            "invalid observation composition (valid_ratio below 0.35 threshold)",
            "Too many ObsEntry items reference invalid fields or out-of-bounds \
             regions. Check that all field IDs in your ObsEntry list exist and \
             that region coordinates are within the space bounds.",
            "obserror",
        ),
        -6 => (
            "command queue full",
            "You're submitting commands faster than the tick engine drains them. \
             Reduce the number of commands per step(). The default queue \
             capacity is 1024 commands.",
            "ingresserror",
        ),
        -7 => (
            "command is stale (basis_tick_id too old)",
            "The command refers to a tick that's too far in the past. \
             Resubmit with a fresh basis tick. In RL, this usually means \
             the agent's observation-to-action loop is too slow.",
            "ingresserror",
        ),
        -8 => (
            "tick was rolled back due to propagator failure",
            "A propagator failed during step(). All writes from that tick \
             were discarded. Check your propagator logic for NaN, division \
             by zero, or constraint violations. The next step() may succeed \
             if the failure was transient.",
            "steperror",
        ),
        -9 => (
            "arena allocation failed (out of memory)",
            "The simulation arena ran out of memory during tick staging. \
             Reduce cell_count or the number of fields. If using \
             RealtimeAsync, ensure epoch reclamation is not stalled \
             (egress workers must complete promptly).",
            "arenaerror",
        ),
        -10 => (
            "propagator step function failed",
            "A propagator's step() returned an error. Common causes: \
             NaN in field data, numerical instability, or a bug in \
             a Python propagator callback. Check the propagator that \
             writes to the fields mentioned in any preceding warnings.",
            "propagatorerror",
        ),
        -11 => (
            "observation execution failed",
            "ObsPlan.execute() failed mid-extraction. The snapshot may \
             have been reclaimed during execution. Retry with a fresh \
             step() and call execute() immediately afterwards.",
            "obserror",
        ),
        -12 => (
            "invalid observation spec",
            "The ObsEntry list has structural errors. Check that: \
             (1) field IDs are valid (0-based index from add_field order), \
             (2) region_params match the region_type requirements, \
             (3) transform parameters are valid (e.g. Normalize min < max).",
            "obserror",
        ),
        -13 => (
            "dt exceeds propagator CFL stability constraint",
            "Your configured dt is larger than a propagator's max_dt(). \
             Call config.set_dt() with a smaller value. For diffusion on \
             a 4-connected grid, max_dt = 1/(2*D*ndim) where D is the \
             diffusion coefficient. Check each propagator's max_dt().",
            "steperror",
        ),
        -14 => (
            "egress worker stalled (exceeded max_epoch_hold)",
            "An observation worker held its epoch pin too long (>100ms), \
             blocking arena garbage collection. Simplify the observation \
             spec (fewer entries, smaller regions) to reduce extraction \
             time. This error only occurs in RealtimeAsync mode.",
            "obserror",
        ),
        -15 => (
            "world is shutting down",
            "The World is in its shutdown sequence. This is expected after \
             calling world.destroy() or dropping the World. Create a new \
             World if you need to continue simulating.",
            "steperror",
        ),
        -16 => (
            "ticking disabled after consecutive rollbacks",
            "The tick engine entered fail-stop mode because too many \
             consecutive ticks failed. The simulation must be reset \
             via world.reset() or reconstructed. Investigate the root \
             cause of repeated propagator failures.",
            "steperror",
        ),
        -17 => (
            "configuration error",
            "WorldConfig validation failed at construction time. Common \
             causes: no fields defined, no propagators, empty space, \
             write conflicts between propagators, or invalid dt. Check \
             that you called add_field(), registered at least one \
             propagator, and set a valid space before creating the World.",
            "configerror",
        ),
        -18 => (
            "invalid argument",
            "A function argument is null or out of range. Check that \
             coordinates are within space bounds, field IDs are valid, \
             and buffer sizes are correct.",
            "configerror",
        ),
        -19 => (
            "caller-provided buffer too small",
            "The NumPy array you passed is too small for the output. \
             For ObsPlan.execute(), the obs buffer must have length >= \
             plan.output_len. For execute_agents(), it must be >= \
             n_agents * plan.output_len. Check plan.output_len and \
             plan.mask_len to allocate correctly sized arrays.",
            "configerror",
        ),
        _ => (
            "unknown murk error",
            "An unrecognized error code was returned from the FFI layer. \
             This may indicate a version mismatch between the Python \
             bindings and the native library.",
            "steperror",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_known_codes_have_detail() {
        for code in -19..=-1 {
            let (msg, hint, section) = error_detail(code);
            assert!(!msg.is_empty(), "code {code} has empty msg");
            assert!(!hint.is_empty(), "code {code} has empty hint");
            assert!(!section.is_empty(), "code {code} has empty section");
        }
    }

    #[test]
    fn unknown_code_returns_fallback() {
        let (msg, hint, section) = error_detail(-999);
        assert!(msg.contains("unknown"));
        assert!(hint.contains("version mismatch"));
        assert!(!section.is_empty());
    }

    #[test]
    fn error_ref_url_is_valid() {
        assert!(ERROR_REF_URL.starts_with("https://"));
        assert!(ERROR_REF_URL.contains("error-reference.md"));
    }
}
