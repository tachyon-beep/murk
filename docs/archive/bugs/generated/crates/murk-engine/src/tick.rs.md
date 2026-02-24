# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [x] murk-engine
- [ ] murk-arena
- [ ] murk-space
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

`SetField` commands that cannot be applied (e.g., out-of-bounds coord) are still reported as successfully applied in receipts.

## Steps to Reproduce

1. Create a world with a valid `SetField` field and a no-op propagator.
2. Submit `CommandPayload::SetField` with an out-of-bounds coordinate.
3. Execute one tick and inspect both the receipt and snapshot.

## Expected Behavior

A command that is not actually applied should not get `applied_tick_id`, and should be marked rejected (or otherwise clearly not applied).

## Actual Behavior

The command write is skipped, but receipt remains `accepted=true` and gets `applied_tick_id=Some(next_tick)`.

## Reproduction Rate

Always (for out-of-bounds / non-applied `SetField` cases)

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

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_engine::{BackoffConfig, LockstepWorld, WorldConfig};
use murk_core::{
    BoundaryBehavior, Command, CommandPayload, FieldDef, FieldId, FieldMutability, FieldSet,
    FieldType, PropagatorError, TickId,
};
use murk_core::traits::SnapshotAccess;
use murk_propagator::{Propagator, StepContext};
use murk_space::{EdgeBehavior, Line1D};

struct Noop;
impl Propagator for Noop {
    fn name(&self) -> &str { "noop" }
    fn reads(&self) -> FieldSet { FieldSet::empty() }
    fn writes(&self) -> Vec<(FieldId, murk_propagator::WriteMode)> { vec![] }
    fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> { Ok(()) }
}

fn main() {
    let cfg = WorldConfig {
        space: Box::new(Line1D::new(4, EdgeBehavior::Absorb).unwrap()),
        fields: vec![FieldDef {
            name: "f".into(),
            field_type: FieldType::Scalar,
            mutability: FieldMutability::PerTick,
            units: None,
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        }],
        propagators: vec![Box::new(Noop)],
        dt: 0.1,
        seed: 1,
        ring_buffer_size: 8,
        max_ingress_queue: 16,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    let mut world = LockstepWorld::new(cfg).unwrap();

    let cmd = Command {
        payload: CommandPayload::SetField {
            coord: vec![99i32].into(), // out of bounds for len=4
            field_id: FieldId(0),
            value: 123.0,
        },
        expires_after_tick: TickId(100),
        source_id: None,
        source_seq: None,
        priority_class: 1,
        arrival_seq: 0,
    };

    let step = world.step_sync(vec![cmd]).unwrap();
    assert!(step.receipts[0].accepted);
    assert_eq!(step.receipts[0].applied_tick_id, Some(TickId(1))); // reported applied
    assert!(step.snapshot.read(FieldId(0)).unwrap().iter().all(|&v| v == 0.0)); // no write happened
}
```

## Additional Context

Evidence in `/home/john/murk/crates/murk-engine/src/tick.rs`:
1. Receipts are pre-marked `accepted: true` at `:254`-`:259`.
2. `SetField` silently no-ops when rank/buffer checks fail at `:270`-`:275` (no receipt mutation).
3. Finalization sets `applied_tick_id` for all `accepted` receipts at `:381`-`:384`.
4. Inline comment says this should be “only for actually executed commands” at `:380`, but non-applied `SetField` still qualifies under current logic.

Suggested fix: explicitly mark non-applied `SetField` receipts as rejected (or at minimum avoid setting `applied_tick_id`) when coordinate/field resolution fails.

---

# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [x] murk-engine
- [ ] murk-arena
- [ ] murk-space
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

`TickEngine::new` performs unchecked `u32` multiplication for static field lengths, which can panic in debug builds instead of returning a normal configuration error.

## Steps to Reproduce

1. Build a config with a `Static` vector field where `cell_count * dims` overflows `u32`.
2. Call `TickEngine::new(config)` in a debug build.
3. Observe arithmetic overflow panic.

## Expected Behavior

Construction should fail gracefully with an error return (`Err(...)`), not panic.

## Actual Behavior

`cell_count * components` overflows in `tick.rs` and panics in debug (`attempt to multiply with overflow`) before normal error handling.

## Reproduction Rate

Always (debug build, overflowing input)

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

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_engine::{BackoffConfig, TickEngine, WorldConfig};
use murk_core::{
    BoundaryBehavior, FieldDef, FieldId, FieldMutability, FieldSet, FieldType, PropagatorError,
};
use murk_propagator::{Propagator, StepContext};
use murk_space::{EdgeBehavior, Line1D};

struct Noop;
impl Propagator for Noop {
    fn name(&self) -> &str { "noop" }
    fn reads(&self) -> FieldSet { FieldSet::empty() }
    fn writes(&self) -> Vec<(FieldId, murk_propagator::WriteMode)> { vec![] }
    fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> { Ok(()) }
}

fn main() {
    let cfg = WorldConfig {
        space: Box::new(Line1D::new(3, EdgeBehavior::Absorb).unwrap()),
        fields: vec![FieldDef {
            name: "static_vec".into(),
            field_type: FieldType::Vector { dims: 2_863_311_531 }, // 3 * dims overflows u32
            mutability: FieldMutability::Static,
            units: None,
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        }],
        propagators: vec![Box::new(Noop)],
        dt: 0.1,
        seed: 1,
        ring_buffer_size: 8,
        max_ingress_queue: 16,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    // Debug build: panic in TickEngine::new at tick.rs multiplication.
    let _ = TickEngine::new(cfg);
}
```

## Additional Context

Evidence in `/home/john/murk/crates/murk-engine/src/tick.rs`:
1. `cell_count` is only checked to fit `u32` at `:138`-`:139`.
2. Static length uses unchecked multiply at `:146`: `cell_count * d.field_type.components()`.

Root cause: missing `checked_mul` on static field length calculation.  
Suggested fix: use `checked_mul` and map overflow into a returned config error (instead of panicking/wrapping).