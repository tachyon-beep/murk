# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [x] RealtimeAsync

## Summary

In `egress.rs:128`, `execute_task` is passed `epoch_counter.current()` as the tick value rather than the snapshot's actual tick ID. The epoch counter is a global monotonic value that may have advanced between `ring.latest()` and `epoch_counter.current()`. This means the `engine_tick` metadata in `ObsMetadata` may not match the snapshot's actual tick, especially under high contention.

## Expected Behavior

Use `snapshot.tick_id()` for the observation metadata, since the snapshot already carries its own tick ID.

## Actual Behavior

`epoch_counter.current()` is used, which may diverge from the snapshot's tick under contention.

## Additional Context

**Source:** murk-engine audit, F-15
**File:** `crates/murk-engine/src/egress.rs:128`
**Suggested fix:** Replace `epoch_counter.current()` with `snapshot.tick_id().0` in the `execute_task` call.
