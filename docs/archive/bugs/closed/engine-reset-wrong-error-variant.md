# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [x] RealtimeAsync

## Summary

`RealtimeAsyncWorld::reset()` at `realtime.rs:530` returns `ConfigError::InvalidTickRate { value: 0.0 }` when the engine cannot be recovered from the tick thread (e.g., tick thread panicked). This is misleading -- the actual error has nothing to do with the tick rate.

## Expected Behavior

A descriptive error variant like `ConfigError::EngineRecoveryFailed` or at minimum a `InvalidBackoff { reason: "..." }` stop-gap.

## Actual Behavior

Returns `InvalidTickRate { value: 0.0 }` which is confusing and misleading.

## Additional Context

**Source:** murk-engine audit, F-12
**File:** `crates/murk-engine/src/realtime.rs:530`
**Suggested fix:** Add a new `ConfigError::EngineRecoveryFailed` variant.
