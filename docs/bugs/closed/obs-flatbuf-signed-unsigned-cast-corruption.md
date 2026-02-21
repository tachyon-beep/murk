# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-obs

## Engine Mode

- [x] Both / Unknown

## Summary

During flatbuf deserialization of region parameters, negative `i32` values are cast to `u32` using `as u32`, which wraps silently. For example, a Disk radius stored as `-1i32` becomes `u32::MAX` (4294967295). The serialization direction has the reverse issue: `*radius as i32` wraps if radius > `i32::MAX`.

Affected sites:
- `flatbuf.rs:311` — `params[ndim] as u32` (Disk radius)
- `flatbuf.rs:333` — `params[ndim] as u32` (Neighbours depth)
- `flatbuf.rs:367` — `params[ndim] as u32` (AgentDisk radius)
- `flatbuf.rs:376` — `params[ndim] as u32` (AgentRect half_extent)
- `flatbuf.rs:149,159,171,173` — serialization direction (`*val as i32`)

**Note**: Related to closed #22 (ffi-obs-negative-to-unsigned-cast) which fixed the FFI layer. This is the same class of bug in the obs crate's own serialization layer.

## Expected Behavior

Return `ObsError::InvalidObsSpec` on out-of-range values.

## Actual Behavior

Silent data corruption — negative values become very large unsigned values.

## Additional Context

**Source:** murk-obs audit, Finding 1
**File:** `crates/murk-obs/src/flatbuf.rs:311,333,367,376,149,159,171,173`
**Suggested fix:** Use `u32::try_from()` / `i32::try_from()` and return error on out-of-range values.
