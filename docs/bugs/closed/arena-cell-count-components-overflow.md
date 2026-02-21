# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-arena

## Engine Mode

- [x] Both / Unknown

## Summary

`descriptor.rs:61` computes `let total_len = cell_count * components;` where both are `u32`. For large grids with multi-component fields (e.g., `cell_count = 1_000_000_000` and `Vector { dims: 7 }` = 7 components), the multiplication yields `7_000_000_000` which overflows `u32`, silently wrapping in release mode.

## Expected Behavior

Use `cell_count.checked_mul(components)` and propagate the error.

## Actual Behavior

Silent `u32` overflow in release mode; panic in debug mode.

## Additional Context

**Source:** murk-arena audit, BUG-3
**File:** `crates/murk-arena/src/descriptor.rs:61`
