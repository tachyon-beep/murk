# Bug Report

**Date:** 2026-02-21
**Reporter:** sparse-reclamation-review panel (systems thinking reviewer)
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low (P4 — observability)

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

- [x] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

The sparse reclamation fix (#29) added `retired_range_count()` and `pending_retired_count()` accessors to `SparseSlab`, but these are not wired into the engine's metrics or exposed through FFI/Python. A steadily growing `retired_ranges` pool would be the earliest signal of a reclamation regression, but there is currently no way to observe it from outside the arena.

## Steps to Reproduce

N/A — observability gap.

## Expected Behavior

Sparse reclamation metrics (retired range count, pending retired count, reuse hit/miss ratio) should be available through the same metrics pipeline that exposes `StepMetrics` (total_us, command_processing_us, per-propagator timings).

## Actual Behavior

The accessors exist at `SparseSlab::retired_range_count()` (sparse.rs:176) and `SparseSlab::pending_retired_count()` (sparse.rs:181) but are only used in unit tests. No engine-level or FFI-level plumbing exposes them.

## Reproduction Rate

Always

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
N/A — observability gap
```

## Minimal Reproducer

```
N/A
```

## Additional Context

**Origin:** Systems thinking reviewer finding from sparse-reclamation-review panel (2026-02-21). Classified as Low priority — the underlying reclamation mechanism is correct, this is about making correctness observable.

**Suggested approach:**
1. Add `sparse_retired_ranges: u32` and `sparse_pending_retired: u32` to `MurkStepMetrics` in murk-ffi
2. Populate from `PingPongArena` after each step
3. Expose through Python `StepMetrics` class
4. Optional: add a `sparse_reuse_hits: u32` / `sparse_reuse_misses: u32` counter to SparseSlab for hit-rate monitoring

**Systems context:** The reclamation loop (B1 in the review's terminology) is a one-tick delayed balancing loop. A growing `retired_range_count` would signal the loop has broken — most likely due to a field size mismatch (currently architecturally prevented, but worth monitoring if dynamic schemas are ever added).
