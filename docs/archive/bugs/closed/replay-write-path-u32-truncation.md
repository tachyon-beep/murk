# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-replay

## Engine Mode

- [x] Both / Unknown

## Summary

Multiple places in `codec.rs` cast `usize` to `u32` with `as u32` without bounds checking on the write path:

- `write_length_prefixed_str` (line 56): `s.len() as u32`
- `write_length_prefixed_bytes` (line 63): `b.len() as u32`
- `encode_frame` (line 201): `frame.commands.len() as u32`
- `serialize_coord` (line 307): `coord.len() as u32`
- `serialize_command` (lines 361, 386, 398): `field_values.len()`, `data.len()`, `params.len()` as u32

On a 64-bit platform, if any of these values exceed `u32::MAX`, the length prefix silently wraps, and deserialization will read fewer items than were written -- silent data corruption in a crate focused on deterministic replay integrity.

## Expected Behavior

Assert or return `ReplayError` before each `as u32` cast if the value exceeds `u32::MAX`.

## Actual Behavior

Silent truncation via `as u32`.

## Additional Context

**Source:** murk-replay audit, Finding 2.2
**File:** `crates/murk-replay/src/codec.rs` (lines 56, 63, 201, 307, 361, 386, 398)
