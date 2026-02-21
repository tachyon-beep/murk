# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-python

## Engine Mode

- [x] Lockstep

## Summary

`python_trampoline` (propagator.rs:183-198) is an `unsafe extern "C"` function with no `std::panic::catch_unwind`. If `Python::attach` itself panics, or if `trampoline_inner` panics (e.g., an `.expect()` or out-of-bounds index), the panic unwinds across the `extern "C"` boundary — undefined behavior.

This is the same class of bug as open #20 (ffi-mutex-poisoning-panic-in-extern-c) but in the Python bindings crate rather than the C FFI crate.

## Expected Behavior

`python_trampoline` should catch panics and return an error code (-10).

## Actual Behavior

Panics unwind across `extern "C"` boundary = UB.

## Additional Context

**Source:** murk-python audit, F-7
**File:** `crates/murk-python/src/propagator.rs:183-198`
**Suggested fix:**
```rust
unsafe extern "C" fn python_trampoline(user_data: *mut c_void, ctx: *const MurkStepContext) -> i32 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        Python::attach(|py| match trampoline_inner(py, data, ctx) {
            Ok(()) => 0,
            Err(e) => { e.print(py); -10 }
        })
    }));
    match result {
        Ok(code) => code,
        Err(_) => -10,
    }
}
```
