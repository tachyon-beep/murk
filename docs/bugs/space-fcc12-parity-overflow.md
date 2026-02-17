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

`Fcc12::check_bounds` and `Fcc12::canonical_ordering` compute `(x + y + z)` as
plain `i32` addition for parity checks. When the constructor allows dimensions up
to `i32::MAX` (2,147,483,647), coordinates near the upper bound can produce sums
exceeding `i32::MAX`, causing a signed integer overflow that panics in debug
builds or yields incorrect parity classification in release builds.

The same pattern affects `canonical_rank` (line 381, 419) and `compile_region`
Rect path (line 305).

## Steps to Reproduce

```rust
use murk_space::{Fcc12, EdgeBehavior, Space};
use smallvec::smallvec;

// Construction succeeds: cell count fits usize on 64-bit systems
let space = Fcc12::new(2_000_000_000, 200_000_000, 1, EdgeBehavior::Absorb).unwrap();

// This coordinate is valid: x=1_999_999_998 < w, y=199_999_998 < h, z=0 < d
// But x + y + z = 2_199_999_996, which overflows i32
let coord = smallvec![1_999_999_998i32, 199_999_998i32, 0i32];
let _ = space.distance(&coord, &coord); // may panic in debug builds
```

## Expected Behavior

Parity checks and canonical rank computation should not overflow for coordinates
that are within the constructor-validated bounds.

## Actual Behavior

`(x + y + z)` overflows `i32::MAX` in debug builds (panic) or wraps silently in
release builds (incorrect parity → wrong canonical rank, wrong neighbor filtering,
wrong region compilation).

## Reproduction Rate

100% deterministic for sufficiently large dimensions where coordinate sums exceed
`i32::MAX`.

## Environment

- **OS:** Any (64-bit required for construction to succeed)
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
// Note: requires 64-bit system; construction allocates a cell_count
// that fits usize but the coordinate arithmetic overflows i32.
use murk_space::{Fcc12, EdgeBehavior, Space};
use smallvec::smallvec;

let space = Fcc12::new(2_000_000_000, 200_000_000, 1, EdgeBehavior::Absorb).unwrap();
let coord = smallvec![1_999_999_998i32, 199_999_998i32, 0i32];
// Panics in debug: attempt to add with overflow
let result = space.canonical_rank(&coord);
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-space/src/fcc12.rs.md
**Verified lines:** fcc12.rs:169, 305, 350, 381, 419
**Root cause:** Parity checks sum `i32` coordinates directly (`(x + y + z) % 2`) without accounting for the full range allowed by the constructor (`MAX_DIM = i32::MAX`).
**Suggested fix:** Replace all parity-related signed additions with bitwise XOR logic:
- `fn parity3(x: i32, y: i32, z: i32) -> i32 { (x ^ y ^ z) & 1 }` -- overflow-free and equivalent to `(x + y + z) % 2` for parity.
- Use `(y ^ z) & 1` / `(x_lo ^ y ^ z) & 1` for `x_start` calculations in Rect compilation and canonical ordering.
- Alternatively, tighten the constructor to reject dimensions where `w + h + d > i32::MAX`, which would be a simpler but more restrictive fix.

Practically unreachable with realistic grid sizes but technically unsound for the full constructor-permitted range.
