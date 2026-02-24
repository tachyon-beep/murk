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

- [x] Lockstep
- [ ] RealtimeAsync
- [ ] Both / Unknown

## Summary

`BatchedEngine::new` accepts an `ObsSpec` field that is missing in all worlds, causing `step_and_observe` to step worlds and then fail in observation, violating error atomicity.

## Steps to Reproduce

1. Build a `BatchedEngine` with 2 worlds that only define `FieldId(0)`.
2. Pass an `ObsSpec` that references `FieldId(1)` (missing in every world).
3. Call `step_and_observe` and inspect ticks after the returned error.

## Expected Behavior

`BatchedEngine::new` should reject the invalid `ObsSpec` up front (or `step_and_observe` should fail before stepping), so an observation error does not mutate world ticks.

## Actual Behavior

`BatchedEngine::new` succeeds because schema validation only checks equality against world 0 (`None == None` passes), then `step_and_observe` performs `step_all` first and fails during observation with `BatchError::Observe`, after ticks have already advanced.

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
// Sketch using the same construction pattern as batched.rs tests.
let spec = ObsSpec {
    entries: vec![ObsEntry {
        field_id: FieldId(1), // missing in all worlds
        region: ObsRegion::Fixed(RegionSpec::All),
        pool: None,
        transform: ObsTransform::Identity,
        dtype: ObsDtype::F32,
    }],
};

let configs = vec![
    make_config_with_only_field0(1),
    make_config_with_only_field0(2),
];

let mut engine = BatchedEngine::new(configs, Some(&spec)).unwrap(); // currently succeeds
let mut out = vec![0.0; engine.num_worlds() * engine.obs_output_len()];
let mut mask = vec![0u8; engine.num_worlds() * engine.obs_mask_len()];

let err = engine.step_and_observe(&[vec![], vec![]], &mut out, &mut mask).unwrap_err();
// err is BatchError::Observe(... "field FieldId(1) not in snapshot"...)
// but ticks have advanced:
assert_eq!(engine.world_tick(0), Some(TickId(1)));
assert_eq!(engine.world_tick(1), Some(TickId(1)));
```

## Additional Context

Evidence:
- Missing-field validation compares only `Option<len>` equality; it does not reject `None` in world 0: `crates/murk-engine/src/batched.rs:169`, `crates/murk-engine/src/batched.rs:173`.
- Step occurs before observation in `step_and_observe`: `crates/murk-engine/src/batched.rs:223`, `crates/murk-engine/src/batched.rs:226`.
- Observation execution fails when field is absent: `crates/murk-obs/src/plan.rs:1020`, `crates/murk-obs/src/plan.rs:1022`.

Root cause hypothesis:
- Constructor-side schema check treats “missing everywhere” as valid schema parity.

Suggested fix:
- In `BatchedEngine::new`, reject any observed `field_id` where `worlds[0].snapshot().read_field(fid)` is `None`.
- Keep current cross-world length equality check for `Some(len)` values.