# Bug Report — FIXED

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low
**Fixed:** 2026-02-18

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [x] RealtimeAsync

## Summary

`WorldConfig::validate()` does not validate `BackoffConfig` invariants, allowing `initial_max_skew > max_skew_cap`, which causes the runtime decay reset path to exceed the documented skew cap.

## Root Cause

`validate()` checked 6 categories of config but skipped `BackoffConfig` entirely. The decay reset at `tick_thread.rs:103` set `effective_max_skew = config.initial_max_skew` without clamping, so if `initial_max_skew > max_skew_cap`, the tolerance would exceed the cap after decay.

## Fix Applied

1. **config.rs**: Added `InvalidBackoff` error variant and BackoffConfig validation in `validate()`:
   - `initial_max_skew <= max_skew_cap`
   - `backoff_factor` is finite and >= 1.0
   - `rejection_rate_threshold` in [0.0, 1.0]
   - `decay_rate >= 1`

2. **tick_thread.rs**: Defensive clamp on decay reset: `initial_max_skew.min(max_skew_cap)`.

## Tests Added

- `validate_backoff_initial_exceeds_cap_fails`
- `validate_backoff_nan_factor_fails`
- `validate_backoff_factor_below_one_fails`
- `validate_backoff_threshold_out_of_range_fails`
- `validate_backoff_zero_decay_rate_fails`
- `validate_valid_backoff_succeeds`
