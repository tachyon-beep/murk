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

`RealtimeAsyncWorld::shutdown()` can block far longer than the documented budget when `tick_rate_hz` is configured very low, because the tick thread sleeps with `std::thread::sleep` (uninterruptible by the shutdown flag) and `shutdown()` does an unbounded `join()`.

## Steps to Reproduce

1. Create a `RealtimeAsyncWorld` with `tick_rate_hz = 0.1` (10-second tick budget).
2. Let it run for at least one tick.
3. Call `shutdown()`.
4. The tick thread may be mid-sleep in a 10-second `std::thread::sleep()` call (tick_thread.rs:215-217).
5. `shutdown()` sets the flag, waits 33ms for `tick_stopped`, times out, then blocks on `handle.join()` (realtime.rs:474-482) for up to 10 seconds.

## Expected Behavior

Shutdown should complete within the documented budget (roughly 243ms total across all phases), regardless of `tick_rate_hz`.

## Actual Behavior

Shutdown blocks for up to the full tick budget duration (e.g., 10 seconds at 0.1 Hz) because `std::thread::sleep` is not interruptible by the shutdown flag.

## Reproduction Rate

Always (with low tick_rate_hz)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [ ] Breaks bit-exact determinism
- [ ] May affect simulation behavior
- [x] No determinism impact (shutdown latency only)

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_engine::{RealtimeAsyncWorld, WorldConfig, AsyncConfig};
use std::time::Instant;

let config = WorldConfig {
    // ... valid fields ...
    tick_rate_hz: Some(0.1),  // 10-second tick budget
    // ...
};
let mut world = RealtimeAsyncWorld::new(config, AsyncConfig::default())?;
std::thread::sleep(std::time::Duration::from_millis(500));

let start = Instant::now();
let report = world.shutdown();
// report.total_ms may be ~10000ms instead of <250ms
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-engine/src/realtime.rs.md
**Verified lines:** realtime.rs:135-140 (accepts any positive finite tick_rate_hz), realtime.rs:474-482 (unbounded join), tick_thread.rs:214-217 (uninterruptible sleep)
**Root cause:** Shutdown signaling is flag-based, but the tick loop uses coarse, uninterruptible sleeps tied to `tick_rate_hz`; there is no wake-up mechanism.
**Suggested fix:** Make tick sleeping shutdown-aware (e.g., sleep in short chunks while polling `shutdown_flag`, or use a condvar/channel wait with timeout). Optionally enforce a minimum supported tick rate.
