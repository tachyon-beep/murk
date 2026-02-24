# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

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

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`validate_pipeline` calls `Propagator::reads()`/`writes()` multiple times, violating the startup contract and allowing inconsistent declarations to pass validation and produce an invalid `ReadResolutionPlan`.

## Steps to Reproduce

1. Implement a `Propagator` whose `writes()` changes return value by call count (e.g., first two calls return `FieldId(0)`, third call returns `FieldId(1)`).
2. Call `validate_pipeline` with `defined_fields = {FieldId(0)}`.
3. Observe `validate_pipeline` returns `Ok`, but the plan contains write metadata for `FieldId(1)` (undefined).

## Expected Behavior

`reads()` and `writes()` should be evaluated once per propagator at startup and reused consistently for conflict checks, field validation, and plan construction.

## Actual Behavior

`validate_pipeline` invokes declarations repeatedly:
- `writes()` at `/home/john/murk/crates/murk-propagator/src/pipeline.rs:238`, `/home/john/murk/crates/murk-propagator/src/pipeline.rs:272`, `/home/john/murk/crates/murk-propagator/src/pipeline.rs:334`
- `reads()` at `/home/john/murk/crates/murk-propagator/src/pipeline.rs:256`, `/home/john/murk/crates/murk-propagator/src/pipeline.rs:319`

This allows non-idempotent propagators to pass checks with one declaration set and build a plan from another.

## Reproduction Rate

Always

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```text
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::{Propagator, StepContext, WriteMode, validate_pipeline};

struct FlakyWrites {
    calls: Arc<AtomicUsize>,
}

impl Propagator for FlakyWrites {
    fn name(&self) -> &str { "FlakyWrites" }

    fn reads(&self) -> FieldSet { FieldSet::empty() }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        let n = self.calls.fetch_add(1, Ordering::Relaxed);
        let f = if n < 2 { FieldId(0) } else { FieldId(1) };
        vec![(f, WriteMode::Full)]
    }

    fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        Ok(())
    }
}

fn repro() {
    let calls = Arc::new(AtomicUsize::new(0));
    let props: Vec<Box<dyn Propagator>> =
        vec![Box::new(FlakyWrites { calls: calls.clone() })];

    let defined_fields: FieldSet = [FieldId(0)].into_iter().collect();
    let space = murk_space::Square4::new(4, 4, murk_space::EdgeBehavior::Wrap).unwrap();

    let plan = validate_pipeline(&props, &defined_fields, 0.1, &space).unwrap();

    assert_eq!(calls.load(Ordering::Relaxed), 3); // called three times
    assert_eq!(plan.write_mode(0, FieldId(1)), Some(WriteMode::Full)); // undefined field in plan
}
```

## Additional Context

Evidence of contract mismatch:
- `/home/john/murk/crates/murk-propagator/src/propagator.rs:32` documents `reads()` and `writes()` are called once at startup.
- `/home/john/murk/crates/murk-propagator/src/propagator.rs:93` reiterates `writes()` is called once at pipeline construction.

Root cause:
- `validate_pipeline` does not snapshot declarations; it recomputes them in separate passes.

Suggested fix:
- Precompute per-propagator metadata once (name, reads, reads_previous, writes) at the start of `validate_pipeline`, then run all validation and plan-building passes from that immutable snapshot.