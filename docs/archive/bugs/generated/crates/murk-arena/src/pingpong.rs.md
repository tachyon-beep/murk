# Bug Report

**Date:** 2026-02-23
**Reporter:** static-analysis-agent
**Severity:** [x] Critical | [ ] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [x] murk-arena
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

`PingPongArena::begin_tick()` allows re-entry while a tick is already in progress, which can prematurely recycle sparse ranges and corrupt currently published snapshot data before `publish()` is called.

## Steps to Reproduce

1. Create a `PingPongArena` with at least two `Sparse` fields of identical `total_len` (same `cell_count` and field components).
2. Call `begin_tick()`, write only sparse field A, then drop `TickGuard` without calling `publish()`.
3. Call `begin_tick()` again, write sparse field B, drop guard, then read `snapshot().read(field_a)` before any `publish()`.

## Expected Behavior

Published snapshot data should remain stable until `publish()`; staging writes from abandoned/restarted ticks must never mutate currently published field storage.

## Actual Behavior

A second `begin_tick()` flushes pending retired sparse ranges from the un-published prior tick, and subsequent sparse allocation can reuse memory still referenced by the published descriptor, causing `snapshot()` to return mutated/corrupted values.

## Reproduction Rate

Always (deterministic when sparse field lengths match).

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
use murk_arena::{ArenaConfig, PingPongArena};
use murk_core::{BoundaryBehavior, FieldDef, FieldId, FieldMutability, FieldType};
use murk_core::id::{ParameterVersion, TickId};
use murk_arena::static_arena::StaticArena;
use murk_core::traits::{FieldWriter, SnapshotAccess};

fn main() {
    let cell_count = 8;
    let config = ArenaConfig { segment_size: 1024, max_segments: 6, max_generation_age: 1, cell_count };

    let defs = vec![
        (FieldId(0), FieldDef {
            name: "a".into(), field_type: FieldType::Scalar, mutability: FieldMutability::Sparse,
            units: None, bounds: None, boundary_behavior: BoundaryBehavior::Clamp
        }),
        (FieldId(1), FieldDef {
            name: "b".into(), field_type: FieldType::Scalar, mutability: FieldMutability::Sparse,
            units: None, bounds: None, boundary_behavior: BoundaryBehavior::Clamp
        }),
    ];

    let static_arena = StaticArena::new(&[]).into_shared();
    let mut arena = PingPongArena::new(config, defs, static_arena).unwrap();

    // Optional baseline publish; bug also occurs from generation 0 baseline.
    { let _g = arena.begin_tick().unwrap(); }
    arena.publish(TickId(1), ParameterVersion(0)).unwrap();

    // Tick in progress #1 (abandoned): write only field A.
    {
        let mut g = arena.begin_tick().unwrap();
        let a = g.writer.write(FieldId(0)).unwrap();
        a[0] = 111.0;
    } // no publish

    // Tick in progress #2 (re-enter begin_tick): write field B.
    {
        let mut g = arena.begin_tick().unwrap();
        let b = g.writer.write(FieldId(1)).unwrap();
        b[0] = 999.0;
    } // no publish

    // Still reading published snapshot, but field A can now be corrupted.
    let snap = arena.snapshot();
    println!("published A[0] = {}", snap.read(FieldId(0)).unwrap()[0]); // may print 999.0
}
```

## Additional Context

Evidence in `pingpong.rs`:
- Missing re-entry guard in `begin_tick`: `crates/murk-arena/src/pingpong.rs:222` (no `tick_in_progress` check).
- Unconditional retired-range promotion at tick start: `crates/murk-arena/src/pingpong.rs:232`.
- `tick_in_progress` only enforced in `publish()`, not in `begin_tick()`: `crates/murk-arena/src/pingpong.rs:338`.
- `snapshot()` reads published descriptor + shared sparse segments: `crates/murk-arena/src/pingpong.rs:371` and `crates/murk-arena/src/pingpong.rs:373` and `crates/murk-arena/src/pingpong.rs:375`.

Supporting allocator behavior:
- Retired sparse ranges are reused purely by matching `len`, not by field identity: `crates/murk-arena/src/sparse.rs:116`.
- That reuse path can run immediately once `flush_retired()` has promoted ranges: `crates/murk-arena/src/sparse.rs:204`.

Root cause:
- `begin_tick()` assumes previous tick reached `publish()`, but the API allows abandoning a tick and starting another. That breaks sparse reclamation phase ordering and can recycle still-published memory.

Suggested fix:
- Add a `tick_in_progress` check at the start of `begin_tick()` returning `Err(ArenaError::InvalidConfig { ... })` on double-begin, mirroring `publish()` state validation.