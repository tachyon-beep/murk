# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [x] RealtimeAsync

## Summary

Two related issues in `realtime.rs`:

1. **`observe()` (line 302-304):** If the egress worker returns a buffer larger than the caller's output, `output[..buf.len()].copy_from_slice(&buf)` panics with an index-out-of-bounds rather than returning an error.

2. **`observe_agents()` (line 367-370):** Uses `buf.len().min(output.len())` which avoids the panic but silently truncates data if the worker returns more than expected.

The two methods are inconsistent: one panics, the other silently truncates.

## Expected Behavior

Both should return `Err(ObsError::ExecutionFailed { reason: "output buffer too small" })` on size mismatch.

## Actual Behavior

`observe()` panics; `observe_agents()` silently truncates.

## Additional Context

**Source:** murk-engine audit, F-10/F-11
**Files:** `crates/murk-engine/src/realtime.rs:302-304`, `crates/murk-engine/src/realtime.rs:367-370`
