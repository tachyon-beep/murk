# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-ffi

## Engine Mode

- [x] Both / Unknown

## Summary

`murk_lockstep_create()` at `world.rs:40-44` documents "On failure, the config is still consumed (destroyed)." However, when `world_out` is null, the function returns `InvalidArgument` **without** removing the config from the handle table. The test at line 573-582 even acknowledges this: "Config was not consumed because we returned early. Clean it up manually."

This inconsistency means C callers who follow the documented "config is always consumed" contract will leak the config handle on null `world_out`.

## Expected Behavior

Either consume the config before the null check (consistent with docs) or fix the docs to say "config is NOT consumed if world_out is null."

## Actual Behavior

Config leaked; documented ownership contract violated.

## Additional Context

**Source:** murk-ffi audit, F-04
**File:** `crates/murk-ffi/src/world.rs:40-44`
