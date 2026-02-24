# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [x] Lockstep
- [ ] RealtimeAsync
- [ ] Both / Unknown

## Summary

`StepResult.receipts` documentation says it does not include submission-rejected receipts, but `step_sync()` explicitly merges rejected receipts (e.g., QueueFull) into the returned `receipts` vec.

## Steps to Reproduce

1. Create a `LockstepWorld` with `max_ingress_queue: 2`.
2. Call `step_sync()` with 4 commands.
3. Inspect `StepResult.receipts` -- it contains both applied and QueueFull-rejected receipts.
4. This contradicts the doc comment at lockstep.rs:50-54.

## Expected Behavior

Either the documentation should be updated to reflect that submission-rejected receipts ARE included, or the code should be changed to exclude them (matching the current docs).

## Actual Behavior

The doc comment says "Does not include submission-rejected receipts (e.g. QueueFull)" but the implementation at lockstep.rs:117-126 merges rejected receipts into the result. A regression test at lockstep.rs:585-626 explicitly asserts this behavior.

## Reproduction Rate

Always

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [ ] Breaks bit-exact determinism
- [ ] May affect simulation behavior
- [x] No determinism impact (documentation-only issue)

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// See existing test: lockstep.rs test step_sync_surfaces_submission_rejections
// which asserts QueueFull rejections ARE in StepResult.receipts,
// contradicting the doc comment.
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-engine/src/lockstep.rs.md
**Verified lines:** lockstep.rs:50-54 (doc comment), lockstep.rs:117-126 (merge logic), lockstep.rs:585-626 (regression test)
**Root cause:** Behavior was changed to surface submission rejections through `step_sync`, but the `StepResult.receipts` field docs were not updated.
**Suggested fix:** Update the doc comment at lockstep.rs:50-54 to state that submission-rejected receipts are included, and clarify that they are merged before tick execution receipts.
