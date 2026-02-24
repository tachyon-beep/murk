# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-ffi

## Engine Mode

- [x] Both / Unknown

## Summary

`batched.rs` handles mutex poisoning gracefully:
```rust
let mut table = match BATCHED.lock() {
    Ok(g) => g,
    Err(_) => return MurkStatus::InternalError as i32,
};
```

But `world.rs`, `config.rs`, `metrics.rs`, and `obs.rs` use `WORLDS.lock().unwrap()` which panics on poisoned mutex. Combined with the missing `catch_unwind` (see existing ticket #20), a panic in any FFI function poisons the mutex, then every subsequent call panics and unwinds through `extern "C"` -- UB cascade.

## Expected Behavior

Consistent `match lock() { Err(_) => InternalError }` pattern across all four global statics.

## Actual Behavior

Mixed: `batched.rs` is defensive, the rest `.unwrap()` and panic.

## Additional Context

**Source:** murk-ffi audit, F-06
**Related:** #20 (ffi-mutex-poisoning-panic-in-extern-c)
**Files:** `crates/murk-ffi/src/world.rs:27,46,80,134`, `crates/murk-ffi/src/config.rs:63`, `crates/murk-ffi/src/metrics.rs:56`, `crates/murk-ffi/src/obs.rs:181,197,221`
