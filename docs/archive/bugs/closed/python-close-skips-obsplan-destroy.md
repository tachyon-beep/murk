# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-python

## Engine Mode

- [x] Both / Unknown

## Summary

`MurkEnv.close()` at `env.py:155-158` destroys the `World` but does not explicitly destroy the `ObsPlan`. The `ObsPlan` holds an FFI handle and should be cleaned up first, since it may reference the `World`. While Python's GC will eventually call `Drop`, if the `ObsPlan` tries to access the already-destroyed `World` during cleanup, behavior is undefined.

Additionally, `BatchedVecEnv.close()` at `batched_vec_env.py:158-160` does not prevent double-close or set the engine to `None` after destroy.

## Expected Behavior

`close()` should destroy `ObsPlan` before `World`, and guard against double-close.

## Actual Behavior

`ObsPlan` cleanup relies on GC; order-dependent resource release not enforced.

## Additional Context

**Source:** murk-python audit, F-15/F-16
**Files:** `crates/murk-python/python/murk/env.py:155-158`, `crates/murk-python/python/murk/batched_vec_env.py:158-160`
