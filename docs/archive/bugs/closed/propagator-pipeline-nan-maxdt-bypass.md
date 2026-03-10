# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [x] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [ ] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [x] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`validate_pipeline()` does not validate per-propagator `max_dt()` return values for finiteness. If a propagator returns `Some(NaN)` from `max_dt()`, the NaN silently fails all IEEE-754 comparisons, leaving `min_max_dt` at its initial value of `f64::INFINITY`. This means the propagator's stability constraint is effectively ignored, and the pipeline accepts any finite `dt` value.

While `dt` itself is validated for finiteness/positivity (line 175), the per-propagator `max_dt` values are not.

## Steps to Reproduce

1. Implement a propagator whose `max_dt()` returns `Some(f64::NAN)`.
2. Call `validate_pipeline()` with `dt = 1000.0`.
3. Observe that validation succeeds (returns `Ok(...)`) instead of rejecting the invalid `max_dt`.

## Expected Behavior

`validate_pipeline()` should reject any propagator returning a non-finite or non-positive `max_dt()` value with a dedicated error variant (e.g., `InvalidMaxDt`).

## Actual Behavior

NaN values are silently treated as "no constraint" because the comparison `max < min_max_dt` (line 240) evaluates to `false` when `max` is NaN (IEEE-754 semantics). The final check `if dt > min_max_dt` (line 246) also sees `INFINITY` and passes.

## Reproduction Rate

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_propagator::pipeline::validate_pipeline;
use murk_propagator::propagator::{Propagator, WriteMode};
use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;

struct NanMaxDtProp;
impl Propagator for NanMaxDtProp {
    fn name(&self) -> &str { "nan_max_dt" }
    fn reads(&self) -> FieldSet { FieldSet::empty() }
    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(FieldId(0), WriteMode::Full)]
    }
    fn max_dt(&self) -> Option<f64> {
        Some(f64::NAN) // Invalid constraint
    }
    fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        Ok(())
    }
}

let props: Vec<Box<dyn Propagator>> = vec![Box::new(NanMaxDtProp)];
let fields = [FieldId(0)].into_iter().collect();
// BUG: This succeeds instead of rejecting the NaN max_dt
let result = validate_pipeline(&props, &fields, 1000.0);
assert!(result.is_ok()); // Should be Err
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-propagator/src/pipeline.rs.md
**Verified lines:** pipeline.rs:236 (min_max_dt starts at INFINITY), pipeline.rs:239-243 (NaN comparison always false), pipeline.rs:246 (final dt check against INFINITY passes)
**Root cause:** The validator checks `dt` for finiteness but does not validate per-propagator `max_dt` values before using them in the minimum reduction. NaN propagates silently through IEEE-754 comparison semantics.
**Suggested fix:** Before the `if max < min_max_dt` comparison, validate that `max` is finite and strictly positive. Return a dedicated `InvalidMaxDt { propagator, value }` error variant when it is not.
