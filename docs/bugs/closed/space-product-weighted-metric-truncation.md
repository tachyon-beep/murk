# Bug Report — FIXED

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low
**Fixed:** 2026-02-18

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [x] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [ ] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`ProductSpace::metric_distance` silently produces incorrect distances when
`ProductMetric::Weighted` is used with a weight vector whose length does not
match the number of component spaces. Rust's `Iterator::zip` truncates to the
shorter iterator, so trailing component distances or trailing weights are
silently ignored instead of triggering an error.

## Steps to Reproduce

```rust
use murk_space::{ProductSpace, ProductMetric, Line1D, EdgeBehavior, Space};
use smallvec::smallvec;

let a = Line1D::new(10, EdgeBehavior::Absorb).unwrap();
let b = Line1D::new(10, EdgeBehavior::Absorb).unwrap();
let c = Line1D::new(10, EdgeBehavior::Absorb).unwrap();
let ps = ProductSpace::new(vec![Box::new(a), Box::new(b), Box::new(c)]).unwrap();

let p1 = smallvec![0, 0, 0];
let p2 = smallvec![5, 5, 5];

// Only 2 weights for 3 components -- third component distance silently dropped
let d = ps.metric_distance(&p1, &p2, &ProductMetric::Weighted(vec![1.0, 1.0]));
// Returns 10.0 instead of the expected 15.0
```

## Expected Behavior

Either:
- Return an error (if the API is made fallible) when `weights.len() != components.len()`
- Panic with a clear message at the mismatch point
- Reject mismatched weights at `ProductMetric::Weighted` construction time

## Actual Behavior

`zip` silently truncates to the shorter of `per_comp` (3 elements) and `weights`
(2 elements), so the third component's distance contribution is dropped. The
caller receives a numerically incorrect distance with no warning.

## Reproduction Rate

100% deterministic for any weight/component arity mismatch.

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
use murk_space::{ProductSpace, ProductMetric, Line1D, EdgeBehavior, Space};
use smallvec::smallvec;

let a = Line1D::new(10, EdgeBehavior::Absorb).unwrap();
let b = Line1D::new(10, EdgeBehavior::Absorb).unwrap();
let c = Line1D::new(10, EdgeBehavior::Absorb).unwrap();
let ps = ProductSpace::new(vec![Box::new(a), Box::new(b), Box::new(c)]).unwrap();

let p1 = smallvec![0, 0, 0];
let p2 = smallvec![5, 5, 5];

// Bug: returns 10.0 (only first 2 components), should be 15.0 or error
let d = ps.metric_distance(&p1, &p2, &ProductMetric::Weighted(vec![1.0, 1.0]));
assert_eq!(d, 15.0); // FAILS
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-space/src/product.rs.md
**Verified lines:** product.rs:130-143
**Root cause:** `Iterator::zip` truncates to the shorter iterator without error on arity mismatch between `per_comp` (component distances) and `weights` vector.
**Suggested fix:** Add `assert_eq!(weights.len(), self.components.len(), "Weighted metric requires exactly one weight per component")` before the zip, or make the method return `Result<f64, SpaceError>`.

This falls under CR-1 (ProductSpace semantics) from the architectural design review.

## Resolution

Added `assert_eq!(weights.len(), self.components.len())` before the zip in `metric_distance`. Wrong weight count is a programming error, so `assert!` (panic) is appropriate — consistent with Rust convention and the existing codebase validation patterns.

Tests added: `weighted_metric_too_few_weights_panics`, `weighted_metric_too_many_weights_panics`, `weighted_metric_exact_match_succeeds`.
