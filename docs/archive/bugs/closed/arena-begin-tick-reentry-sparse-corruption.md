# Bug Report

**Date:** 2026-02-24
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

- [x] Both / Unknown

## Summary

`PingPongArena::begin_tick()` has no re-entry guard: calling it twice without an intervening `publish()` unconditionally flushes pending retired sparse ranges and resets the staging buffer, which can corrupt currently published snapshot data by recycling memory still referenced by the published descriptor.

## Steps to Reproduce

1. Create a `PingPongArena` with at least one `Sparse` field.
2. Call `begin_tick()`, write the sparse field (triggers CoW -- old range moves to `pending_retired`), then call `publish()`.
3. Call `begin_tick()` again, write the sparse field (new CoW -- published range moves to `pending_retired`), but do NOT call `publish()`.
4. Call `begin_tick()` a second time without publishing. This calls `flush_retired()` at line 232, which promotes the still-referenced published range into `retired_ranges`.
5. The subsequent sparse allocation inside this `begin_tick()` (or a later one) can reuse that range, overwriting data that the published snapshot still points to.

## Expected Behavior

Either:

- `begin_tick()` should reject re-entry while `tick_in_progress == true`, returning `Err(ArenaError::InvalidConfig { ... })`, or
- `begin_tick()` should safely roll back the abandoned tick state before starting a new one.

Published snapshot data must remain stable and uncorrupted until the next successful `publish()`.

## Actual Behavior

A second `begin_tick()` call while `tick_in_progress == true`:

1. Calls `flush_retired()` (line 232) which promotes ranges from `pending_retired` to `retired_ranges` -- but `pending_retired` may contain ranges still referenced by the published descriptor (because `publish()` was never called to swap descriptors).
2. Resets the staging buffer (lines 235-239), discarding any writes from the abandoned tick.
3. Re-allocates all PerTick fields, potentially reusing segment memory that the published snapshot references.
4. Sets `tick_in_progress = true` again (line 282), masking the fact that no publish occurred.

Subsequent sparse allocations via `alloc()` can match and reuse the prematurely promoted ranges, mutating memory visible through `snapshot()`.

## Reproduction Rate

Always (deterministic when sparse fields are present and CoW writes occur before the abandoned tick).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.9)

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
use murk_arena::{ArenaConfig, PingPongArena};
use murk_arena::static_arena::StaticArena;
use murk_core::{BoundaryBehavior, FieldDef, FieldId, FieldMutability, FieldType};
use murk_core::id::{ParameterVersion, TickId};
use murk_core::traits::{FieldWriter, SnapshotAccess};

let cell_count = 8;
let config = ArenaConfig {
    segment_size: 1024,
    max_segments: 6,
    max_generation_age: 1,
    cell_count,
};

let defs = vec![
    (FieldId(0), FieldDef {
        name: "a".into(),
        field_type: FieldType::Scalar,
        mutability: FieldMutability::Sparse,
        units: None, bounds: None,
        boundary_behavior: BoundaryBehavior::Clamp,
    }),
];

let static_arena = StaticArena::new(&[]).into_shared();
let mut arena = PingPongArena::new(config, defs, static_arena).unwrap();

// Tick 1: write sparse field, publish normally.
{
    let mut g = arena.begin_tick().unwrap();
    let a = g.writer.write(FieldId(0)).unwrap();
    a[0] = 42.0;
}
arena.publish(TickId(1), ParameterVersion(0)).unwrap();

// Verify published value.
assert_eq!(arena.snapshot().read(FieldId(0)).unwrap()[0], 42.0);

// Tick 2: begin_tick + sparse write (CoW retires tick 1's range to pending),
// but do NOT publish -- abandon the tick.
{
    let mut g = arena.begin_tick().unwrap();
    let a = g.writer.write(FieldId(0)).unwrap();
    a[0] = 100.0;
} // no publish -- tick abandoned

// Tick 3: begin_tick re-entry. flush_retired() promotes tick 1's range
// (still referenced by published descriptor!) to retired_ranges.
{
    let mut g = arena.begin_tick().unwrap();
    // Sparse alloc may now reuse tick 1's range, overwriting published data.
    let a = g.writer.write(FieldId(0)).unwrap();
    a[0] = 999.0;
}

// Published snapshot may now return 999.0 instead of 42.0.
let snap = arena.snapshot();
let val = snap.read(FieldId(0)).unwrap()[0];
// BUG: val may be 999.0 (corrupted) instead of 42.0 (published).
assert_eq!(val, 42.0); // may fail
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-arena/src/pingpong.rs.md`

**Affected lines:**

- `crates/murk-arena/src/pingpong.rs:222` -- `begin_tick()` entry point: no `tick_in_progress` check.
- `crates/murk-arena/src/pingpong.rs:232` -- Unconditional `self.sparse_slab.flush_retired()` promotes pending ranges regardless of whether `publish()` was called.
- `crates/murk-arena/src/pingpong.rs:235-239` -- Unconditional buffer reset discards abandoned tick writes.
- `crates/murk-arena/src/pingpong.rs:282` -- `self.tick_in_progress = true` set without checking prior state.
- `crates/murk-arena/src/pingpong.rs:338` -- `publish()` correctly checks `tick_in_progress`, but `begin_tick()` does not have the symmetric check.

**Supporting allocator behavior:**

- `crates/murk-arena/src/sparse.rs:116` -- Retired range reuse is by matching `len`, not by field identity: any sparse allocation of the same size can reuse the range.
- `crates/murk-arena/src/sparse.rs:204` -- `flush_retired()` moves all pending ranges to the reusable pool.

**Root cause:** `begin_tick()` assumes the previous tick reached `publish()`, but the API allows abandoning a tick and starting another. The unconditional `flush_retired()` call violates the two-phase reclamation invariant: ranges freed during an unpublished tick are promoted to reusable status while the published descriptor still references them.

**Suggested fix:** Add a `tick_in_progress` guard at the start of `begin_tick()`:

```rust
pub fn begin_tick(&mut self) -> Result<TickGuard<'_>, ArenaError> {
    if self.tick_in_progress {
        return Err(ArenaError::InvalidConfig {
            reason: "begin_tick() called while a tick is already in progress \
                     (missing publish() call)".into(),
        });
    }
    // ... rest of method
}
```

This mirrors the state guard already present in `publish()` (line 338) and prevents the two-phase reclamation invariant from being violated.
