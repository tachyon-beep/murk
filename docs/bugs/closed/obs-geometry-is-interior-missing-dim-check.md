# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [x] murk-obs
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

`GridGeometry::is_interior` does not validate that `center.len() == self.ndim`. Because it uses `center.iter().zip(&self.coord_dims)` (line 129), mismatched dimensions silently truncate the iteration. An empty `center` on a 2D grid causes the loop to iterate zero times and return `true`, incorrectly reporting the agent as interior. Additionally, `radius as i32` (line 128) performs unchecked narrowing of u32 to i32, which wraps for values > i32::MAX.

Note: The current internal callers (in `plan.rs:858`) always pass coordinates from the Space, which guarantees correct dimensionality. This bug is a defensive-programming gap in a `pub` API, not an active production failure.

## Steps to Reproduce

1. Create a `GridGeometry` for a 2D grid (e.g., from `Square4::new(20, 20, Absorb)`).
2. Call `geo.is_interior(&[], 3)` (empty center).
3. Observe it returns `true` instead of `false`.

## Expected Behavior

`is_interior` should return `false` for a center with wrong dimensionality, matching the behavior of `in_bounds` (line 106 checks `coord.len() != self.ndim`).

## Actual Behavior

Returns `true` for empty center because the `zip` loop iterates zero times.

## Reproduction Rate

- Deterministic for any `center.len() != self.ndim`.

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
use murk_obs::geometry::GridGeometry;
use murk_space::{Square4, EdgeBehavior};

let s = Square4::new(20, 20, EdgeBehavior::Absorb).unwrap();
let geo = GridGeometry::from_space(&s).unwrap();

// BUG: empty center reports as interior
assert!(geo.is_interior(&[], 3)); // returns true, should be false

// BUG: radius > i32::MAX wraps
assert!(!geo.is_interior(&[10, 10], u32::MAX)); // may return true due to wrapping
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-obs/src/geometry.rs.md
**Verified lines:** geometry.rs:124-135 (is_interior), geometry.rs:105-115 (in_bounds for comparison)
**Root cause:** Missing `center.len() == self.ndim` guard, unlike `in_bounds`. Unchecked `radius as i32` narrowing.
**Suggested fix:** Add `if center.len() != self.ndim { return false; }` at the start of `is_interior`. Use `i32::try_from(radius).unwrap_or(i32::MAX)` for the radius conversion.
