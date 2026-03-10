# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [x] Lockstep
- [ ] RealtimeAsync
- [ ] Both / Unknown

## Summary

`BatchedEngine::new` accepts an `ObsSpec` referencing a `FieldId` that is missing in ALL worlds (including world 0); `step_and_observe` then steps all worlds before observation fails, violating error atomicity.

## Steps to Reproduce

1. Build a `BatchedEngine` with 2+ worlds that only define `FieldId(0)`.
2. Pass an `ObsSpec` referencing `FieldId(1)` (missing in every world).
3. Call `step_and_observe` and observe that worlds advance a tick before the error is returned.

## Expected Behavior

Either `BatchedEngine::new` should reject an `ObsSpec` that references fields absent from world 0's snapshot, or `step_and_observe` should detect the problem in pre-flight (before stepping).

## Actual Behavior

The constructor's field schema validation (`batched.rs:162-188`) compares `ref_snap.read_field(fid).map(|d| d.len())` across worlds. When the field is `None` in world 0 and `None` in all other worlds, `None != None` evaluates to `false`, so validation passes. The `validate_observe_buffers` pre-flight check (`batched.rs:287-309`) only validates buffer sizes and plan type, not field existence. Worlds are stepped via `step_all` (`batched.rs:223`), then observation fails in `observe_all_inner` when `ObsPlan::execute_batch` encounters the missing field. Ticks have already advanced.

## Reproduction Rate

Always

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** 0.1.8 / HEAD (feat/release-0.1.9)

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
// Sketch: create worlds with only FieldId(0), obs spec references FieldId(1)
let spec = ObsSpec {
    entries: vec![ObsEntry {
        field_id: FieldId(1), // missing in all worlds
        region: ObsRegion::Fixed(RegionSpec::All),
        pool: None,
        transform: ObsTransform::Identity,
        dtype: ObsDtype::F32,
    }],
};
let configs = vec![make_config_one_field(1), make_config_one_field(2)];
let mut engine = BatchedEngine::new(configs, Some(&spec)).unwrap(); // succeeds
let mut out = vec![0.0; engine.num_worlds() * engine.obs_output_len()];
let mut mask = vec![0u8; engine.num_worlds() * engine.obs_mask_len()];
let err = engine.step_and_observe(&[vec![], vec![]], &mut out, &mut mask).unwrap_err();
// err is BatchError::Observe, but worlds have already advanced to tick 1
assert_eq!(engine.world_tick(0), Some(TickId(1))); // atomicity violated
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-engine/src/batched.rs.md`

**Affected lines:**
- Constructor field schema validation: `crates/murk-engine/src/batched.rs:162-188`
- Pre-flight buffer check (misses field existence): `crates/murk-engine/src/batched.rs:287-309`
- Step before observe: `crates/murk-engine/src/batched.rs:223`

**Root cause:** The constructor treats "field missing everywhere" as valid schema parity. The `ref_snap.read_field(fid)` at line 169 returns `None` for world 0, and the cross-world comparison only checks for mismatches, not for absence.

**Suggested fix:** In `BatchedEngine::new`, after the cross-world comparison loop, add an explicit check that every observed `field_id` exists in world 0: reject construction if `ref_snap.read_field(fid).is_none()`.
