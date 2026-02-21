# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

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
- [ ] Both / Unknown

## Summary

`SparseSlab::alloc` reuses slot metadata via the free list but always bump-allocates new segment memory, so repeated sparse CoW writes accumulate dead allocations in the sparse segment pool until `CapacityExceeded`.

## Steps to Reproduce

1. Create a `PingPongArena` with a sparse field.
2. On every tick, write to the sparse field (triggering CoW).
3. Each CoW write bump-allocates `total_len` new f32s in the sparse segment pool.
4. Old allocations are never reclaimed (bump-only, pool is never reset).
5. After enough ticks, `SegmentList::alloc` returns `CapacityExceeded`.

## Expected Behavior

Sparse field CoW writes in long-running simulations should reclaim memory from dead allocations, either via compaction or by tracking and reusing freed segment ranges.

## Actual Behavior

At sparse.rs:69, `segments.alloc(len)?` always consumes new bump space. At sparse.rs:72-74, the old slot is marked dead and its INDEX is added to the free list, but the underlying segment memory `(segment_index, offset, len)` is permanently consumed. The sparse pool is documented as "dedicated, never reset" (pingpong.rs:56), confirming no reclamation path exists.

With default config (6 sparse segments of 16M f32s each = 384MB), a 100-cell scalar sparse field (100 f32s per CoW) exhausts the pool after ~960K ticks. At 10K ticks/sec, this is ~96 seconds of continuous operation with per-tick sparse writes.

Note: `PingPongArena::reset()` (pingpong.rs:344) recreates the sparse pool entirely, so episode-boundary resets do reclaim memory. The issue only manifests within a single episode.

## Reproduction Rate

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

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
// Conceptual: run many ticks writing to a sparse field
use murk_arena::{ArenaConfig, PingPongArena};
// ... setup arena with sparse field ...

for tick in 1..=1_000_000 {
    {
        let mut guard = arena.begin_tick().unwrap();
        // Each write triggers CoW, consuming new sparse segment memory
        let data = guard.writer.write(sparse_field_id).unwrap();
        data[0] = tick as f32;
    }
    arena.publish(TickId(tick), ParameterVersion(0));
    // Eventually: CapacityExceeded from sparse pool exhaustion
}
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-arena/src/sparse.rs.md
**Verified lines:** sparse.rs:69 (always bumps), sparse.rs:72-74 (free list tracks slot indices, not segment ranges), segment.rs:13 (bump-only, no deallocation), pingpong.rs:56 (sparse pool documented as "dedicated, never reset"), pingpong.rs:344 (reset() recreates sparse pool)
**Root cause:** The `free_list` tracks `slots` vector indices for metadata reuse, not freed segment memory ranges. The allocator recycles bookkeeping entries but not underlying storage.
**Suggested fix:** Track retired sparse ranges separately and allocate from the reclaim list before falling back to `SegmentList::alloc`. Alternatively, implement periodic sparse compaction that rewrites only live sparse fields into a fresh segment list. The epoch-reclamation design doc (docs/design/epoch-reclamation.md) may already cover this for RealtimeAsync mode; the same mechanism should apply to Lockstep.

## Resolution

**Fixed:** 2026-02-21
**Commit branch:** feat/release-0.1.7

**Fix:** Two-phase retired range reclamation in `SparseSlab`:
- `pending_retired: Vec<(u16, u32, u32)>` — segment ranges freed during the current tick (published descriptor may still reference them).
- `retired_ranges: Vec<(u16, u32, u32)>` — ranges freed in previous ticks, safe to reuse.
- `flush_retired()` moves pending → retired; called by `PingPongArena::begin_tick()` after publish.
- `alloc()` searches `retired_ranges` for an exact-size match before falling back to bump allocation.

**Files changed:** `crates/murk-arena/src/sparse.rs`, `crates/murk-arena/src/pingpong.rs`
**Tests added:** `retired_range_reused_after_flush`, `pending_retired_not_reused_before_flush`, `many_cow_writes_with_flush_stays_bounded`, `different_size_ranges_not_mixed`, `sparse_cow_does_not_leak_segment_memory` (integration)
