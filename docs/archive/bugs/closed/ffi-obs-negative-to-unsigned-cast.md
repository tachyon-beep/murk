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
- [x] murk-obs
- [ ] murk-replay
- [x] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [x] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`murk_obsplan_compile` in `obs.rs` casts `i32` region parameters and pool parameters to unsigned types using `as` without validating non-negativity:

- Line 110: `e.region_params[0] as u32` for `AgentDisk.radius` -- `-1i32 as u32` = `4294967295`
- Line 120: `e.region_params[..n].iter().map(|&v| v as u32)` for `AgentRect.half_extent` -- same issue for every element
- Line 151: `e.pool_kernel_size as usize` -- `-1i32 as usize` = `usize::MAX` on 64-bit
- Line 152: `e.pool_stride as usize` -- same

These pathological unsigned values propagate into `murk-obs` plan compilation where they drive shape calculations and memory allocation:
- `plan.rs:448/1070`: `2 * he as usize + 1` overflows for `he = u32::MAX`
- `plan.rs:1071`: `shape.iter().product()` produces overflow or enormous total
- `plan.rs:1075`: `Vec::with_capacity(total)` panics (capacity overflow or OOM)

The panic occurs inside `extern "C"` `murk_obsplan_compile`, which is UB.

## Steps to Reproduce

```c
MurkObsEntry entry = {
    .field_id = 0,
    .region_type = 5,  // AgentDisk
    .region_params = { -1 },  // negative radius
    .n_region_params = 1,
    .transform_type = 0,
    .dtype = 0,
    .pool_kernel = 0,
    .pool_kernel_size = 0,
    .pool_stride = 0,
};
uint64_t plan;
// Panics or OOM instead of returning MURK_ERROR_INVALID_ARGUMENT
murk_obsplan_compile(world_handle, &entry, 1, &plan);
```

## Expected Behavior

`murk_obsplan_compile` should validate that all size/radius-like parameters are non-negative before casting, and return `MURK_ERROR_INVALID_ARGUMENT` for negative values.

## Actual Behavior

Negative `i32` values are reinterpreted as very large `u32`/`usize` values via `as` casts, causing downstream capacity overflow panics or OOM in plan compilation. The panic crosses the FFI boundary (UB).

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
use murk_ffi::obs::{MurkObsEntry, murk_obsplan_compile};

fn trigger() {
    let entry = MurkObsEntry {
        field_id: 0,
        region_type: 5, // AgentDisk
        region_params: [-1, 0, 0, 0, 0, 0, 0, 0], // negative radius
        n_region_params: 1,
        transform_type: 0,
        normalize_min: 0.0,
        normalize_max: 0.0,
        dtype: 0,
        pool_kernel: 0,
        pool_kernel_size: 0,
        pool_stride: 0,
    };
    let mut plan: u64 = 0;
    // This will panic in plan compilation instead of returning an error
    murk_obsplan_compile(world_handle, &entry, 1, &mut plan);
}
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-ffi/src/obs.rs.md
**Verified lines:** obs.rs:109-111, obs.rs:119-121, obs.rs:150-153; plan.rs:448, plan.rs:1070-1075
**Root cause:** Unchecked `i32` to `u32`/`usize` `as` casts at the FFI boundary. Rust's `as` cast wraps for integer-to-integer: `-1i32 as u32 == u32::MAX`.
**Suggested fix:**
1. Validate all size-like `i32` fields are non-negative before casting: `if v < 0 { return MurkStatus::InvalidArgument as i32; }`
2. Use `u32::try_from(v).ok()` or manual bounds checks.
3. For `pool_kernel_size` and `pool_stride`, also validate they are positive (not zero) when pooling is enabled.
4. Add reasonable upper-bound caps (e.g., `radius <= 1024`) to prevent absurd allocations even with valid non-negative inputs.
