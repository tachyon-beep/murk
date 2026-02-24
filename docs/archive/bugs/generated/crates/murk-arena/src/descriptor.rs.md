# Bug Report

**Date:** 2026-02-23  
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

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`FieldDescriptor::from_field_defs` silently accepts duplicate `FieldId`s and overwrites earlier entries, causing field definitions to be dropped without any error.

## Steps to Reproduce

1. Build `field_defs` with two entries that share the same `FieldId` but different `FieldDef` values.
2. Call `FieldDescriptor::from_field_defs(&field_defs, cell_count)`.
3. Observe that construction succeeds and `desc.len()` equals unique IDs (not input length), with only the last duplicate retained.

## Expected Behavior

Duplicate `FieldId` input should be rejected (e.g., `Err(ArenaError::InvalidConfig { ... })`) so callers cannot silently lose field metadata/allocation intent.

## Actual Behavior

Construction succeeds; duplicate IDs are collapsed by overwrite semantics, and earlier field definitions are silently discarded.

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

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_arena::descriptor::FieldDescriptor;
use murk_core::{BoundaryBehavior, FieldDef, FieldId, FieldMutability, FieldType};

fn main() {
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

    // Expected if duplicates were rejected/prevented: either Err or len == 2.
    // Actual: len == 1 (first entry overwritten silently).
    assert_eq!(desc.len(), 2);
}
```

## Additional Context

Evidence in target file:
- `crates/murk-arena/src/descriptor.rs:85` uses `entries.insert(*id, FieldEntry { ... })`; `IndexMap::insert` overwrites an existing key.
- `crates/murk-arena/src/descriptor.rs:87` returns `Ok(Self { entries })` with no duplicate validation.
- `crates/murk-arena/src/descriptor.rs:113` reports `len()` from map cardinality, which masks dropped duplicates.

Propagation path:
- `crates/murk-arena/src/pingpong.rs:105` exposes public `PingPongArena::new(..., field_defs: Vec<(FieldId, FieldDef)>, ...)`.
- `crates/murk-arena/src/pingpong.rs:134` forwards directly into `FieldDescriptor::from_field_defs(...)`, so external callers can trigger silent descriptor loss.

Related invariant evidence:
- `crates/murk-arena/src/static_arena.rs:39` explicitly rejects duplicate `FieldId`s and notes silent `IndexMap::insert` overwrite risk.

Suggested fix:
- In `from_field_defs`, detect duplicates (`if entries.insert(...).is_some()`) and return `ArenaError::InvalidConfig` with the duplicate `FieldId` in the reason.