# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

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

`FieldDescriptor::from_field_defs` initializes all PerTick fields with placeholder handles (`segment_index: 0, offset: 0, len: total_len`), and these placeholders are live in `published_descriptor` immediately after `PingPongArena::new()`, allowing `snapshot()` to read from unallocated regions of the per-tick buffer.

## Steps to Reproduce

1. Create a `PingPongArena` with PerTick fields.
2. Call `arena.snapshot()` immediately after construction (before any `begin_tick()`/`publish()` cycle).
3. Call `snap.read(per_tick_field_id)`.
4. The read resolves the placeholder handle and slices into the published buffer at `[0..total_len]`, which was never allocated via the bump allocator.

## Expected Behavior

Reading a PerTick field before any tick has been published should return `None` or an appropriate error, since no valid allocation has been made for that field in the published buffer.

## Actual Behavior

The read succeeds and returns a slice of zeros from the unallocated portion of the published buffer's backing storage. While this does not currently panic in the common case (because `total_len <= segment_size` and segments are zero-initialized to full capacity), it violates the logical invariant that `slice()` should only access allocated data. If `total_len > segment_size` (large fields or small segment sizes), this path would panic at `Segment::slice()`.

The `resolve_field` implementations in both `Snapshot` (read.rs:74) and `OwnedSnapshot` (read.rs:173) unconditionally wrap the slice call in `Some(...)` without validating that the handle represents a real allocation.

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
use murk_arena::{ArenaConfig, PingPongArena};
use murk_arena::static_arena::StaticArena;
use murk_core::{FieldId, FieldDef, FieldMutability, FieldType, BoundaryBehavior};
use murk_core::traits::FieldReader;

let config = ArenaConfig::new(100);
let field_defs = vec![(
    FieldId(0),
    FieldDef {
        name: "temp".into(),
        field_type: FieldType::Scalar,
        mutability: FieldMutability::PerTick,
        units: None,
        bounds: None,
        boundary_behavior: BoundaryBehavior::Clamp,
    },
)];
let static_arena = StaticArena::new(&[]).into_shared();
let arena = PingPongArena::new(config, field_defs, static_arena).unwrap();

// Reading before any begin_tick/publish -- returns stale zeros from unallocated region
let snap = arena.snapshot();
let data = snap.read(FieldId(0));
// BUG: data is Some(&[0.0; 100]) from unallocated storage
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-arena/src/descriptor.rs.md and /home/john/murk/docs/bugs/generated/crates/murk-arena/src/read.rs.md
**Verified lines:** descriptor.rs:69 (placeholder handle init), pingpong.rs:147 (published_descriptor = staging clone with placeholders), read.rs:74 (unchecked slice in Snapshot), read.rs:173 (unchecked slice in OwnedSnapshot), segment.rs:57-60 (slice checks capacity not cursor)
**Root cause:** The descriptor lacks an "unallocated" handle state. Placeholder PerTick handles are valid-looking but point at regions that were never bump-allocated. The read path trusts handles as authoritative without verifying allocation.
**Suggested fix:** Either (a) introduce `FieldLocation::Unallocated` or `Option<FieldHandle>` so unallocated fields return `None` on read, or (b) gate `snapshot()` to only be callable after at least one `publish()`, or (c) add a checked slice API (`SegmentList::try_slice`) that validates offset+len against cursor bounds.
