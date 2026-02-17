# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [x] murk-engine
- [x] murk-arena
- [ ] murk-space
- [x] murk-propagator
- [x] murk-propagators
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

`WriteMode::Incremental` is documented as "Buffer seeded from the previous generation via memcpy. Propagator modifies only the cells it needs to update." However, the runtime never inspects write mode metadata. PerTick field buffers are always freshly allocated and zero-initialized, meaning incremental propagators receive zeroed buffers instead of previous-generation data. This causes silent data loss for any propagator relying on state persistence between ticks.

`AgentMovementPropagator` declares `WriteMode::Incremental` for `AGENT_PRESENCE` and depends on previous-tick positions persisting, but the arena gives it a zeroed buffer each tick.

## Steps to Reproduce

1. Register a propagator that declares `WriteMode::Incremental` for a field.
2. On tick 1, write non-zero values to some cells but not all.
3. On tick 2, observe that the write buffer is all zeros, not seeded from tick 1.

## Expected Behavior

For `WriteMode::Incremental`, the write buffer should be pre-filled with a memcpy of the previous generation's data before `step()` is called. The propagator should only need to update the cells that changed.

## Actual Behavior

The write buffer is always zero-initialized regardless of `WriteMode`. The `_mode` variable is discarded in `pipeline.rs` line 278 (`for (field_id, _mode) in prop.writes()`). The `PingPongArena::begin_tick()` method calls `Segment::alloc()` which zero-fills via `slice.fill(0.0)` at segment.rs:48.

The `AgentMovementPropagator` unit tests mask this bug by manually pre-filling the `MockFieldWriter` buffer in `setup_presence()` (agent_movement.rs:236-257), simulating the incremental seeding that the real engine does not perform.

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
use murk_propagator::propagator::{Propagator, WriteMode};
use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;

struct IncrementalProp;
impl Propagator for IncrementalProp {
    fn name(&self) -> &str { "incremental" }
    fn reads(&self) -> FieldSet { FieldSet::empty() }
    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(FieldId(0), WriteMode::Incremental)]
    }
    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let buf = ctx.writes().write(FieldId(0)).unwrap();
        // On tick 2+, expect previous values to be present.
        // BUG: buf is always zeroed, previous-gen data is lost.
        let sum: f32 = buf.iter().sum();
        assert!(sum > 0.0, "expected seeded data, got zeroes");
        Ok(())
    }
}
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-propagator/src/propagator.rs.md
**Verified lines:** propagator.rs:18-23 (Incremental doc), pipeline.rs:278 (_mode discarded), segment.rs:47-48 (zero-fill), pingpong.rs:204-205 (alloc path), agent_movement.rs:93 (declares Incremental), agent_movement.rs:236-257 (test workaround)
**Root cause:** WriteMode metadata is declared in the Propagator API but never threaded into the arena's tick-time write buffer initialization. PerTick buffers always follow zero-init semantics.
**Suggested fix:**
1. Carry WriteMode metadata from `prop.writes()` into `begin_tick()`.
2. For `WriteMode::Incremental` fields, memcpy the published buffer into the staging buffer before returning the `TickGuard`.
3. For `WriteMode::Full` fields, keep zero-init (current behavior).
4. Add a regression test: run two ticks with an incremental propagator and verify state persists.
