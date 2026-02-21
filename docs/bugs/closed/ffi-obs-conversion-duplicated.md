# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-ffi

## Engine Mode

- [x] Both / Unknown

## Summary

The `MurkObsEntry`-to-`ObsEntry` conversion logic is copy-pasted between `obs.rs:101-173` (`murk_obsplan_compile`) and `batched.rs:29-101` (`convert_obs_entry`). If a new region type, transform type, or pool kernel is added, it must be updated in two places -- a divergence bug waiting to happen.

## Expected Behavior

Single shared `convert_obs_entry` function called from both locations.

## Actual Behavior

Two functionally identical but structurally different copies.

## Additional Context

**Source:** murk-ffi audit, F-07
**Files:** `crates/murk-ffi/src/obs.rs:101-173`, `crates/murk-ffi/src/batched.rs:29-101`
