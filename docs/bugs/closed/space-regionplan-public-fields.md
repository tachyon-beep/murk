# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-space

## Engine Mode

- [x] Both / Unknown

## Summary

`RegionPlan` (region.rs:42-54) has all 5 fields as `pub`, meaning any consumer can construct or mutate a `RegionPlan` with inconsistent data (e.g., `cell_count` not matching `coords.len()`, or `tensor_indices` out of sync with `valid_mask`). Additionally, `cell_count` is redundant with `coords.len()` -- having both invites desynchronization.

Similarly, `ProductMetric::Weighted` panics via `assert_eq!` in library code when weights length mismatches (product.rs:142-149) rather than returning `Result`.

## Expected Behavior

Make `RegionPlan` fields `pub(crate)` with accessor methods. Remove redundant `cell_count` field. Validate `ProductMetric::Weighted` weights at construction time.

## Actual Behavior

Structural invariants not enforced; library panics on invalid weighted metric.

## Additional Context

**Source:** murk-space audit, A-2/A-3/Q-3
**Files:** `crates/murk-space/src/region.rs:42-54`, `crates/murk-space/src/product.rs:142-149`
