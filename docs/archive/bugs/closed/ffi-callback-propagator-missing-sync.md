# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-ffi

## Engine Mode

- [x] Lockstep

## Summary

`CallbackPropagator` has `unsafe impl Send` but no `unsafe impl Sync` (propagator.rs:93-94). The `Propagator` trait requires `Send + 'static`, which is satisfied. However, the propagator's `step()` takes `&self` (shared reference), and if the engine or future code ever invokes `step()` from multiple threads on the same instance, the `*mut c_void` user_data pointer would be accessed from multiple threads without `Sync`.

Currently, `LockstepWorld` is behind a `Mutex`, so the lock serializes access. This is technically safe **today**, but the missing `Sync` impl means the compiler cannot catch future unsoundness if the design changes (e.g., RealtimeAsync mode calling propagators from a worker pool).

## Expected Behavior

Either `unsafe impl Sync for CallbackPropagator` with a safety comment documenting that C callers must ensure user_data is thread-safe, or explicit documentation that `CallbackPropagator` is deliberately `!Sync` and the `Mutex<LockstepWorld>` serialization is the load-bearing invariant.

## Actual Behavior

No `Sync` impl and no documentation of the deliberate omission.

## Additional Context

**Source:** murk-ffi audit, F-02
**File:** `crates/murk-ffi/src/propagator.rs:93-94`
