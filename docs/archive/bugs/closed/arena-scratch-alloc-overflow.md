# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [x] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [ ] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`ScratchRegion::alloc` uses unchecked `* 2` multiplication for capacity growth, which can overflow `usize` and cause a panic instead of returning `None` as the API contract promises.

## Steps to Reproduce

1. Create a `ScratchRegion`.
2. Call `alloc()` with a `len` value such that `new_cursor > usize::MAX / 2`.
3. The growth calculation `self.data.len().max(1024).max(new_cursor) * 2` overflows.
4. Debug builds: panic on arithmetic overflow. Release builds: wraparound causes `new_cap < new_cursor`, and `Vec::resize` may allocate too little, leading to out-of-bounds at line 54.

## Expected Behavior

Per the API contract (scratch.rs:40-43), `alloc` should return `None` when the scratch region cannot accommodate the request.

## Actual Behavior

At scratch.rs:48, the `* 2` multiplication is unchecked. While `checked_add` is used for `cursor + len` (line 45), the growth factor is not checked. For extremely large allocations (> `usize::MAX / 2` elements, i.e., > 16 EB on 64-bit or > 8 GB on 32-bit), this overflows instead of returning `None`.

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
use murk_arena::scratch::ScratchRegion;

let mut scratch = ScratchRegion::new(0);
// On 64-bit: usize::MAX / 2 + 1 = 9_223_372_036_854_775_808
// This is practically unreachable (16 EB of f32 data)
// On 32-bit: usize::MAX / 2 + 1 = 2_147_483_648 (~8 GB of f32 data)
let result = scratch.alloc(usize::MAX / 2 + 1);
// BUG: panics instead of returning None
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-arena/src/scratch.rs.md
**Verified lines:** scratch.rs:44-48 (alloc method with checked_add for cursor but unchecked * 2 for growth)
**Root cause:** Overflow-safe arithmetic was applied to `cursor + len` but not to the capacity growth factor.
**Suggested fix:** Replace `* 2` with `.checked_mul(2).unwrap_or(new_cursor)` or similar, ensuring that capacity growth cannot overflow. Alternatively, cap maximum scratch allocation to a configurable limit. Note: this is practically unreachable on 64-bit systems (would require > 16 exabytes), making this a low-severity correctness issue rather than a practical runtime risk.
