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

No concrete bug found in /home/john/murk/crates/murk-propagator/src/propagator.rs.

## Steps to Reproduce

1. Inspect `/home/john/murk/crates/murk-propagator/src/propagator.rs:1`.
2. Review all executable/default code paths in `/home/john/murk/crates/murk-propagator/src/propagator.rs:87`, `/home/john/murk/crates/murk-propagator/src/propagator.rs:104`, and `/home/john/murk/crates/murk-propagator/src/propagator.rs:117`.
3. Check for concrete panic/UB/incorrect-result paths (unsafe, FFI, overflow, truncation, atomics, ownership/resource leaks) and confirm none are present in this file.

## Expected Behavior

No concrete runtime bug should be present in this file.

## Actual Behavior

No concrete runtime bug was identified in this file.

## Reproduction Rate

N/A (no bug found)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

## Determinism Impact

- [ ] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// N/A - no concrete bug found in this file.
```

## Additional Context

`/home/john/murk/crates/murk-propagator/src/propagator.rs` is limited to `WriteMode` and the `Propagator` trait/default methods; no unsafe blocks, no `extern "C"` boundary, no arithmetic that can overflow, no raw-pointer ownership paths, and no atomic coordination logic appear in this unit.