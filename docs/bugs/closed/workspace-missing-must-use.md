# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [x] murk-core
- [x] murk-arena
- [x] murk-propagator

## Engine Mode

- [x] Both / Unknown

## Summary

Multiple types and methods across the workspace are missing `#[must_use]` annotations. Discarding these values is almost certainly a bug:

**murk-core:**
- `FieldSet::union()`, `intersection()`, `difference()`, `is_subset()`, `is_empty()`, `len()` -- set operations whose results are the entire point of calling them
- `SpaceInstanceId::next()` -- side effect increments global counter; discarding wastes a unique ID

**murk-arena:**
- `TickGuard` (from `begin_tick()`) -- discarding means the tick does nothing
- `FieldHandle`, `ArenaConfig` -- constructed values that should be stored

**murk-propagator:**
- `ReadResolutionPlan` -- needed to configure per-propagator field routing

## Expected Behavior

`#[must_use]` on all listed types and methods.

## Actual Behavior

Results silently discardable.

## Additional Context

**Source:** murk-core audit A-1/A-2, murk-arena audit API-4, murk-propagator audit Finding 13
