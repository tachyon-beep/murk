# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-python

## Engine Mode

- [x] Lockstep

## Summary

`BatchedWorld::reset_all()` at `batched.rs:260-269` does not validate that `seeds.len() == num_worlds`. If the user passes a seeds vector with the wrong length, the FFI function could read out of bounds (if fewer seeds than worlds) or ignore extras (if more). Whether the FFI function validates this internally is uncertain from the Python bindings layer.

## Expected Behavior

Validate `seeds.len() == self.cached_num_worlds` before calling FFI, raising `ValueError` on mismatch.

## Actual Behavior

No validation; wrong-length seeds passed directly to FFI.

## Additional Context

**Source:** murk-python audit, F-4
**File:** `crates/murk-python/src/batched.rs:260-269`
