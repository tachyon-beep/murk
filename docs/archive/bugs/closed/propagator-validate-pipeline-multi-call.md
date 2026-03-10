# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent
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

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`validate_pipeline` calls `writes()`, `reads()`, and `reads_previous()` multiple times per propagator across its validation and plan-building passes, which is wasteful and violates the documented "called once at startup" contract. If a `Propagator` implementation were stateful (e.g., via interior mutability), the repeated calls could return inconsistent declarations, allowing an invalid pipeline to pass validation.

## Steps to Reproduce

1. Implement a `Propagator` whose `writes()` returns different values on successive calls (e.g., using `AtomicUsize` to vary return by call count).
2. Call `validate_pipeline` with a `defined_fields` set that includes only the first call's field.
3. Observe that validation passes (conflict check uses first call), but the `ReadResolutionPlan` contains the third call's field (which may be undefined).

## Expected Behavior

`reads()`, `reads_previous()`, and `writes()` should each be evaluated exactly once per propagator at the start of `validate_pipeline`. All subsequent validation passes (conflict detection, field existence checks, dt validation, plan construction) should operate on that single immutable snapshot. This matches the trait documentation at `crates/murk-propagator/src/propagator.rs:32` ("called once at startup") and `crates/murk-propagator/src/propagator.rs:93` ("Called once at pipeline construction, not per-tick").

## Actual Behavior

`validate_pipeline` calls declaration methods multiple times per propagator:

- `writes()` called at:
  - `crates/murk-propagator/src/pipeline.rs:238` (write-conflict detection pass)
  - `crates/murk-propagator/src/pipeline.rs:272` (field existence validation pass)
  - `crates/murk-propagator/src/pipeline.rs:334` (plan construction pass)
- `reads()` called at:
  - `crates/murk-propagator/src/pipeline.rs:256` (field existence validation pass)
  - `crates/murk-propagator/src/pipeline.rs:319` (plan construction pass)
- `reads_previous()` called at:
  - `crates/murk-propagator/src/pipeline.rs:264` (field existence validation pass)

This is at minimum wasteful (3x `writes()` calls, 2x `reads()` calls per propagator), and at worst a correctness hazard if any implementation uses interior mutability or is otherwise non-idempotent.

## Reproduction Rate

Always (structural code issue).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.9)

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
use std::sync::atomic::{AtomicUsize, Ordering};
use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::{Propagator, StepContext, WriteMode};

struct FlakyWrites {
    calls: AtomicUsize,
}

impl Propagator for FlakyWrites {
    fn name(&self) -> &str { "FlakyWrites" }

    fn reads(&self) -> FieldSet { FieldSet::empty() }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        let n = self.calls.fetch_add(1, Ordering::Relaxed);
        // First two calls (conflict check + field validation): FieldId(0)
        // Third call (plan construction): FieldId(1) — possibly undefined
        let f = if n < 2 { FieldId(0) } else { FieldId(1) };
        vec![(f, WriteMode::Full)]
    }

    fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        Ok(())
    }
}
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-propagator/src/pipeline.rs.md`

**Affected lines:**
- Trait contract documentation: `crates/murk-propagator/src/propagator.rs:32`, `crates/murk-propagator/src/propagator.rs:93`
- Multi-call sites in pipeline.rs: lines 238, 256, 264, 272, 319, 334

**Root cause:** `validate_pipeline` does not snapshot each propagator's declarations before running its passes. Instead, it re-evaluates `reads()`, `writes()`, and `reads_previous()` in each pass that needs them (conflict detection, field validation, plan construction).

**Suggested fix:** At the top of `validate_pipeline`, precompute per-propagator metadata once into a local struct:

```rust
struct PropMeta {
    name: String,
    reads: FieldSet,
    reads_previous: FieldSet,
    writes: Vec<(FieldId, WriteMode)>,
}

let metas: Vec<PropMeta> = propagators.iter().map(|p| PropMeta {
    name: p.name().to_string(),
    reads: p.reads(),
    reads_previous: p.reads_previous(),
    writes: p.writes(),
}).collect();
```

Then run all validation and plan-building passes against `metas` instead of calling trait methods again. This honours the "called once" contract, eliminates redundant work, and prevents any future non-idempotent implementation from silently corrupting the pipeline.
