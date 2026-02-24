# Bug Report

**Date:** 2026-02-23
**Reporter:** static-analysis-agent
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

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

`SpaceInstanceId::next` can wrap its `u64` atomic counter and eventually reuse prior IDs, violating its own uniqueness contract in `/home/john/murk/crates/murk-core/src/id.rs`.

## Steps to Reproduce

1. In a test inside `id.rs` (same module visibility), call `SpaceInstanceId::next()` once to allocate `1`.
2. Force `SPACE_INSTANCE_COUNTER.store(u64::MAX, Ordering::Relaxed)`.
3. Call `SpaceInstanceId::next()` three times; observe returned IDs include `u64::MAX`, `0`, then `1` (duplicate of step 1).

## Expected Behavior

`SpaceInstanceId::next()` should never return an ID that has already been returned within the process lifetime.

## Actual Behavior

`SPACE_INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed)` at `/home/john/murk/crates/murk-core/src/id.rs:65` wraps from `u64::MAX` to `0`, and subsequent allocations eventually repeat old IDs (e.g., `1`), contradicting docs at `/home/john/murk/crates/murk-core/src/id.rs:61-62`.

## Reproduction Rate

Always (deterministic once counter reaches wraparound state)

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

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
#[test]
fn space_instance_id_wraps_and_reuses_ids() {
    use std::sync::atomic::Ordering;

    // First issued ID from normal startup path is 1.
    let first = SpaceInstanceId::next().to_string();

    // Simulate near-exhausted process lifetime.
    SPACE_INSTANCE_COUNTER.store(u64::MAX, Ordering::Relaxed);

    let _max = SpaceInstanceId::next(); // returns u64::MAX
    let _zero = SpaceInstanceId::next(); // returns 0
    let wrapped = SpaceInstanceId::next().to_string(); // returns 1 again

    assert_eq!(first, wrapped); // duplicate ID
}
```

## Additional Context

Root cause is unchecked overflow/wraparound in `/home/john/murk/crates/murk-core/src/id.rs:65` with a global counter initialized at `/home/john/murk/crates/murk-core/src/id.rs:43`. Suggested fix: use `fetch_update`/CAS to detect `u64::MAX` and fail fast (panic/error), or reserve a terminal behavior (saturate/abort) so previously-issued IDs are never reused.