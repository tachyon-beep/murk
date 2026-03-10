# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [x] Critical | [ ] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-arena

## Engine Mode

- [x] Both / Unknown

## Summary

`PingPongArena::publish()` has no guard against being called without a preceding `begin_tick()`, or being called twice. The generation counter is incremented unconditionally in `publish()` via `self.generation += 1` (line 315), and this uses wrapping arithmetic — not `checked_add` like `begin_tick()` does. Two distinct issues:

1. **No state machine**: Nothing prevents calling `publish()` without `begin_tick()`, or calling it twice. This would advance the generation counter while handles in the published descriptor point to stale/never-allocated data.
2. **Unchecked overflow**: `begin_tick()` correctly uses `checked_add(1)` and returns an error at `u32::MAX`. But `publish()` uses `+= 1` which panics in debug and wraps silently in release.

**Note**: Related to closed #10 (arena-generation-counter-overflow) which fixed `begin_tick()`. The `publish()` path was not addressed.

## Steps to Reproduce

1. Create a `PingPongArena`.
2. Call `publish()` without calling `begin_tick()` first.
3. Generation counter advances but staging descriptor has stale handles.

## Expected Behavior

`publish()` should either require a `TickGuard` token (compile-time enforcement) or check a `tick_in_progress` flag and return an error.

## Actual Behavior

Generation counter silently advances, potentially producing snapshots with stale handles.

## Additional Context

**Source:** murk-arena audit, BUG-1 + BUG-2
**File:** `crates/murk-arena/src/pingpong.rs:314-315`
**Suggested fix:**
1. Add a `tick_in_progress: bool` flag, set true by `begin_tick()`, cleared by `publish()`.
2. Return error from `publish()` if flag is false.
3. Use `checked_add(1)` in `publish()` (or store the generation computed in `begin_tick()` and use it in `publish()`).
