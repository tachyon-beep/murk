# Bug Report

**Date:** 2026-02-23
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

No concrete bug found in /home/john/murk/crates/murk-engine/examples/replay.rs.

## Steps to Reproduce

1. Inspect `/home/john/murk/crates/murk-engine/examples/replay.rs` for panic/UB/overflow/indexing/race patterns.
2. Validate field access and indexing in `DiffusionPropagator::step`.
3. Validate replay/write/read/verify loops for truncation, mismatched progression, and unchecked assumptions.

## Expected Behavior

No concrete runtime bug should be present in this example under its defined constants/configuration.

## Actual Behavior

No concrete, demonstrable bug was identified by static analysis.

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

```text
N/A - found via static analysis
```

## Minimal Reproducer

```text
N/A - no concrete bug found
```

## Additional Context

Evidence reviewed with exact line references:

- Boundary-safe neighbor indexing in diffusion step at `/home/john/murk/crates/murk-engine/examples/replay.rs:80`, `/home/john/murk/crates/murk-engine/examples/replay.rs:89`, `/home/john/murk/crates/murk-engine/examples/replay.rs:94`, `/home/john/murk/crates/murk-engine/examples/replay.rs:99`, `/home/john/murk/crates/murk-engine/examples/replay.rs:104`, `/home/john/murk/crates/murk-engine/examples/replay.rs:112`.
- Read/write access is guarded with error returns (no unwrap panic path there) at `/home/john/murk/crates/murk-engine/examples/replay.rs:65`, `/home/john/murk/crates/murk-engine/examples/replay.rs:75`.
- Replay recording and verification loops advance one simulation step per frame at `/home/john/murk/crates/murk-engine/examples/replay.rs:192`, `/home/john/murk/crates/murk-engine/examples/replay.rs:239`, `/home/john/murk/crates/murk-engine/examples/replay.rs:277`.
