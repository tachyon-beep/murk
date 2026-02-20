# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [x] murk-core
- [x] murk-engine
- [x] murk-space
- [x] murk-propagator

## Engine Mode

- [x] Both / Unknown

## Summary

Error types across the workspace are missing `PartialEq`/`Eq` derives despite all their fields supporting it:

- **murk-core:** `StepError`, `PropagatorError`, `ObsError` -- all contain only `String`/enum variants that support `PartialEq`
- **murk-engine:** `ConfigError`, `TickError`, `SubmitError`, `BatchError` -- forces tests to use `matches!()` instead of `assert_eq!()`
- **murk-space:** `SpaceError`
- **murk-propagator:** `PipelineError` (contains `f64`, but can implement custom `PartialEq`)

This makes test ergonomics significantly worse.

## Expected Behavior

Add `PartialEq, Eq` derives to all error types where possible.

## Actual Behavior

Tests forced to use `matches!()` pattern matching for error assertions.

## Additional Context

**Source:** murk-core audit E-1/E-3, murk-engine audit F-18, murk-space audit A-1, murk-propagator audit Finding 12
