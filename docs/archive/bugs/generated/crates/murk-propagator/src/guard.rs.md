# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [x] murk-propagator
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

No concrete bug found in /home/john/murk/crates/murk-propagator/src/guard.rs.

## Steps to Reproduce

1. Inspect all write-tracking paths in `FullWriteGuard` (`write_at`, `as_mut_slice`, `mark_complete`, `Drop`).
2. Check for concrete failure modes from the Rust-specific risk list (overflow, unsafe misuse, FFI panic, zip truncation, off-by-one, atomic TOCTOU, leak paths).
3. Validate behavior against in-file contract comments and tests.

## Expected Behavior

`FullWriteGuard` should track coverage in debug builds, remain zero-overhead in release builds, and avoid UB/resource leaks.

## Actual Behavior

Observed behavior matches the stated contract; no concrete, demonstrable bug was identified in this file.

## Reproduction Rate

N/A (no bug found)

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

```text
N/A (no concrete bug found)
```

## Additional Context

Evidence reviewed at:
- `/home/john/murk/crates/murk-propagator/src/guard.rs:47` (`write_at` uses bounds-checked indexing).
- `/home/john/murk/crates/murk-propagator/src/guard.rs:57` and `/home/john/murk/crates/murk-propagator/src/guard.rs:61` (`as_mut_slice` explicitly documents and implements “mark all written” behavior).
- `/home/john/murk/crates/murk-propagator/src/guard.rs:77` and `/home/john/murk/crates/murk-propagator/src/guard.rs:83` (coverage computation is straightforward, no unchecked arithmetic).
- `/home/john/murk/crates/murk-propagator/src/guard.rs:103` and `/home/john/murk/crates/murk-propagator/src/guard.rs:109` (drop diagnostic path only logs on partial coverage; no unsafe/FFI boundary).