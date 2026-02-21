# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-arena

## Engine Mode

- [x] Both / Unknown

## Summary

Two related per-tick allocation issues:

1. **`publish()` (pingpong.rs:323):** `self.staging_descriptor = self.published_descriptor.clone()` clones the entire `FieldDescriptor` (an `IndexMap` with `String` field names) every single tick. For 100 fields, this is 100 heap allocations per tick for the `String` values alone.

2. **`WriteArena::write()` (write.rs:150):** `let meta = entry.meta.clone()` clones the `FieldMeta` struct (which contains a `String` name) on every write call, just to read `mutability` and `total_len`.

## Expected Behavior

Field names should use `Arc<str>` or interned strings so cloning is a reference count bump, not a heap allocation. Alternatively, split `FieldDescriptor` into an immutable metadata table (names, types) and a mutable handle table (just `IndexMap<FieldId, FieldHandle>`, no strings).

## Actual Behavior

Per-tick String heap allocations proportional to field count.

## Additional Context

**Source:** murk-arena audit, PERF-3/PERF-4/ARCH-3
**Files:** `crates/murk-arena/src/pingpong.rs:323`, `crates/murk-arena/src/write.rs:150`
**Suggested fix:** `Arc<str>` for field names (quick win), or split metadata from handles (proper fix).
