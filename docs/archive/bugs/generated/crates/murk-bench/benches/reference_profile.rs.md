# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

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

- [x] Lockstep
- [ ] RealtimeAsync
- [ ] Both / Unknown

## Summary

No concrete bug found in /home/john/murk/crates/murk-bench/benches/reference_profile.rs.

## Steps to Reproduce

1. Inspect `/home/john/murk/crates/murk-bench/benches/reference_profile.rs`.
2. Check all executable paths in benchmark fns (`bench_tick_10k`, `bench_tick_100k`, `bench_1000_ticks_10k`).
3. Verify no concrete arithmetic/unsafe/FFI/iterator/atomic/resource-lifetime bug is present in this file.

## Expected Behavior

No concrete defect should be identifiable in this benchmark harness file itself.

## Actual Behavior

Static analysis found no concrete, demonstrable bug in this file.

## Reproduction Rate

Always

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
N/A - no concrete bug identified in the target file.
```

## Additional Context

Evidence reviewed with line references: `/home/john/murk/crates/murk-bench/benches/reference_profile.rs:8`, `/home/john/murk/crates/murk-bench/benches/reference_profile.rs:24`, `/home/john/murk/crates/murk-bench/benches/reference_profile.rs:39`, `/home/john/murk/crates/murk-bench/benches/reference_profile.rs:11`, `/home/john/murk/crates/murk-bench/benches/reference_profile.rs:14`, `/home/john/murk/crates/murk-bench/benches/reference_profile.rs:18`, `/home/john/murk/crates/murk-bench/benches/reference_profile.rs:27`, `/home/john/murk/crates/murk-bench/benches/reference_profile.rs:29`, `/home/john/murk/crates/murk-bench/benches/reference_profile.rs:33`, `/home/john/murk/crates/murk-bench/benches/reference_profile.rs:44`, `/home/john/murk/crates/murk-bench/benches/reference_profile.rs:46`, `/home/john/murk/crates/murk-bench/benches/reference_profile.rs:53`.  
No unsafe blocks, no FFI boundary crossings, no arithmetic prone to overflow, no `zip` truncation pattern, no raw-pointer/resource ownership hazards, and no atomic TOCTOU pattern exist in this file.