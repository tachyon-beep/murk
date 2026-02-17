# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
- [x] murk-replay
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

`compare_snapshot` reports `FieldDivergence` with hardcoded `recorded_value: 0.0` and `replayed_value: 0.0` for both field-length mismatches (line 88-89) and field-presence mismatches (line 98-99). This makes it impossible for consumers to distinguish "field missing from one side" from "both values were genuinely 0.0", and the reported values are factually incorrect for non-zero fields.

This is a reporting quality issue, not a correctness bug. The divergence is still detected; only the diagnostic detail is misleading.

## Steps to Reproduce

1. Create a recorded snapshot with field 0 having 10 elements.
2. Create a replayed snapshot with field 0 having 5 elements.
3. Call `compare_snapshot` with both.
4. Observe the length-mismatch `FieldDivergence` has `recorded_value: 0.0, replayed_value: 0.0`.

## Expected Behavior

Length and presence mismatches should be represented with a distinct divergence type or use `Option<f32>` to indicate absent values, so consumers can tell "missing data" from "zero-valued mismatch."

## Actual Behavior

All structural mismatches use sentinel `0.0` for both values, making them indistinguishable from a genuine value mismatch at cell 0.

## Reproduction Rate

- Deterministic for any length or presence mismatch.

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
use murk_replay::compare::compare_snapshot;
// Set up recorded snapshot with field 0 = [1.0, 2.0, 3.0]
// and replayed snapshot with field 0 = [1.0, 2.0] (shorter)
// The length-mismatch FieldDivergence will report:
//   recorded_value: 0.0, replayed_value: 0.0
// which is misleading since the actual recorded[2] was 3.0
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-replay/src/compare.rs.md
**Verified lines:** compare.rs:83-91 (length mismatch sentinel), compare.rs:93-101 (presence mismatch sentinel)
**Root cause:** `FieldDivergence` uses required `f32` fields for all divergence types, forcing sentinel values for structural mismatches.
**Suggested fix:** Either (a) make `recorded_value`/`replayed_value` into `Option<f32>`, or (b) add a `DivergenceKind` enum (`ValueMismatch`, `LengthMismatch`, `FieldMissing`) to `FieldDivergence` with variant-specific payloads.
