# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent (wave-5)
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-obs

## Engine Mode

- [x] Both / Unknown

## Summary

`GridGeometry::graph_distance` panics with index-out-of-bounds in the `Hex` branch when called with a `relative` slice containing fewer than 2 elements. The dimensionality is guarded only by `debug_assert_eq!` (removed in release builds), but the subsequent indexing (`relative[0]`, `relative[1]`) is unconditional.

The `FourWay` and `EightWay` branches handle arbitrary-length input gracefully via iterators, but the `Hex` branch uses direct indexing which panics for short inputs.

## Steps to Reproduce

1. Construct a hex geometry via `GridGeometry::from_space(&Hex2D::new(10, 10).unwrap())`.
2. Call `graph_distance(&[0])` or `graph_distance(&[])`.
3. Observe runtime panic from out-of-bounds indexing (release build).

## Expected Behavior

`graph_distance` should not panic on malformed input. It should either validate dimensionality with a runtime check (not just debug_assert) or handle the mismatch safely.

## Actual Behavior

Panics with `index out of bounds: the len is N but the index is 1` in release builds. In debug builds, the `debug_assert_eq!` triggers first with a more informative message.

## Reproduction Rate

Always (deterministic for `relative.len() < 2` on Hex geometry).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.9)

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

// Panics in release: index out of bounds
let _ = geo.graph_distance(&[0]);
let _ = geo.graph_distance(&[]);
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-obs/src/geometry.rs.md` (first report)

**Affected lines:**
- `crates/murk-obs/src/geometry.rs:161-170` (Hex branch of graph_distance)

**Internal callers are safe:** The only internal call site (plan.rs:1264) always passes `relative` vectors built from `half_extent.len()` which matches the space dimensionality. The risk is to external callers of this `pub` function on a `pub` struct.

**Root cause:** The `debug_assert_eq!` at line 163 provides no protection in release builds, and the direct indexing at lines 164-165 is unconditional.

**Suggested fix:** Replace `debug_assert_eq!` with a runtime `assert_eq!`:
```rust
GridConnectivity::Hex => {
    assert_eq!(relative.len(), 2, "Hex graph_distance requires 2D relative coords");
    let dq = relative[0];
    let dr = relative[1];
    // ...
}
```
Or refactor to return `Option<u32>` / `Result<u32, _>` if the API should handle misuse gracefully.
