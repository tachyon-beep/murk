# Bug Report

**Date:** February 23, 2026  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

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
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

No concrete bug found in /home/john/murk/crates/murk-engine/src/metrics.rs.

## Steps to Reproduce

1. Review `/home/john/murk/crates/murk-engine/src/metrics.rs` for executable logic and state transitions.
2. Check Rust-specific risk patterns (overflow, unsafe usage, FFI boundaries, atomics consistency, iterator truncation, indexing).
3. No concrete failure path is present in this file.

## Expected Behavior

No panic/UB/incorrect-result bug should be present in metrics container definitions and default/test behavior.

## Actual Behavior

No concrete bug behavior identified. The file contains metrics data structure fields and tests; no unsafe blocks, FFI surface, atomics, or arithmetic paths that demonstrate a defect in this file.

## Reproduction Rate

Always (no bug reproduced).

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
// N/A: no concrete bug found in this file.
```

## Additional Context

Evidence reviewed in `/home/john/murk/crates/murk-engine/src/metrics.rs:8` (metrics struct field declarations) and `/home/john/murk/crates/murk-engine/src/metrics.rs:48` (tests/default assertions). No demonstrable overflow, unsafe misuse, FFI panic path, atomic TOCTOU pattern, iterator truncation bug, or indexing bug exists in this file.