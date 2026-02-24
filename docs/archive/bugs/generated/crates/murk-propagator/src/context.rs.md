# Bug Report

**Date:** 2026-02-23
**Reporter:** static-analysis-agent
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [ ] Low

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
- [ ] Both / Unknown

## Summary

No concrete bug found in /home/john/murk/crates/murk-propagator/src/context.rs.

## Steps to Reproduce

1. Inspect `/home/john/murk/crates/murk-propagator/src/context.rs:27`-`/home/john/murk/crates/murk-propagator/src/context.rs:100` for runtime hazards (overflow, unsafe, indexing, FFI, panics).
2. Inspect `/home/john/murk/crates/murk-propagator/src/context.rs:111`-`/home/john/murk/crates/murk-propagator/src/context.rs:195` for test-covered behavior.
3. Trace `StepContext::new(...)` engine call path at `/home/john/murk/crates/murk-engine/src/tick.rs:331`-`/home/john/murk/crates/murk-engine/src/tick.rs:339` and config validation at `/home/john/murk/crates/murk-engine/src/config.rs:302`-`/home/john/murk/crates/murk-engine/src/config.rs:307`, plus invalid-`dt` test at `/home/john/murk/crates/murk-engine/src/config.rs:388`-`/home/john/murk/crates/murk-engine/src/config.rs:396`.

## Expected Behavior

No concrete correctness or safety bug in `StepContext` wrapper/accessor logic.

## Actual Behavior

Static analysis found no concrete, demonstrable bug in the target file.

## Reproduction Rate

N/A (no bug identified)

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

Reviewed all executable logic in `/home/john/murk/crates/murk-propagator/src/context.rs` and found it to be a straightforward field/reference carrier with accessor methods only; no unsafe blocks, unchecked arithmetic, indexing, FFI boundary, or panic-prone unwraps were present in this file.