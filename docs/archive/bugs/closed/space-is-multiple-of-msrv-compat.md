# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-space
- [x] murk-obs

## Engine Mode

- [x] Both / Unknown

## Summary

`u32::is_multiple_of(2)` is used in `fcc12.rs:103` (murk-space) and `flatbuf.rs:315` (murk-obs). This method was stabilized in Rust 1.88, but the workspace declares MSRV 1.87. If the project is building on nightly or 1.88+, this compiles fine. But on stable 1.87, it will fail.

## Steps to Reproduce

```
rustup override set 1.87.0
cargo build
# error[E0599]: no method named `is_multiple_of` found for type `u32`
```

## Expected Behavior

Code compiles on the declared MSRV (1.87).

## Actual Behavior

Compilation fails on stable 1.87.

## Additional Context

**Source:** murk-space audit I-2, murk-obs audit Finding 15
**Files:** `crates/murk-space/src/fcc12.rs:103`, `crates/murk-obs/src/flatbuf.rs:315`
**Suggested fix:** Replace `!w.is_multiple_of(2)` with `w % 2 != 0` (or `w & 1 != 0`). Trivial one-line fix.
