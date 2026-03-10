# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-propagators

## Engine Mode

- [x] Lockstep

## Summary

`ScalarDiffusion` builder validates `self.coefficient < 0.0` and `self.decay < 0.0`, but `f64::NAN < 0.0` is `false` and `f64::INFINITY >= 0.0` is `true`, so both NaN and Infinity pass validation.

- NaN coefficient → NaN output on every cell, every tick
- Infinite coefficient → immediate blow-up
- NaN decay → NaN everywhere
- Infinite decay → `exp(-INF * dt) = 0.0`, zeroing all values

The P4 propagators correctly use the `!(x >= 0.0)` pattern that rejects NaN. The ScalarDiffusion builder does not.

## Steps to Reproduce

```rust
let prop = ScalarDiffusion::builder()
    .output(FieldId(0))
    .coefficient(f64::NAN)
    .build(); // Returns Ok — should return Err
```

## Expected Behavior

`build()` returns `Err` for NaN or infinite coefficient/decay values.

## Actual Behavior

`build()` returns `Ok`, and the propagator produces NaN on every tick.

## Additional Context

**Source:** murk-propagators audit, H-4 + H-5
**File:** `crates/murk-propagators/src/scalar_diffusion.rs:430-438`
**Suggested fix:** Replace `self.coefficient < 0.0` with `!(self.coefficient >= 0.0) || !self.coefficient.is_finite()`.
