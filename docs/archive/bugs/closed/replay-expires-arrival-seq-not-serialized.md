# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-replay

## Engine Mode

- [x] Both / Unknown

## Summary

`serialize_command` (codec.rs:341-343) discards `expires_after_tick` and `arrival_seq` from commands. On deserialization, `expires_after_tick` is hardcoded to `TickId(u64::MAX)` ("never expires") and `arrival_seq` to `0`.

If the original simulation uses `expires_after_tick` for command expiry, replayed commands will never expire, causing divergent behavior. Similarly, `arrival_seq` is used for deterministic ordering within priority classes -- setting all to 0 could reorder commands differently on replay.

## Expected Behavior

Either serialize these fields for faithful replay, or document that replay normalizes command timing and the contract requires commands to be fed at exactly the right tick.

## Actual Behavior

Fields silently discarded; replay may diverge from recording if expiry or ordering are relied upon.

## Additional Context

**Source:** murk-replay audit, Finding 9.1
**Files:** `crates/murk-replay/src/codec.rs:341-343, 549-556`
