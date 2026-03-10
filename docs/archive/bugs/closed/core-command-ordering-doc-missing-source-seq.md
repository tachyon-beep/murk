# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [x] murk-core
- [ ] murk-engine
- [ ] murk-arena
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

The doc comment on `Command` (lines 7-10) describes ordering as `priority_class` then `source_id` then `arrival_seq`, omitting `source_seq` which is actually the third sort key. The actual ingress sort order in `murk-engine/src/ingress.rs:161-167` is `(priority_class, source_id|MAX, source_seq|MAX, arrival_seq)`. This can mislead external clients implementing deterministic command injection.

## Steps to Reproduce

1. Read `crates/murk-core/src/command.rs` lines 7-10.
2. Compare with `crates/murk-engine/src/ingress.rs` lines 161-167.
3. Observe `source_seq` is used as third sort key but not documented.

## Expected Behavior

Doc comment should describe the full sort key: `priority_class`, then `source_id`, then `source_seq`, then `arrival_seq`.

## Actual Behavior

Doc comment says: "ordered by `priority_class` (lower = higher priority), then by `source_id` for disambiguation, then by `arrival_seq` as a final tiebreaker" -- omitting `source_seq`.

## Reproduction Rate

- Always (doc is static)

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
// No runtime reproducer -- doc-only issue.
// crates/murk-core/src/command.rs:7-10 says:
//   priority_class -> source_id -> arrival_seq
// crates/murk-engine/src/ingress.rs:161-167 actually sorts:
//   (priority_class, source_id|MAX, source_seq|MAX, arrival_seq)
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-core/src/command.rs.md
**Verified lines:** command.rs:7-10, ingress.rs:161-167
**Root cause:** Doc comment was not updated when `source_seq` was added to the ingress sort key.
**Suggested fix:** Update the doc comment at `command.rs:7-10` to include `source_seq` as the third sort key. Consider adding a doc/test assertion to keep the contract from drifting.
