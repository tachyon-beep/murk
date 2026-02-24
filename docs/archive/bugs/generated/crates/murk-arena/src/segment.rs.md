# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

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

`SegmentList::new` violates its own `max_segments` bound when `max_segments == 0`, allowing allocations to succeed despite a zero-segment budget.

## Steps to Reproduce

1. Create a list with zero segment budget: `let mut list = SegmentList::new(8, 0);`
2. Observe `list.segment_count()` is `1`.
3. Call `list.alloc(1)` and observe it returns `Ok((0, 0))`.

## Expected Behavior

A zero-segment budget should either be rejected at construction or make allocation impossible (consistent with `max_segments` semantics).

## Actual Behavior

`SegmentList::new` always pushes one segment (`segments.push(Segment::new(segment_size))`), even when `max_segments == 0`, and `alloc` can use that segment successfully before any capacity check for new segments.

Evidence:
- Unconditional initial segment creation: `/home/john/murk/crates/murk-arena/src/segment.rs:122` and `/home/john/murk/crates/murk-arena/src/segment.rs:123`
- First allocation served from current segment: `/home/john/murk/crates/murk-arena/src/segment.rs:146` and `/home/john/murk/crates/murk-arena/src/segment.rs:147`
- `max_segments` check only applies when adding a new segment: `/home/john/murk/crates/murk-arena/src/segment.rs:161`

## Reproduction Rate

Always

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):**
- **C compiler (if murk-ffi C header/source):**

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```text
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_arena::segment::SegmentList;

fn main() {
    let mut list = SegmentList::new(8, 0);

    // Violates max_segments=0 expectation.
    assert_eq!(list.segment_count(), 1);

    // Also succeeds unexpectedly.
    let got = list.alloc(1);
    assert!(got.is_ok(), "alloc should not succeed when max_segments is 0");
}
```

## Additional Context

Root cause is an invariant gap: constructor does not enforce `max_segments >= 1` but assumes one initial segment exists. Suggested fix: enforce `max_segments >= 1` (constructor validation/panic or fallible `new`) so `segments.len() <= max_segments` holds immediately after construction.