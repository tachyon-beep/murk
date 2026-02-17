# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [x] murk-space
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

`Hex2D::compile_hex_disk` can overflow signed `i64` arithmetic when computing
`side * side` (bounding area) for large effective radius values. The effective
radius is clamped to `i32::MAX`, but `side = 2 * i32::MAX + 1 = 4,294,967,295`
and `side^2 = 18,446,744,065,119,617,025`, which exceeds `i64::MAX`. This causes
a panic in debug builds or incorrect bounding tensor allocation in release builds.

## Steps to Reproduce

```rust
use murk_space::{Hex2D, Space};
use murk_space::region::RegionSpec;
use smallvec::smallvec;

// Large grid: construction succeeds
let hex = Hex2D::new(i32::MAX as u32, 1).unwrap();

// Disk with large radius: max_useful = i32::MAX, side * side overflows i64
let result = hex.compile_region(&RegionSpec::Disk {
    center: smallvec![0, 0],
    radius: u32::MAX,
});
// Panics in debug builds at line 157: attempt to multiply with overflow
```

## Expected Behavior

`compile_hex_disk` should detect the overflow and either:
- Clamp the effective radius more tightly (e.g., to `sqrt(i64::MAX)` or the actual grid diagonal)
- Use checked arithmetic and return `Err(SpaceError::InvalidRegion)` on overflow

## Actual Behavior

`side * side` overflows `i64` at hex2d.rs:157, causing a debug-mode panic or
silent wraparound in release mode. In release mode, the subsequent `vec![0u8; bounding_size]`
receives a garbage size (could be very large or very small due to wraparound).

## Reproduction Rate

100% deterministic when `effective_radius >= 46341` (where `(2*46341+1)^2 > i64::MAX`
is not true -- the actual threshold is `r >= 2^31`, i.e., when `side > 2^32`).

More precisely, overflow occurs when `r > (i64::MAX as f64).sqrt() as i32 / 2`,
approximately `r > 1_518_500_249`.

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
use murk_space::{Hex2D, Space};
use murk_space::region::RegionSpec;
use smallvec::smallvec;

let hex = Hex2D::new(i32::MAX as u32, 1).unwrap();
let result = hex.compile_region(&RegionSpec::Disk {
    center: smallvec![0, 0],
    radius: u32::MAX,
});
// Debug: panics with overflow
// Release: silent corruption of bounding_size
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-space/src/hex2d.rs.md
**Verified lines:** hex2d.rs:150-157, 173
**Root cause:** `max_useful` is clamped to `i32::MAX`, but the subsequent `side * side` computation in `i64` can still overflow because `(2 * i32::MAX + 1)^2 > i64::MAX`.
**Suggested fix:** Either:
1. Tighten the `max_useful` clamp to ensure `(2 * eff_radius + 1)^2` fits in `i64` (i.e., `eff_radius <= (i64::MAX.isqrt() - 1) / 2`), OR
2. Use `u64` or `u128` for the bounding area computation with a checked conversion to `usize`, returning `SpaceError::InvalidRegion` on overflow.

The existing test `compile_region_disk_huge_radius_does_not_overflow` passes because it uses a small 3x3 grid where `max_useful = 6`. The overflow only manifests with very large grids combined with large radii.

Practically unreachable with realistic grid sizes.
