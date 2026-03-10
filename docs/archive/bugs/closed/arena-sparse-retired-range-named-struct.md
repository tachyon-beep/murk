# Bug Report

**Date:** 2026-02-21
**Reporter:** sparse-reclamation-review panel
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low (P4 — code quality)

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

`SparseSlab` uses raw `(u16, u32, u32)` tuples for `retired_ranges` and `pending_retired`. A named `RetiredRange` struct would improve readability and provide a natural extension point for future monitoring (e.g. `created_at_generation: u32` for fragmentation detection).

## Steps to Reproduce

N/A — code quality improvement.

## Expected Behavior

A named struct like:
```rust
struct RetiredRange {
    segment_index: u16,
    offset: u32,
    len: u32,
}
```

## Actual Behavior

Raw tuples `(u16, u32, u32)` used in 4 locations within `sparse.rs`. Field ordering is a transposition hazard (offset vs. len are both `u32`).

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
N/A — code quality improvement
```

## Minimal Reproducer

```
N/A
```

## Additional Context

**Origin:** Consensus finding from the sparse-reclamation-review panel (2026-02-21). Flagged independently by architecture, Python engineering, and systems thinking reviewers.

**Rationale:** The tuple flows through a safety-critical path (pending_retired → flush_retired → retired_ranges → alloc reuse). A named struct:
1. Prevents field transposition bugs (offset vs. len are both u32)
2. Provides a natural doc-comment home for the fixed-size invariant
3. Creates an extension point for `created_at_generation` if fragmentation monitoring is added later

**Scope:** Internal to `SparseSlab` — no public API change. Estimated 4 locations to update in `sparse.rs`.
