# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
- [x] murk-replay
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

`snapshot_hash` documents "Returns `0` if the snapshot contains no readable fields" (line 45), but the implementation initializes with `FNV_OFFSET` (0xcbf29ce484222325, non-zero) and returns it directly when no fields are hashed. For `field_count == 0`, the function returns `FNV_OFFSET` instead of `0`.

This is a doc/code contract violation. If any consumer relies on the documented "returns 0 for empty" behavior (e.g., to detect empty snapshots), it will incorrectly treat empty snapshots as non-empty. For replay comparison, this is not a correctness issue since both sides compute the same non-zero hash for empty snapshots.

## Steps to Reproduce

1. Create an empty `MockSnapshot` with no fields.
2. Call `snapshot_hash(&snap, 0)`.
3. Observe the return value is `0xcbf29ce484222325`, not `0`.

## Expected Behavior

Per documentation: returns `0` when no readable fields exist.

## Actual Behavior

Returns `FNV_OFFSET` (0xcbf29ce484222325).

## Reproduction Rate

- Deterministic.

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
use murk_replay::hash::snapshot_hash;
use murk_test_utils::MockSnapshot;
use murk_core::id::{TickId, WorldGenerationId, ParameterVersion};

let snap = MockSnapshot::new(TickId(0), WorldGenerationId(0), ParameterVersion(0));
let h = snapshot_hash(&snap, 0);

// BUG: doc says this should be 0, but it's FNV_OFFSET
assert_eq!(h, 0); // FAILS: h == 0xcbf29ce484222325
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-replay/src/hash.rs.md
**Verified lines:** hash.rs:45 (doc claim), hash.rs:47 (FNV_OFFSET init), hash.rs:11 (FNV_OFFSET value), hash.rs:60 (return), hash.rs:151 (test only checks determinism, not value)
**Root cause:** Doc comment was written for a version that returned 0 for empty snapshots; code was later changed to always use FNV offset initialization, but doc was not updated.
**Suggested fix:** Either (a) update the doc to remove the "returns 0" claim and document the actual behavior, or (b) add `if field_count == 0 { return 0; }` at the top to match the documented contract. Option (a) is safer since changing the hash value would break replay compatibility with existing recordings.
