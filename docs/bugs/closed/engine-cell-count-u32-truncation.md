# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [x] Both / Unknown

## Summary

`TickEngine::new()` at `tick.rs:119` performs `let cell_count = config.space.cell_count() as u32;` which silently truncates if `cell_count > u32::MAX`. A 65536x65536 grid produces 4,294,967,296 cells, overflowing to 0 and creating a zero-sized arena. The same pattern exists in `FieldId(i as u32)` at `config.rs:273`.

## Expected Behavior

Use `u32::try_from(config.space.cell_count()).map_err(...)` and return a `ConfigError` variant on overflow.

## Actual Behavior

Silent truncation via `as u32`.

## Additional Context

**Source:** murk-engine audit, F-02/F-03
**Files:** `crates/murk-engine/src/tick.rs:119`, `crates/murk-engine/src/config.rs:273`
