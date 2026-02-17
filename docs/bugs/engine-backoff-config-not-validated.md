# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [ ] Lockstep
- [x] RealtimeAsync
- [ ] Both / Unknown

## Summary

`WorldConfig::validate()` does not validate `BackoffConfig` invariants, allowing `initial_max_skew > max_skew_cap`, which causes the runtime reset path to exceed the documented skew cap.

## Steps to Reproduce

1. Construct a `WorldConfig` with `BackoffConfig { initial_max_skew: 100, max_skew_cap: 5, .. }`.
2. Call `validate()` -- it succeeds.
3. Start a `RealtimeAsyncWorld` and trigger the decay path in `AdaptiveBackoff::record_tick()`.
4. After `decay_rate` ticks with no rejections, `effective_max_skew` resets to `initial_max_skew` (100), which exceeds `max_skew_cap` (5).

## Expected Behavior

`WorldConfig::validate()` should reject configurations where `initial_max_skew > max_skew_cap`, or the runtime reset path should clamp to `max_skew_cap`.

## Actual Behavior

Validation passes silently. At runtime, the decay reset at `tick_thread.rs:103` sets `effective_max_skew = config.initial_max_skew` without clamping, so the effective skew tolerance exceeds the documented cap.

## Reproduction Rate

Always (with misconfigured BackoffConfig)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [ ] Breaks bit-exact determinism
- [x] May affect simulation behavior under edge-case configs
- [ ] No determinism impact

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_engine::config::{BackoffConfig, WorldConfig};

let config = WorldConfig {
    // ... valid fields ...
    backoff: BackoffConfig {
        initial_max_skew: 100,
        max_skew_cap: 5,
        backoff_factor: 1.5,
        decay_rate: 10,
        rejection_rate_threshold: 0.20,
    },
};
// This should fail but passes:
config.validate().unwrap();
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-engine/src/config.rs.md
**Verified lines:** config.rs:196-225 (validate method), tick_thread.rs:102-103 (decay reset path)
**Root cause:** Backoff parameters are treated as trusted input; no validation step checks the relationship between `initial_max_skew` and `max_skew_cap`.
**Suggested fix:** Add backoff validation in `WorldConfig::validate()` (reject `initial_max_skew > max_skew_cap`, non-finite `backoff_factor`, out-of-range `rejection_rate_threshold`). Optionally clamp the reset path at tick_thread.rs:103 as a defensive guard.
