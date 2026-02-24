# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [x] murk-engine
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
- [x] RealtimeAsync
- [ ] Both / Unknown

## Summary

On 32-bit targets, `u64 -> usize` truncation before `% capacity` causes ring index discontinuity at `2^32`, leading to early eviction and false `None` for positions that should still be retained.

## Steps to Reproduce

1. Build for a 32-bit target (for example `i686-unknown-linux-gnu`).
2. In an internal test (same module), set `write_pos` near the boundary and push across it with a non-divisor capacity (for example `capacity = 3`, `write_pos = u32::MAX as u64 - 1`, then 3 pushes).
3. Call `get_by_pos(u32::MAX as u64)`.

## Expected Behavior

`get_by_pos` should return `Some(snapshot)` for any position where `current - pos <= capacity` (still retained).

## Actual Behavior

`get_by_pos` returns `None` for a still-retained position because the slot was overwritten early due to boundary-induced index remapping.

## Reproduction Rate

Always (once crossing the `2^32` boundary on 32-bit with affected capacity values).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

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
#[cfg(target_pointer_width = "32")]
#[test]
fn truncation_causes_early_eviction() {
    use std::sync::atomic::Ordering;

    let ring = SnapshotRing::new(3);
    ring.write_pos.store((u32::MAX as u64) - 1, Ordering::Relaxed); // 2^32 - 2

    // Writes positions: 2^32-2, 2^32-1, 2^32
    ring.push(make_test_snapshot(1));
    ring.push(make_test_snapshot(2));
    ring.push(make_test_snapshot(3));

    // current = 2^32 + 1, so (2^32-1) should still be retained (distance = 2 <= 3).
    assert!(ring.get_by_pos(u32::MAX as u64).is_some(), "unexpected early eviction");
}
```

## Additional Context

Evidence:
- `/home/john/murk/crates/murk-engine/src/ring.rs:75`
- `/home/john/murk/crates/murk-engine/src/ring.rs:128`
- `/home/john/murk/crates/murk-engine/src/ring.rs:189`
- `/home/john/murk/crates/murk-engine/src/ring.rs:205`

Root cause: slot index uses `(pos as usize) % self.capacity`. On 32-bit, this truncates high bits before modulo, so after `2^32` pushes mapping is no longer equivalent to `pos % capacity`, breaking retention invariants around the boundary.  
Suggested fix: compute modulo in `u64` first, then cast the bounded result to `usize` (for example `let idx = (pos % self.capacity as u64) as usize;`) everywhere index is derived from position.

---

# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [x] murk-engine
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
- [x] RealtimeAsync
- [ ] Both / Unknown

## Summary

`write_pos` increments with unchecked `pos + 1`; at `u64::MAX` it overflows, causing panic (overflow-check builds) or wrap-to-zero state corruption (release-style wrapping semantics).

## Steps to Reproduce

1. In an internal test, set `write_pos` to `u64::MAX`.
2. Call `push(...)` once.
3. Call `latest()` and/or `get_by_pos(u64::MAX)`.

## Expected Behavior

The ring should preserve monotonic position semantics and continue serving the latest snapshot.

## Actual Behavior

`push` overflows at increment:
- Overflow-check builds: panic at increment.
- Wrapping semantics: `write_pos` becomes `0`, so `latest()` treats ring as empty and retrieval semantics break.

## Reproduction Rate

Always (once `write_pos` reaches `u64::MAX`).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

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
#[test]
fn write_pos_overflow_breaks_ring() {
    use std::sync::atomic::Ordering;

    let ring = SnapshotRing::new(4);
    ring.write_pos.store(u64::MAX, Ordering::Relaxed);

    // overflow-check builds panic here; wrapping behavior sets write_pos to 0
    let _ = ring.push(make_test_snapshot(1));

    // In wrapping case, ring is non-empty but latest() reports None.
    assert!(ring.latest().is_some(), "ring lost monotonic state after overflow");
}
```

## Additional Context

Evidence:
- `/home/john/murk/crates/murk-engine/src/ring.rs:90`
- `/home/john/murk/crates/murk-engine/src/ring.rs:121`
- `/home/john/murk/crates/murk-engine/src/ring.rs:127`
- `/home/john/murk/crates/murk-engine/src/ring.rs:228`

Root cause: `self.write_pos.store(pos + 1, Ordering::Release)` has no `checked_add`/saturation/error path.  
Suggested fix: use `checked_add(1)` with explicit failure handling (for example return error, panic with clear message, or reset protocol that preserves invariants).