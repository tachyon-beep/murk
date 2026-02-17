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

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`Segment::slice` and `Segment::slice_mut` allow access to memory beyond the bump-allocated cursor up to the full backing capacity, violating their documented contract and enabling reads of stale data after `reset()`.

## Steps to Reproduce

1. Create a `Segment` with capacity 1024.
2. Allocate 100 elements via `alloc(100)`. Write data to them.
3. Call `reset()` (sets cursor to 0).
4. Call `slice(0, 100)` -- succeeds and returns stale data despite cursor being 0.
5. Alternatively: call `slice(0, 1024)` -- succeeds up to full capacity, far beyond any allocation.

## Expected Behavior

Per the doc comment on `Segment::slice` (segment.rs:56): "Panics if offset + len exceeds the segment's **allocated region**." After `reset()`, the allocated region is 0 elements, so `slice(0, 100)` should panic (or return an error).

## Actual Behavior

`Segment::slice` (segment.rs:57-60) computes `end = start + len as usize` and indexes `self.data[start..end]`, which checks against `self.data.len()` (full capacity), not `self.cursor` (allocated region). Any offset+len within capacity succeeds, including reads of logically freed/stale data.

The same issue applies to `Segment::slice_mut` (segment.rs:68-71) and propagates through `SegmentList::slice` (segment.rs:177) and `SegmentList::slice_mut` (segment.rs:182).

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
use murk_arena::segment::Segment;

let mut seg = Segment::new(1024);
let (_, data) = seg.alloc(100).unwrap();
data[0] = 42.0;
seg.reset();
// BUG: succeeds despite cursor == 0 (no allocated region)
let stale = seg.slice(0, 100);
assert_eq!(stale[0], 42.0); // reads stale data from "freed" region
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-arena/src/segment.rs.md
**Verified lines:** segment.rs:57-60 (slice uses data[start..end] not cursor), segment.rs:68-71 (slice_mut same), segment.rs:79 (reset sets cursor=0), segment.rs:77 (reset does not zero memory), segment.rs:177,182 (SegmentList forwards to Segment)
**Root cause:** Bounds checking is performed against backing `Vec` capacity rather than the logical allocation boundary (`cursor`). The doc promises "allocated region" checking but the code checks capacity.
**Suggested fix:** Add `assert!(end <= self.cursor)` (or equivalent guard) in both `slice` and `slice_mut` to enforce the documented contract. Alternatively, if accessing beyond cursor is intentional (e.g., for zero-copy buffer reuse), update the documentation to reflect the actual behavior. Note: adding this check may surface other bugs (such as the placeholder handle issue) that currently read beyond cursor without triggering a panic.
