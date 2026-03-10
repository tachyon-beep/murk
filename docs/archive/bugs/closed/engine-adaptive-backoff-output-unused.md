# Bug Report — FIXED

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low
**Fixed:** 2026-02-19

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [x] RealtimeAsync

## Summary

The adaptive backoff state machine computes `effective_max_skew` on each tick but the result is never consumed by stall detection, which always uses a fixed `max_epoch_hold_ns` threshold, effectively disabling the intended false-positive mitigation.

## Root Cause

`self.backoff.record_tick(had_rejection)` was called at tick_thread.rs:202 but its return value was discarded. `check_stalled_workers()` used the fixed `self.max_epoch_hold_ns` for stall detection thresholds, ignoring the adaptive backoff state entirely.

## Fix Applied

1. **tick_thread.rs**: Removed `#[cfg(test)]` from `effective_max_skew()` and added `initial_max_skew()` accessor so the tick thread can read backoff state.

2. **tick_thread.rs**: Added `effective_hold_ns()` method that scales `max_epoch_hold_ns` by the ratio `effective_max_skew / initial_max_skew`. At rest this equals the base threshold; under sustained rejections it grows proportionally.

3. **tick_thread.rs**: Modified `check_stalled_workers()` to accept the effective hold threshold as a parameter, and wired the scaled threshold from `run()`.

## Behavior Change

- At rest (no rejections): threshold = `max_epoch_hold_ns` (unchanged)
- After backoff (e.g. effective=4, initial=2): threshold = `2x max_epoch_hold_ns`
- At max backoff (effective=10, initial=2): threshold = `5x max_epoch_hold_ns`

## Tests Added

- `backoff_output_scales_hold_threshold` — verifies effective_max_skew increases with rejections
- `initial_max_skew_accessor` — verifies the accessor returns config value
