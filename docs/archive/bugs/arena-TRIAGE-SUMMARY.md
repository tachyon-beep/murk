# Arena Crate Triage Summary

**Date:** 2026-02-17
**Triaged by:** static-analysis-triage
**Scope:** All 13 static analysis reports for `crates/murk-arena/src/`

## Results Overview

| # | Source Report | Classification | Severity | Ticket |
|---|---|---|---|---|
| 1 | config.rs.md | CONFIRMED | High | [arena-missing-segment-size-validation.md](arena-missing-segment-size-validation.md) |
| 2 | descriptor.rs.md | CONFIRMED | High | [arena-placeholder-pertick-handles-in-snapshot.md](arena-placeholder-pertick-handles-in-snapshot.md) |
| 3 | error.rs.md | SKIPPED (trivial) | -- | -- |
| 4 | handle.rs.md | SKIPPED (trivial/doc-only) | -- | -- |
| 5 | lib.rs.md | SKIPPED (trivial) | -- | -- |
| 6 | pingpong.rs.md | CONFIRMED | High | [arena-generation-counter-overflow.md](arena-generation-counter-overflow.md) |
| 7 | raw.rs.md | SKIPPED (trivial/stub) | -- | -- |
| 8 | read.rs.md | CONFIRMED | High | [arena-placeholder-pertick-handles-in-snapshot.md](arena-placeholder-pertick-handles-in-snapshot.md) (merged with #2) |
| 9 | scratch.rs.md | CONFIRMED | Low | [arena-scratch-alloc-overflow.md](arena-scratch-alloc-overflow.md) |
| 10 | segment.rs.md | CONFIRMED | Medium | [arena-segment-slice-beyond-cursor.md](arena-segment-slice-beyond-cursor.md) |
| 11 | sparse.rs.md | CONFIRMED | Medium | [arena-sparse-segment-memory-leak.md](arena-sparse-segment-memory-leak.md) |
| 12 | static_arena.rs.md | CONFIRMED | High | [arena-static-arena-duplicate-field-ids.md](arena-static-arena-duplicate-field-ids.md) |
| 13 | write.rs.md | CONFIRMED | High | [arena-sparse-cow-generation-rollover.md](arena-sparse-cow-generation-rollover.md) |

## Classification Breakdown

- **CONFIRMED:** 9 reports (8 unique tickets; descriptor.rs + read.rs merged into one)
- **SKIPPED (trivial/no-bug):** 4 reports (error.rs, handle.rs, lib.rs, raw.rs)
- **FALSE_POSITIVE:** 0
- **DESIGN_AS_INTENDED:** 0
- **ALREADY_FIXED:** 0

## Tickets Created: 8

### High Severity (5)

1. **arena-missing-segment-size-validation.md** -- `ArenaConfig::segment_size` documented constraints (power-of-two, >= 1024) are never validated. Invalid configs accepted silently.

2. **arena-placeholder-pertick-handles-in-snapshot.md** -- Placeholder PerTick handles (segment_index=0, offset=0, len=total_len) are live in published_descriptor after construction. `snapshot().read()` resolves them into unallocated buffer regions.

3. **arena-generation-counter-overflow.md** -- `u32` generation counter overflows after ~4B ticks. Debug panic; release wrap breaks generation-based correctness (sparse CoW, handle staleness).

4. **arena-sparse-cow-generation-rollover.md** -- `write_sparse` uses `h.generation() < self.generation` which fails after u32 wrap, silently skipping copy-before-write and losing sparse field data.

5. **arena-static-arena-duplicate-field-ids.md** -- `StaticArena::new` accepts duplicate FieldIds, over-allocating storage and orphaning earlier allocations. Reads/writes route to last duplicate only.

### Medium Severity (2)

6. **arena-segment-slice-beyond-cursor.md** -- `Segment::slice/slice_mut` bounds-check against capacity, not cursor. Docs promise "allocated region" checking. Allows reading stale data after `reset()`.

7. **arena-sparse-segment-memory-leak.md** -- Sparse CoW writes bump-allocate new segment memory but never reclaim dead allocations. Long-running episodes exhaust the sparse pool. Mitigated by `reset()` at episode boundaries.

### Low Severity (1)

8. **arena-scratch-alloc-overflow.md** -- `ScratchRegion::alloc` growth uses unchecked `* 2` which can overflow usize. Practically unreachable on 64-bit (requires > 16 EB allocation).

## Reports Skipped (No Ticket)

- **error.rs.md** -- No bug found. Straightforward error type definition.
- **handle.rs.md** -- Trivial doc issue: `offset` documented as "byte offset" but used as f32 element index. Not a runtime bug, only a documentation inaccuracy.
- **lib.rs.md** -- No bug found. Module declarations and re-exports only.
- **raw.rs.md** -- No bug found. Intentional Phase 1 placeholder stub.

## Dependency Graph Between Bugs

Several bugs are related:

```
arena-generation-counter-overflow
  └── arena-sparse-cow-generation-rollover  (downstream consequence, separate fix location)

arena-placeholder-pertick-handles-in-snapshot
  └── arena-segment-slice-beyond-cursor  (contributing factor: slice doesn't enforce cursor bounds)
```

## Recommended Fix Priority

1. **arena-generation-counter-overflow** + **arena-sparse-cow-generation-rollover** -- Fix together. Either widen to u64 or add checked_add + change `<` to `!=`. Highest impact: data corruption in long-running training.
2. **arena-static-arena-duplicate-field-ids** -- Simple validation fix. Prevents silent memory waste and incorrect field routing.
3. **arena-missing-segment-size-validation** -- Simple validation fix. Prevents confusing downstream errors.
4. **arena-placeholder-pertick-handles-in-snapshot** -- Requires design decision (unallocated handle state vs. snapshot gating). Medium effort.
5. **arena-segment-slice-beyond-cursor** -- May break existing code if cursor check added; pair with #4.
6. **arena-sparse-segment-memory-leak** -- Known design limitation. Address in sparse compaction/epoch-reclamation work.
7. **arena-scratch-alloc-overflow** -- Low priority. Practically unreachable.
