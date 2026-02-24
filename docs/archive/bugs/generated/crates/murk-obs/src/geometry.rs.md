# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [x] Critical | [ ] High | [ ] Medium | [ ] Low

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

`GridGeometry::graph_distance` for hex connectivity can panic in production when passed a relative coordinate with fewer than 2 dimensions.

## Steps to Reproduce

1. Construct a hex geometry via `GridGeometry::from_space(&Hex2D::new(...).unwrap())`.
2. Call `graph_distance(&[0])` (or `graph_distance(&[])`).
3. Observe runtime panic from out-of-bounds indexing.

## Expected Behavior

`graph_distance` should not panic on malformed input in production; it should validate dimensionality and handle it safely.

## Actual Behavior

`graph_distance` directly indexes `relative[0]` and `relative[1]` after a `debug_assert_eq!`, so release builds panic with index out-of-bounds when `relative.len() < 2`.

Evidence:
- `/home/john/murk/crates/murk-obs/src/geometry.rs:163`
- `/home/john/murk/crates/murk-obs/src/geometry.rs:164`
- `/home/john/murk/crates/murk-obs/src/geometry.rs:165`

## Reproduction Rate

Always

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

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
use murk_space::Hex2D;

let hex = Hex2D::new(10, 10).unwrap();
let geo = GridGeometry::from_space(&hex).unwrap();

// Panics: index out of bounds
let _ = geo.graph_distance(&[0]);
```

## Additional Context

Root cause: dimensionality is only guarded by `debug_assert_eq!` (removed in release), but indexing is unconditional.  
Suggested fix: add an explicit runtime length check before indexing (e.g., return a sentinel or refactor API to return `Option<u32>`/`Result<u32, _>`).

---

# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
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

`GridGeometry::is_interior` performs `i32` addition without overflow protection, which can return incorrect interior classification in release builds (and panic in overflow-checked/debug builds).

## Steps to Reproduce

1. Create a maximal valid grid geometry (`rows/cols == i32::MAX`) via `Square4::new`.
2. Use an in-bounds center near the upper boundary (`i32::MAX - 1`) with `radius = 2`.
3. Call `is_interior`; release build returns `true` even though it should be boundary (`false`).

## Expected Behavior

Coordinates near the upper edge should be classified as non-interior when `center + radius >= dim`.

## Actual Behavior

The expression `*c + r` can overflow `i32`. In release, it wraps and may bypass the boundary check, producing `true` incorrectly.

Evidence:
- `/home/john/murk/crates/murk-obs/src/geometry.rs:135`
- `/home/john/murk/crates/murk-obs/src/geometry.rs:139`
- `/home/john/murk/crates/murk-obs/src/geometry.rs:140`

## Reproduction Rate

Always (for overflowing inputs)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

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
use murk_space::{EdgeBehavior, Square4};

let s = Square4::new(i32::MAX as u32, i32::MAX as u32, EdgeBehavior::Absorb).unwrap();
let geo = GridGeometry::from_space(&s).unwrap();

// In-bounds center at upper edge on first axis.
let center = [i32::MAX - 1, 100];
let interior = geo.is_interior(&center, 2);

// Expected: false (near boundary). Release can return true due to overflow.
println!("{interior}");
```

## Additional Context

Root cause: unchecked `i32` arithmetic in boundary test (`*c + r`).  
Suggested fix: perform the comparison in a wider type (`i64`) or rewrite using checked arithmetic (`checked_add`) and treat overflow as boundary/non-interior.