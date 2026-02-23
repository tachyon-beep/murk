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

Unchecked `u64` arithmetic in `TickThreadState::new` and `check_stalled_workers` can overflow (panic in debug, wrap in release), causing incorrect stall detection and force-unpin behavior.

## Steps to Reproduce

1. Create an `AsyncConfig` with `max_epoch_hold_ms = u64::MAX` or another value where `ms * 1_000_000` overflows `u64`.
2. Start a `RealtimeAsyncWorld`.
3. In debug builds, observe overflow panic. In release builds, the wrapped value causes incorrect stall threshold computation.

## Expected Behavior

Overflow-prone arithmetic should use `checked_mul`/`saturating_mul`, or config validation should reject values that would cause overflow.

## Actual Behavior

Four overflow sites in `tick_thread.rs`:
1. **Line 168:** `max_epoch_hold_ms * 1_000_000` -- overflows if `max_epoch_hold_ms > u64::MAX / 1_000_000` (approximately 18.4 trillion).
2. **Line 169:** `cancel_grace_ms * 1_000_000` -- same issue.
3. **Line 260:** `self.max_epoch_hold_ns * effective / initial` -- intermediate product can overflow.
4. **Line 284:** `effective_hold_ns + self.cancel_grace_ns` -- addition can overflow.

No validation on `AsyncConfig` prevents large `max_epoch_hold_ms` or `cancel_grace_ms` values from reaching these sites. The `AsyncConfig` struct (config.rs:56-66) has no `validate()` method.

## Reproduction Rate

Always (with overflowing inputs)

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
let async_cfg = AsyncConfig {
    worker_count: Some(1),
    max_epoch_hold_ms: u64::MAX, // overflows at * 1_000_000
    cancel_grace_ms: 10,
};
// Debug build: panic at tick_thread.rs:168
let _world = RealtimeAsyncWorld::new(valid_config(), async_cfg).unwrap();
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-engine/src/tick_thread.rs.md`

**Affected lines:**
- ms-to-ns conversion: `crates/murk-engine/src/tick_thread.rs:168-169`
- Effective hold computation: `crates/murk-engine/src/tick_thread.rs:260`
- Stall threshold addition: `crates/murk-engine/src/tick_thread.rs:284`

**Root cause:** No upper bound validation on `AsyncConfig::max_epoch_hold_ms` and `cancel_grace_ms`, combined with unchecked `u64` arithmetic in the conversion to nanoseconds and in the stall detection logic.

**Suggested fix:** Either:
1. Validate `max_epoch_hold_ms` and `cancel_grace_ms` upper bounds in `AsyncConfig` (reject values where `ms * 1_000_000 > u64::MAX`), or
2. Use `saturating_mul`/`saturating_add` throughout the stall detection code path so overflow produces a capped threshold rather than a panic or wraparound.

Option 1 is preferred since it catches misconfiguration early. A reasonable upper bound (e.g., `u64::MAX / 1_000_000`) prevents all four overflow sites.
