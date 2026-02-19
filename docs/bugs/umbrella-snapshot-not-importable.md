# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk (umbrella)

## Engine Mode

- [x] Both / Unknown

## Summary

`LockstepWorld::step_sync()` returns `StepResult<'w>` containing `pub snapshot: Snapshot<'w>`. `LockstepWorld::snapshot()` returns `Snapshot<'_>`. `LockstepWorld::reset()` returns `Result<Snapshot<'_>, ConfigError>`. The `Snapshot` type comes from `murk_arena::read::Snapshot`, but `murk-arena` is not a dependency of the `murk` facade crate.

This means users cannot write `fn process(snap: murk::???::Snapshot)` — there is no path to name this type through the facade. They can use snapshots through trait methods (`FieldReader::read()`, `SnapshotAccess::tick_id()`), but cannot accept or store `Snapshot` by name.

This is a leaky abstraction: the engine exposes arena types in its public API without the facade re-exporting them.

## Expected Behavior

`Snapshot` and `OwnedSnapshot` importable through `murk::engine::Snapshot` or similar.

## Actual Behavior

Type is present in return types but cannot be named by downstream crates using only `murk`.

## Additional Context

**Source:** murk umbrella audit, Finding 16
**File:** `crates/murk/src/lib.rs`, `crates/murk/Cargo.toml`
**Suggested fix:** Either add `murk-arena` as a dependency and `pub use murk_arena as arena;`, or targeted re-exports: `pub use murk_arena::read::{Snapshot, OwnedSnapshot};` in the engine module.
