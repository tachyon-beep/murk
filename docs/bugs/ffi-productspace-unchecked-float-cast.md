# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [x] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [x] Lockstep
- [x] RealtimeAsync
- [x] Both / Unknown

## Summary

`parse_space()` in `config.rs` casts untrusted `f64` FFI parameters to `usize` and `u32` using `as` without validation. For `ProductSpace`, `n_components` (line 151) and `n_comp_params` (line 162) are cast from `f64` to `usize` with no bounds checking. Values like `f64::INFINITY`, `f64::NAN`, or large negative numbers produce pathological `usize` values (e.g., `usize::MAX`), which then trigger panics in `Vec::with_capacity(n_components)` (capacity overflow) or in arithmetic/slice operations. Since `parse_space` is called from `murk_config_set_space` (an `extern "C"` function), the panic crosses the FFI boundary, which is undefined behavior in Rust and typically aborts the host process.

This also affects non-ProductSpace arms: all `p[i] as u32` casts (lines 87, 97, 104, 105, 115, 116, 127, 128, 138-140) can silently truncate or produce unexpected values from pathological `f64` inputs, though these are less likely to cause panics because they feed into constructors that return `Result`.

## Steps to Reproduce

```c
// C caller passes pathological f64 params for ProductSpace
double params[] = { 1.0/0.0 }; // INFINITY for n_components
murk_config_set_space(handle, MURK_SPACE_PRODUCT, params, 1);
// Process aborts due to capacity overflow panic
```

## Expected Behavior

`murk_config_set_space` should return `MURK_ERROR_INVALID_ARGUMENT` for non-finite, negative, or out-of-range parameter values.

## Actual Behavior

The function panics inside `Vec::with_capacity()` when `f64::INFINITY as usize` saturates to `usize::MAX`. The panic crosses the `extern "C"` boundary, which is undefined behavior and typically aborts the host process.

## Reproduction Rate

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_ffi::config::murk_config_set_space;
use murk_ffi::config::murk_config_create;
use murk_ffi::types::MurkSpaceType;

fn main() {
    let mut h: u64 = 0;
    murk_config_create(&mut h);
    // ProductSpace with INFINITY as n_components
    let params = [f64::INFINITY];
    // This will panic (capacity overflow) instead of returning InvalidArgument
    murk_config_set_space(h, MurkSpaceType::ProductSpace as i32, params.as_ptr(), 1);
}
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-ffi/src/config.rs.md
**Verified lines:** config.rs:146-175, config.rs:82-145, config.rs:193-222
**Root cause:** Unchecked `f64` to `usize`/`u32` `as` casts on untrusted FFI input. Rust's `as` cast for f64->usize saturates: `INFINITY` and large values become `usize::MAX`, `NAN` becomes `0`.
**Suggested fix:**
1. Add a helper function `f64_to_usize_checked(v: f64) -> Option<usize>` that rejects non-finite, negative, non-integer, and overly large values.
2. Use it for all size/count parameters in `parse_space`.
3. Cap `n_components` by remaining parameter length before allocating: `Vec::with_capacity(n_components.min(p.len()))`.
4. Use `offset.checked_add(n_comp_params)` and return `None` on overflow.
