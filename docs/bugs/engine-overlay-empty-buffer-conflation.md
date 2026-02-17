# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [x] Lockstep
- [x] RealtimeAsync
- [ ] Both / Unknown

## Summary

Overlay cache readers (`BaseFieldCache` and `StagedFieldCache`) conflate "missing/stale field" with "present but empty field" by using `Vec::is_empty()` as a freshness sentinel, causing valid zero-component field reads to return `None`.

## Steps to Reproduce

1. Register a field with `FieldType::Vector { dims: 0 }` (zero components per cell).
2. The arena stores a zero-length buffer for this field.
3. `BaseFieldCache::populate()` calls `extend_from_slice(&[])`, storing an empty `Vec`.
4. `BaseFieldCache::read()` at overlay.rs:113-116 filters with `!v.is_empty()` and returns `None`.
5. A propagator reading this field via the overlay sees `None` instead of `Some(&[])`.

## Expected Behavior

A field that was successfully populated with zero-length data should return `Some(&[])` from `read()`, distinguishing it from a field that was never populated (stale/missing).

## Actual Behavior

The `is_empty()` filter causes empty-but-valid field data to be treated as stale/missing, returning `None`.

## Reproduction Rate

Always (with zero-component fields)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [x] Propagators may fail or produce incorrect results when reading zero-component fields
- [ ] No determinism impact

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// Requires a FieldType::Vector { dims: 0 } field definition.
// The overlay cache will store an empty Vec for this field,
// then read() will return None instead of Some(&[]).
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-engine/src/overlay.rs.md
**Verified lines:** overlay.rs:113-116 (BaseFieldCache::read filter), overlay.rs:158-161 (StagedFieldCache::read filter), overlay.rs:99-106 (populate with empty slice), murk-core/src/field.rs:21-38 (Vector{dims:0} produces 0 components)
**Root cause:** Cache freshness is encoded implicitly by vector emptiness, but emptiness is also a valid payload shape (zero-component fields), so the sentinel collides with valid data.
**Suggested fix:** Store explicit presence/freshness metadata per entry (e.g., a `populated: bool` flag) instead of relying on `!is_empty()`. Alternatively, reject zero-component fields in `WorldConfig::validate()` if they are not intended to be supported.
