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

`assert_canonical_ordering_complete` validates only that `canonical_ordering()`
returns `cell_count` unique coordinates, but never verifies that those coordinates
are actually valid cells of the space. A buggy Space implementation could return
the right number of unique but wholly incorrect coordinates and pass this
compliance check. Similarly, `assert_compile_region_all_covers_all` only checks
`plan.cell_count == space.cell_count()` without verifying coordinate content or
cross-checking against `canonical_ordering()`.

This is a gap in the compliance test harness, not a runtime bug in production
logic. All existing backends produce correct orderings.

## Steps to Reproduce

```rust
// Hypothetical: a Space impl where canonical_ordering() returns
// cell_count unique coords that don't match compile_region(All).coords
// would pass assert_canonical_ordering_complete and
// assert_compile_region_all_covers_all without error.
```

## Expected Behavior

The compliance suite should verify set equality between `canonical_ordering()`
output and `compile_region(All).coords`, ensuring both reference the same set
of cells.

## Actual Behavior

`assert_canonical_ordering_complete` (compliance.rs:76-91) checks only:
1. `ordering.len() == space.cell_count()` (cardinality)
2. All elements in `ordering` are unique (via `IndexSet`)

It does NOT check that the coordinates are valid cells or that they match
`compile_region(All).coords`.

`assert_compile_region_all_covers_all` (compliance.rs:106-117) checks only:
1. `plan.cell_count == space.cell_count()` (cardinality)

## Reproduction Rate

N/A -- this is a test coverage gap, not a runtime failure.

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
// No runtime reproducer -- this is a test harness gap.
// To demonstrate, one would need to create a deliberately broken
// Space impl that returns wrong coords with the right count.
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-space/src/compliance.rs.md
**Verified lines:** compliance.rs:76-91 (assert_canonical_ordering_complete), compliance.rs:106-117 (assert_compile_region_all_covers_all)
**Root cause:** Compliance checks validate cardinality-oriented invariants only, omitting set/content equality checks.
**Suggested fix:** Add a new compliance assertion (or extend existing ones) that verifies:
```rust
let ordering_set: IndexSet<_> = space.canonical_ordering().into_iter().collect();
let region_set: IndexSet<_> = space.compile_region(&RegionSpec::All)?.coords.into_iter().collect();
assert_eq!(ordering_set, region_set, "canonical_ordering and compile_region(All) must cover the same cells");
```
This would catch any implementation where the two code paths disagree on which cells exist.
