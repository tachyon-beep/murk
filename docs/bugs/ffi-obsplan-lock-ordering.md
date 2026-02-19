# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-ffi

## Engine Mode

- [x] Both / Unknown

## Summary

`murk_obsplan_execute()` at `obs.rs:221-248` acquires locks in order: `OBS_PLANS` -> `WORLDS` -> `world_arc`. The `OBS_PLANS` lock is held for the entire duration including the world lock acquisition and observation execution. If any other code path acquires these locks in a different order, deadlock occurs. The ordering is implicit and undocumented.

Additionally, holding `OBS_PLANS` during the full observation execution prevents all other obs plan operations from making progress, unnecessarily limiting concurrency.

## Expected Behavior

Document the lock ordering invariant. Restructure to hold `OBS_PLANS` briefly (clone/extract what's needed, drop the lock, then execute), similar to how `WORLDS` is handled.

## Actual Behavior

`OBS_PLANS` held for full step+observe duration; lock ordering undocumented.

## Additional Context

**Source:** murk-ffi audit, F-08
**File:** `crates/murk-ffi/src/obs.rs:221-248`
