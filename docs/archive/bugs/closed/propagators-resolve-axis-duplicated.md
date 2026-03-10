# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-propagators

## Engine Mode

- [x] Both / Unknown

## Summary

The `resolve_axis` and `neighbours_flat` helper functions are copy-pasted identically across 5 files:
- `scalar_diffusion.rs:88-97`
- `diffusion.rs:33-42`
- `gradient_compute.rs:80-89`
- `flow_field.rs:82-91`
- `wave_propagation.rs:64-73`

This is a maintenance hazard — a bug fix in one copy could be missed in others. The functions are identical and have no file-specific variations.

## Expected Behavior

Shared helpers extracted to a single module (e.g., `src/grid_helpers.rs`).

## Actual Behavior

~100 lines of duplicated code across 5 files.

## Additional Context

**Source:** murk-propagators audit, H-3 + M-1
**Suggested fix:** Create `crates/murk-propagators/src/grid_helpers.rs` with `pub(crate) fn resolve_axis(...)` and `pub(crate) fn neighbours_flat(...)`, then import in all 5 files.
