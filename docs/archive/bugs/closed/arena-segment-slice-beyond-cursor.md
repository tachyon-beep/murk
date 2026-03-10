# Bug Report — FIXED

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low
**Fixed:** 2026-02-18

## Affected Crate(s)

- [x] murk-arena

## Engine Mode

- [x] Both / Unknown

## Summary

`Segment::slice` and `Segment::slice_mut` allowed access to memory beyond the bump-allocated cursor up to the full backing capacity, violating their documented contract and enabling reads of stale data after `reset()`.

## Root Cause

Bounds checking in `Segment::slice()` and `Segment::slice_mut()` was performed against `self.data.len()` (full Vec capacity) rather than `self.cursor` (logical allocation boundary). The doc promised "allocated region" checking but the code checked capacity.

Additionally, `PingPongArena::new()` and `PingPongArena::reset()` did not pre-allocate PerTick fields in the published buffer, leaving placeholder handles pointing into unallocated memory at generation 0.

## Fix Applied

1. **segment.rs**: Added `assert!(end <= self.cursor)` to both `Segment::slice()` and `Segment::slice_mut()` to enforce the documented contract.

2. **pingpong.rs**: Pre-allocate PerTick fields in both buffers at construction time (`new()`) and after `reset()`, so the published buffer is always valid from generation 0. This also resolves BUG-013 (placeholder PerTick handles in snapshot).

## Tests Added

- `segment_slice_panics_after_reset` — verifies panic on stale read after reset
- `segment_slice_mut_panics_after_reset` — same for mutable variant
- `segment_slice_panics_beyond_cursor` — verifies panic when reading beyond cursor but within capacity
- `segment_slice_within_cursor_succeeds` — positive test for valid access
- `segment_list_slice_panics_after_reset` — verifies SegmentList level propagation

## Cascade

The fix surfaced 4 pre-existing failures in `murk-engine` where code was silently reading unallocated memory at generation 0:
- `batched::tests::observe_all_after_reset`
- `tick::tests::reads_previous_sees_base_gen`
- `tick::tests::partial_failure_rolls_back_all`
- `tick::tests::writemode_incremental_seeds_from_previous_gen`

All resolved by the `PingPongArena` pre-allocation fix (item 2 above).
