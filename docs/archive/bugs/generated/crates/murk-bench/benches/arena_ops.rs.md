# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [ ] murk-ffi
- [ ] murk-python
- [x] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

No concrete bug found in /home/john/murk/crates/murk-bench/benches/arena_ops.rs.

## Steps to Reproduce

1. Inspect `/home/john/murk/crates/murk-bench/benches/arena_ops.rs` for panic/UB/overflow/FFI/atomic/indexing hazards.
2. Cross-check arena lifecycle assumptions against `/home/john/murk/crates/murk-arena/src/pingpong.rs`.
3. Cross-check sparse write length behavior against `/home/john/murk/crates/murk-arena/src/write.rs`.

## Expected Behavior

No deterministic panic, UB, or incorrect state transition should be present in this benchmark file.

## Actual Behavior

No concrete, reproducible bug was identified in this file from static analysis.

## Reproduction Rate

N/A

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

```text
N/A - found via static analysis
```

## Minimal Reproducer

```text
N/A
```

## Additional Context

Evidence reviewed with exact locations:
- `/home/john/murk/crates/murk-bench/benches/arena_ops.rs:137`
- `/home/john/murk/crates/murk-bench/benches/arena_ops.rs:146`
- `/home/john/murk/crates/murk-bench/benches/arena_ops.rs:223`
- `/home/john/murk/crates/murk-arena/src/pingpong.rs:221`
- `/home/john/murk/crates/murk-arena/src/pingpong.rs:333`
- `/home/john/murk/crates/murk-arena/src/write.rs:164`