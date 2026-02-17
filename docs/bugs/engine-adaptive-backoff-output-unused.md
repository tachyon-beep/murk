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

The adaptive backoff state machine computes `effective_max_skew` on each tick but the result is never consumed by stall detection, which always uses a fixed `max_epoch_hold_ns` threshold, effectively disabling the intended false-positive mitigation.

## Steps to Reproduce

1. Start a `RealtimeAsyncWorld`.
2. Trigger worker force-unpins to activate the adaptive backoff.
3. `AdaptiveBackoff::record_tick()` returns an increasing `effective_max_skew` value.
4. However, `check_stalled_workers()` at tick_thread.rs:251-257 still compares against the fixed `self.max_epoch_hold_ns`, ignoring the adaptive output.

## Expected Behavior

The adaptive backoff output should feed into the stall detection threshold, increasing tolerance after force-unpin events to avoid cascading false positives (as documented at tick_thread.rs:31-34).

## Actual Behavior

`self.backoff.record_tick(had_rejection)` is called at tick_thread.rs:202 but its return value is discarded. Stall detection at tick_thread.rs:251-257 is hard-coded to `self.max_epoch_hold_ns` + `self.cancel_grace_ns`.

## Reproduction Rate

Always (the backoff output is never wired in)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [ ] Breaks bit-exact determinism
- [x] May cause excessive false-positive force-unpins under sustained load
- [ ] No determinism impact

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// Not directly reproducible in a unit test.
// The issue is structural: record_tick() return value is discarded
// at tick_thread.rs:202, and check_stalled_workers() never reads
// backoff.effective_max_skew.
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-engine/src/tick_thread.rs.md
**Verified lines:** tick_thread.rs:31-34 (backoff intent doc), tick_thread.rs:91-114 (computes and returns effective_max_skew), tick_thread.rs:201-202 (discards return value), tick_thread.rs:251-257 (uses fixed threshold)
**Root cause:** The adaptive-backoff state machine was implemented but its output was not wired into `check_stalled_workers`, leaving stall/unpin decisions hard-coded to static thresholds.
**Suggested fix:** Feed the backoff output into stall detection by computing an effective hold threshold from `effective_max_skew` and using it in `check_stalled_workers()` instead of only `max_epoch_hold_ns`.
