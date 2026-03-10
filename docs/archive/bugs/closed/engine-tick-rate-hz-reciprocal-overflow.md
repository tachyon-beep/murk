# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [ ] Lockstep
- [x] RealtimeAsync
- [ ] Both / Unknown

## Summary

Very small but positive `tick_rate_hz` values pass config validation but cause a panic in the tick thread because `1.0 / tick_rate_hz` produces `inf`, and `Duration::from_secs_f64(inf)` panics.

## Steps to Reproduce

1. Create a `WorldConfig` with `tick_rate_hz = Some(f64::from_bits(1))` (smallest positive finite `f64`, approximately `5e-324`).
2. Call `RealtimeAsyncWorld::new(config, AsyncConfig::default())`.
3. The tick thread calls `TickThreadState::new` which computes `Duration::from_secs_f64(1.0 / tick_rate_hz)`.

## Expected Behavior

Configuration should be rejected up front with `ConfigError::InvalidTickRate`, or the reciprocal should be computed with overflow protection, so the tick thread never panics.

## Actual Behavior

- `config.rs:262-265` validates `!hz.is_finite() || hz <= 0.0` -- passes for very small positive finite values.
- `realtime.rs:170-174` repeats the same check -- passes.
- `tick_thread.rs:167` computes `Duration::from_secs_f64(1.0 / tick_rate_hz)`. Since `1.0 / 5e-324 = inf`, this panics.

The tick thread panics during startup, leaving the realtime world in a broken state.

## Reproduction Rate

Always (with subnormal or very small tick_rate_hz values)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** 0.1.8 / HEAD (feat/release-0.1.9)

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
let mut cfg = valid_world_config();
cfg.tick_rate_hz = Some(f64::from_bits(1)); // 5e-324, finite and > 0

// Tick thread panics at Duration::from_secs_f64(inf)
let _world = RealtimeAsyncWorld::new(cfg, AsyncConfig::default()).unwrap();
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-engine/src/tick_thread.rs.md`

**Affected lines:**
- Config validation (passes): `crates/murk-engine/src/config.rs:262-265`
- Realtime validation (passes): `crates/murk-engine/src/realtime.rs:170-174`
- Panic site: `crates/murk-engine/src/tick_thread.rs:167`

**Root cause:** The validation only rejects non-finite and non-positive values, but does not check that the reciprocal `1.0 / hz` is itself finite and representable as a `Duration`.

**Suggested fix:** Add a minimum `tick_rate_hz` bound in validation (e.g., `>= 0.001`, or check that `1.0 / hz` is finite) before constructing the tick budget. Alternatively, compute the reciprocal in the validation step and reject if it produces infinity or exceeds a reasonable maximum period.
