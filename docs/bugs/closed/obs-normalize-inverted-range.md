# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-obs

## Engine Mode

- [x] Both / Unknown

## Summary

`ObsTransform::Normalize { min, max }` does not validate that `min <= max`. If `min > max`, then `range = max - min` is negative, and the normalization formula `(raw - min) / range` inverts the mapping. The subsequent `clamp(0.0, 1.0)` collapses all inputs to either 0.0 or 1.0, silently destroying information.

Additionally, NaN values for `min` or `max` would produce NaN in all outputs.

## Expected Behavior

`ObsPlan::compile` should validate `min < max` (or `min <= max`) and return `ObsError::InvalidObsSpec`. The existing `min == max` case (which outputs 0.0) is handled at `plan.rs:1171-1184`.

## Actual Behavior

Inverted range silently accepted; all observation data collapsed to {0.0, 1.0}.

## Additional Context

**Source:** murk-obs audit, Finding 3
**Files:** `crates/murk-obs/src/spec.rs:179-184`, `crates/murk-obs/src/plan.rs:1171-1184`
