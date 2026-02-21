# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-arena

## Engine Mode

- [x] Both / Unknown

## Summary

`Segment::slice()` and `Segment::slice_mut()` use `assert!()` for bounds checking, and `SegmentList::slice()`/`slice_mut()` use unchecked indexing (`self.segments[segment_index as usize]`). These are reachable from public API paths via `Snapshot::resolve_field()` and `OwnedSnapshot::resolve_field()`. A stale or corrupt `FieldHandle` will cause a panic with an unhelpful message instead of returning `None`/`Err`. This is particularly problematic because `resolve_field()` returns `Option<&[f32]>` — callers expect `None` for missing fields but get panics for corrupt handles.

Three specific sites:
1. `segment.rs:57-66` — `Segment::slice()` uses `assert!(end <= self.cursor)`
2. `segment.rs:73-82` — `Segment::slice_mut()` same pattern
3. `segment.rs:187-194` — `SegmentList::slice()` uses `self.segments[segment_index as usize]` without bounds check

## Expected Behavior

Return `None` or `Err` for invalid handles, consistent with the `Option` return type of `resolve_field()`.

## Actual Behavior

Panics with opaque index-out-of-bounds or assertion failure messages.

## Additional Context

**Source:** murk-arena audit, SAFE-2, SAFE-3, ERR-1
**Files:** `crates/murk-arena/src/segment.rs:57-82,187-194`, `crates/murk-arena/src/read.rs:69-86,168-185`
**Suggested fix:** Either make segment slice methods return `Option`/`Result` and propagate errors, or restrict them to `pub(crate)` and add descriptive `debug_assert!` messages.
