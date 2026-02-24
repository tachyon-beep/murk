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

No concrete bug found in /home/john/murk/crates/murk-bench/benches/obs_ops.rs.

## Steps to Reproduce

1. Inspect `/home/john/murk/crates/murk-bench/benches/obs_ops.rs` for overflow, unsafe/FFI misuse, truncation, indexing, atomic, and resource-lifetime issues.
2. Cross-check assumptions against `/home/john/murk/crates/murk-bench/src/lib.rs:27-33` (100x100 reference profile) and observation execution contracts in `/home/john/murk/crates/murk-obs/src/plan.rs:671-769`.
3. Verify buffer sizing and call patterns in `/home/john/murk/crates/murk-bench/benches/obs_ops.rs:132-133`, `/home/john/murk/crates/murk-bench/benches/obs_ops.rs:166-169`, `/home/john/murk/crates/murk-bench/benches/obs_ops.rs:179-192`.

## Expected Behavior

No panic/UB/incorrectness-triggering bug should be present in the benchmark harness.

## Actual Behavior

Static analysis found no concrete, demonstrable bug in the target file.

## Reproduction Rate

Always (no bug reproduced/found)

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

```text
N/A - no concrete bug identified in /home/john/murk/crates/murk-bench/benches/obs_ops.rs.
```

## Additional Context

Evidence reviewed in target file includes compile/execute paths at `/home/john/murk/crates/murk-bench/benches/obs_ops.rs:76-80`, `/home/john/murk/crates/murk-bench/benches/obs_ops.rs:107-110`, `/home/john/murk/crates/murk-bench/benches/obs_ops.rs:141-143`, and `/home/john/murk/crates/murk-bench/benches/obs_ops.rs:179-192`; none demonstrated a concrete correctness or safety defect.