# Bug Report

**Date:** 2026-02-21
**Reporter:** sparse-reclamation-review panel (QA reviewer)
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

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

- [x] Lockstep
- [ ] RealtimeAsync
- [ ] Both / Unknown

## Summary

Two test gaps in the sparse reclamation code path: (1) `alloc()` falling back to bump allocation when `retired_ranges` is non-empty but contains no size match, and (2) multiple same-sized sparse fields retiring and reclaiming ranges in interleaved order. Both are untested branches in the fix for bug #29 (arena-sparse-segment-memory-leak).

## Steps to Reproduce

N/A — test coverage gaps, not runtime bugs.

## Expected Behavior

Two tests should exist:

1. **`alloc_falls_back_to_bump_when_no_size_match`** (sparse.rs): Populate `retired_ranges` with size-100 entries, then request size-200. Assert: bump allocation occurs (total_used increases), retired_ranges untouched.

2. **`three_fields_same_size_all_ranges_retired`** (sparse.rs): Three fields of equal size, each CoW'd once. Flush retired. Assert: all three retired ranges are consumed when the three fields are CoW'd again.

## Actual Behavior

Both code paths work correctly (verified by the panel's structural analysis) but have no dedicated test coverage. In a codebase where segment exhaustion was the original bug, untested allocation branches warrant explicit regression tests.

## Reproduction Rate

Always

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
N/A — test gap
```

## Minimal Reproducer

```
N/A
```

## Additional Context

**Origin:** QA reviewer finding from sparse-reclamation-review panel (2026-02-21). Classified as Important (not Critical) after cross-challenge with architect and systems-thinker.

**Risk context:** The no-match fallback can only fall through to bump allocation, which either succeeds or returns `CapacityExceeded`. Neither outcome is silent. The systemic limitation (no reclaim path for size-mismatched entries) is bounded to architecturally-prevented scenarios (field sizes are fixed at Config time).
