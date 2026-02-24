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
- [x] murk-obs
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

No concrete bug found in /home/john/murk/crates/murk-obs/src/lib.rs.

## Steps to Reproduce

1. Inspect `/home/john/murk/crates/murk-obs/src/lib.rs:1`.
2. Verify the file only contains crate attributes, module declarations, and re-exports (`/home/john/murk/crates/murk-obs/src/lib.rs:21`, `/home/john/murk/crates/murk-obs/src/lib.rs:25`, `/home/john/murk/crates/murk-obs/src/lib.rs:33`).
3. Confirm no executable logic exists in this file (no arithmetic/indexing/unsafe/FFI code paths).

## Expected Behavior

No runtime-affecting bugs should be present in a module-wiring-only crate root file.

## Actual Behavior

Matched expectation; no concrete, demonstrable bug found in this file.

## Reproduction Rate

Always

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
N/A - no concrete bug identified in /home/john/murk/crates/murk-obs/src/lib.rs.
```

## Additional Context

Evidence reviewed:
- `/home/john/murk/crates/murk-obs/src/lib.rs:21-23` (`deny`/`forbid` attributes only)
- `/home/john/murk/crates/murk-obs/src/lib.rs:25-31` (`pub mod ...` declarations only)
- `/home/john/murk/crates/murk-obs/src/lib.rs:33-36` (`pub use ...` re-exports only)

No arithmetic, unsafe blocks, raw pointers, `extern "C"` functions, indexing logic, or allocator/generation logic exists in this file, so none of the targeted concrete bug classes can be demonstrated here.