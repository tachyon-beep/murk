# Bug Report

**Date:** 2026-02-23
**Reporter:** static-analysis-agent
**Severity:** [x] Critical | [ ] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [x] murk-engine
- [ ] murk-arena
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

- [ ] Lockstep
- [x] RealtimeAsync
- [ ] Both / Unknown

## Summary

Very small but positive `tick_rate_hz` values can panic the tick thread because `1.0 / tick_rate_hz` becomes `inf`, then `Duration::from_secs_f64` is called with `inf`.

## Steps to Reproduce

1. Create a valid `WorldConfig` baseline and set `tick_rate_hz = Some(f64::from_bits(1))` (smallest positive finite `f64`).
2. Call `RealtimeAsyncWorld::new(config, AsyncConfig::default())`.
3. Tick thread executes `TickThreadState::new` and hits `Duration::from_secs_f64(1.0 / tick_rate_hz)`.

## Expected Behavior

Configuration should be rejected up front (or safely clamped) so the tick thread never panics.

## Actual Behavior

Tick thread can panic during startup from invalid duration conversion, leaving realtime world in a broken state.

## Reproduction Rate

Always (with that input).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

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
use murk_engine::{RealtimeAsyncWorld, WorldConfig, AsyncConfig};

// Assume `valid_world_config()` builds any otherwise-valid config.
let mut cfg: WorldConfig = valid_world_config();
cfg.tick_rate_hz = Some(f64::from_bits(1)); // 5e-324, finite and > 0

let _world = RealtimeAsyncWorld::new(cfg, AsyncConfig::default()).unwrap();
// Tick thread path: tick_thread.rs:167 panics on Duration::from_secs_f64(inf)
```

## Additional Context

Evidence:
- `/home/john/murk/crates/murk-engine/src/tick_thread.rs:167`
- `/home/john/murk/crates/murk-engine/src/realtime.rs:171`
- `/home/john/murk/crates/murk-engine/src/config.rs:261`

Root cause: validation only checks `is_finite && > 0`, but does not guard reciprocal overflow (`1.0 / hz`) before `Duration::from_secs_f64`.  
Suggested fix: validate `tick_rate_hz >= 1.0 / Duration::MAX.as_secs_f64()` (or compute with checked logic) before constructing the tick budget.

---

# Bug Report

**Date:** 2026-02-23
**Reporter:** static-analysis-agent
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [x] murk-engine
- [ ] murk-arena
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

- [ ] Lockstep
- [x] RealtimeAsync
- [ ] Both / Unknown

## Summary

Unchecked `u64` arithmetic in stall-threshold math can overflow (panic in checked builds, wrap in release), causing incorrect stall detection and force-unpin behavior.

## Steps to Reproduce

1. Use a valid baseline config but set very large timing/backoff values (examples below).
2. Start `RealtimeAsyncWorld`.
3. Overflow occurs in one or more threshold computations in tick thread code.

## Expected Behavior

Threshold computations should be overflow-safe and either return config errors or saturate safely.

## Actual Behavior

Arithmetic can overflow in:
- ms→ns conversion
- scaled threshold computation
- grace-threshold addition  
This can panic or silently wrap, causing wrong cancellation/unpin decisions.

## Reproduction Rate

Always with overflowing inputs.

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

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
use murk_engine::{RealtimeAsyncWorld, WorldConfig, AsyncConfig, BackoffConfig};

let mut cfg: WorldConfig = valid_world_config();
cfg.backoff = BackoffConfig {
    initial_max_skew: 1_000_000_000_000,
    max_skew_cap: 1_000_000_000_000,
    backoff_factor: 1.0,
    decay_rate: 1,
    rejection_rate_threshold: 0.0,
};

let async_cfg = AsyncConfig {
    worker_count: Some(1),
    max_epoch_hold_ms: u64::MAX, // overflows at * 1_000_000
    cancel_grace_ms: 10,
};

let _world = RealtimeAsyncWorld::new(cfg, async_cfg).unwrap();
```

## Additional Context

Evidence:
- `/home/john/murk/crates/murk-engine/src/tick_thread.rs:168`
- `/home/john/murk/crates/murk-engine/src/tick_thread.rs:169`
- `/home/john/murk/crates/murk-engine/src/tick_thread.rs:260`
- `/home/john/murk/crates/murk-engine/src/tick_thread.rs:284`

Root cause: arithmetic uses plain `u64` `*` and `+` without `checked_*`/`saturating_*` or prior bounds checks.  
Suggested fix: validate upper bounds in config and switch to checked/saturating math (or `u128` intermediate with explicit clamp/error).