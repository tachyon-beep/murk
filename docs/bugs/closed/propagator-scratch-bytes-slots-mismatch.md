# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-propagator

## Engine Mode

- [x] Both / Unknown

## Summary

`Propagator::scratch_bytes()` returns bytes (doc says "Scratch memory required in bytes"), but `ScratchRegion::new()` takes capacity in **f32 slots**, not bytes. There is `ScratchRegion::with_byte_capacity()` that converts bytes to slots, but nothing in the crate itself ensures the engine bridges this gap correctly.

If a caller mistakenly passes `scratch_bytes()` directly to `ScratchRegion::new()`, they get 4x more memory than intended (or 1/4 if the conversion goes the other way).

## Expected Behavior

Either rename to make units explicit (e.g., `scratch_slots()` and `ScratchRegion::new_with_slot_count()`) or add prominent doc comments warning about the unit mismatch.

## Actual Behavior

Ambiguous units across the API boundary.

## Additional Context

**Source:** murk-propagator audit, Finding 6
**Files:** `crates/murk-propagator/src/propagator.rs:104-109`, `crates/murk-propagator/src/scratch.rs:18`
