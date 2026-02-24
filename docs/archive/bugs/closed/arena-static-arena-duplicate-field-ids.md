# Bug Report — FIXED

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low
**Fixed:** 2026-02-18

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

`StaticArena::new` silently accepts duplicate `FieldId` entries, over-allocating backing storage and routing reads/writes to the last duplicate's offset, leaving earlier allocated regions permanently unreachable.

## Steps to Reproduce

1. Call `StaticArena::new(&[(FieldId(0), 100), (FieldId(0), 50)])`.
2. Total storage allocated: 150 f32s (both entries summed).
3. `field_offsets` maps `FieldId(0)` to `(100, 50)` (second insert overwrites first).
4. The first 100 f32s at offset 0 are permanently unreachable.
5. `read_field(FieldId(0))` returns `&data[100..150]`, missing the first region entirely.
6. `field_count()` returns 1 but 150 f32s are allocated (metadata/storage mismatch).

## Expected Behavior

`StaticArena::new` should either reject duplicate `FieldId`s with an error/panic, or deduplicate them (using only the last entry's size). The current behavior silently wastes memory and creates a confusing internal state.

## Actual Behavior

At static_arena.rs:39, total storage is computed by summing ALL entries including duplicates. At static_arena.rs:45, `field_offsets.insert(id, (cursor, len))` overwrites the mapping for duplicate keys (IndexMap semantics). At static_arena.rs:46, `cursor` advances unconditionally, so the earlier allocation's memory range becomes orphaned. `field_count()` (line 90) returns the number of unique keys, which disagrees with the allocated storage size.

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
use murk_arena::static_arena::StaticArena;
use murk_core::FieldId;

let arena = StaticArena::new(&[
    (FieldId(0), 100),
    (FieldId(0), 50),  // duplicate FieldId
]);

// Memory allocated: 150 f32s
assert_eq!(arena.memory_bytes(), 150 * 4);

// But only 1 field tracked
assert_eq!(arena.field_count(), 1);

// Reads only the second allocation (offset 100..150)
let data = arena.read_field(FieldId(0)).unwrap();
assert_eq!(data.len(), 50); // first 100 elements are orphaned
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-arena/src/static_arena.rs.md
**Verified lines:** static_arena.rs:39 (total sums all entries), static_arena.rs:45 (IndexMap insert overwrites), static_arena.rs:46 (cursor always advances), static_arena.rs:56-67 (read/write use final mapping), static_arena.rs:90 (field_count reflects unique keys)
**Root cause:** The constructor takes a slice of tuples (not a map) and does not validate FieldId uniqueness. `IndexMap::insert` replacement semantics plus unconditional cursor advancement create a silent inconsistency.
**Suggested fix:** Validate uniqueness at construction: either return `Result<Self, ArenaError>` and reject duplicates, or panic with a clear message. Only advance `cursor` when inserting a truly new key.

## Resolution

Added O(n^2) duplicate FieldId check at the start of `StaticArena::new()`, before allocation. Panics with a clear message identifying the duplicate FieldId. Duplicate FieldIds are a programming error, so `assert!` (panic) is appropriate — consistent with Rust convention and the existing codebase validation patterns.

Tests added: `new_rejects_duplicate_field_ids`, `new_rejects_non_adjacent_duplicate_field_ids`, `new_accepts_distinct_field_ids`.
