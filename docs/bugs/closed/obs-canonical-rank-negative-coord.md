# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-obs

## Engine Mode

- [x] Both / Unknown

## Summary

`canonical_rank` at `geometry.rs:95-101` computes `rank += *c as usize * stride`. If `*c` is negative (valid since `coord` is `&[i32]`), `*c as usize` wraps to a very large number. The function is called from the fast path of agent observation execution (lines 963, 1014), where the result indexes into field data. A wrapped negative coordinate reads from a random field offset rather than producing an error.

Callers do bounds-check via `.get()`, but a wrapped coordinate could land within bounds and silently read wrong data.

## Expected Behavior

`debug_assert!(coord.iter().all(|c| *c >= 0))` or `usize::try_from(*c)` with error handling.

## Actual Behavior

Negative coordinates silently wrap via `as usize`.

## Additional Context

**Source:** murk-obs audit, Finding 4
**File:** `crates/murk-obs/src/geometry.rs:95-101`
