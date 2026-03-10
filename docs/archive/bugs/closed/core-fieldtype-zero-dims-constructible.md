# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-core

## Engine Mode

- [x] Both / Unknown

## Summary

Two related validation gaps in field type construction:

1. `FieldType::Vector { dims: 0 }` is constructible and produces a field with zero f32 components via `components()`. This is semantically nonsensical and has already caused downstream bugs (BUG-008 regression test in murk-engine overlay.rs). Similarly, `FieldType::Categorical { n_values: 0 }` produces a meaningless zero-category field.

2. `FieldDef.bounds` is `Option<(f32, f32)>` with no validation that `min <= max`. `bounds: Some((100.0, 0.0))` and NaN/infinity bounds are accepted. No validation occurs at `FieldDef` construction or in `WorldConfig::validate()`.

## Expected Behavior

Validated constructors: `FieldType::vector(dims: u32) -> Result<Self, ...>` rejecting `dims == 0`, and `FieldDef::validate()` checking bounds ordering and NaN/infinity.

## Actual Behavior

All public fields freely constructible with degenerate values.

## Additional Context

**Source:** murk-core audit, B-2/B-3
**Files:** `crates/murk-core/src/field.rs:17-41` (FieldType), `crates/murk-core/src/field.rs:146-147` (FieldDef.bounds)
