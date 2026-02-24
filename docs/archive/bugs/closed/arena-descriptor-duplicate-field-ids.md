# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent
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

- [x] Both / Unknown

## Summary

`FieldDescriptor::from_field_defs` silently accepts duplicate `FieldId`s, overwriting earlier entries via `IndexMap::insert`. This was fixed in `StaticArena::new` as part of #14, but the same class of bug remains in `FieldDescriptor`.

## Steps to Reproduce

1. Construct a `field_defs` Vec containing two entries with the same `FieldId` but different `FieldDef` values (e.g. different `FieldType` or `FieldMutability`).
2. Call `FieldDescriptor::from_field_defs(&field_defs, cell_count)`.
3. Observe that construction succeeds and `desc.len()` equals the number of unique IDs (not input length), with only the last duplicate entry retained.

## Expected Behavior

Duplicate `FieldId` input should be rejected with `Err(ArenaError::InvalidConfig { ... })`, so callers cannot silently lose field definitions. This matches the behavior already implemented in `StaticArena::new` (which panics on duplicates).

## Actual Behavior

Construction succeeds. The `IndexMap::insert` call at `descriptor.rs:85` overwrites the earlier entry for the duplicate key. The earlier field definition is silently discarded, resulting in incorrect arena layout (wrong component count, wrong mutability class, wrong total_len).

## Reproduction Rate

Always (deterministic).

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
use murk_arena::descriptor::FieldDescriptor;
use murk_core::{BoundaryBehavior, FieldDef, FieldId, FieldMutability, FieldType};

let defs = vec![
    (
        FieldId(42),
        FieldDef {
            name: "first".into(),
            field_type: FieldType::Scalar,
            mutability: FieldMutability::PerTick,
            units: None,
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        },
    ),
    (
        FieldId(42), // duplicate ID
        FieldDef {
            name: "second".into(),
            field_type: FieldType::Vector { dims: 3 },
            mutability: FieldMutability::Static,
            units: None,
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        },
    ),
];

let desc = FieldDescriptor::from_field_defs(&defs, 10).unwrap();

// Bug: succeeds with len == 1 (first entry silently dropped).
// Expected: Err(ArenaError::InvalidConfig { ... }) rejecting the duplicate.
assert_eq!(desc.len(), 1); // passes — "first" is gone
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-arena/src/descriptor.rs.md`

**Affected lines:**
- `crates/murk-arena/src/descriptor.rs:85` -- `entries.insert(*id, FieldEntry { handle, meta })` overwrites silently on duplicate key.
- `crates/murk-arena/src/descriptor.rs:87` -- Returns `Ok(Self { entries })` with no duplicate validation.

**Propagation path:**
- `crates/murk-arena/src/pingpong.rs:134` forwards `field_defs` directly into `FieldDescriptor::from_field_defs(...)`, so external callers of `PingPongArena::new` can trigger silent descriptor loss.

**Related fix (same class of bug):**
- `crates/murk-arena/src/static_arena.rs:39-48` -- `StaticArena::new` explicitly checks for and rejects duplicate `FieldId`s with a panic. This was added as part of ticket #14 (`arena-static-arena-duplicate-field-ids`). The same fix was not applied to `FieldDescriptor::from_field_defs`.

**Root cause:** `IndexMap::insert` returns the old value on key collision but the return value is ignored. No pre-insertion check for duplicate keys exists.

**Suggested fix:** In `from_field_defs`, detect duplicates by checking the return value of `insert`:
```rust
if entries.insert(*id, FieldEntry { handle, meta }).is_some() {
    return Err(ArenaError::InvalidConfig {
        reason: format!("duplicate FieldId({}) in field_defs", id.0),
    });
}
```
This returns an error (rather than panicking like `StaticArena`) since `from_field_defs` already returns `Result`.
