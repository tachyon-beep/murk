# Bug Report — FIXED

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low
**Fixed:** 2026-02-18

## Affected Crate(s)

- [x] murk-arena

## Engine Mode

- [x] Both / Unknown

## Summary

`ArenaConfig` documents that `segment_size` must be a power of two and at least 1024, but no validation enforces these invariants, allowing invalid configs that produce misleading runtime errors.

## Root Cause

`PingPongArena::new()` only validated `max_segments >= 3`. The `segment_size` constraint documented on `ArenaConfig` was never checked, so invalid values (non-power-of-two, below 1024) were silently accepted, producing misleading downstream `CapacityExceeded` errors instead of the expected `InvalidConfig`.

## Fix Applied

Added `segment_size` validation at the start of `PingPongArena::new()`, before the existing `max_segments` check:
- `segment_size.is_power_of_two()` — required for alignment
- `segment_size >= 1024` — minimum documented floor

Returns `Err(ArenaError::InvalidConfig)` with a clear message on violation.

Also updated the existing `new_fails_when_sparse_field_exceeds_segment_size` test to use a valid `segment_size` (1024) with a larger `cell_count` (2000) to keep its intended coverage.

## Tests Added

- `new_rejects_non_power_of_two_segment_size` — segment_size=1000 rejected
- `new_rejects_segment_size_below_1024` — segment_size=512 rejected
- `new_accepts_segment_size_of_1024` — positive test
