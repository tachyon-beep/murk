# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-python

## Engine Mode

- [x] Both / Unknown

## Summary

The `.pyi` type stub file (`python/murk/_murk.pyi`) is completely missing type stubs for all 9 library propagator classes:
- `ScalarDiffusion`, `GradientCompute`, `IdentityCopy`, `FlowField`
- `AgentEmission`, `ResourceField`, `MorphologicalOp`, `WavePropagation`, `NoiseInjection`

These are registered in `lib.rs:51-59` and exported in `__init__.py:9-38`, but type checkers (mypy, pyright) and IDEs cannot see their signatures. Users get no autocompletion, no type checking, and no inline documentation for the primary propagator API.

## Expected Behavior

All 9 library propagator classes should have complete type stubs with `__init__`, `register`, and `__repr__` method signatures.

## Actual Behavior

Type stubs are absent. IDEs show no completions for library propagators.

## Additional Context

**Source:** murk-python audit, F-32 + F-35
**File:** `crates/murk-python/python/murk/_murk.pyi`
