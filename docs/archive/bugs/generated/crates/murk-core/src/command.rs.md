# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [ ] Low

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

No concrete bug found in /home/john/murk/crates/murk-core/src/command.rs.

## Steps to Reproduce

1. Inspect `/home/john/murk/crates/murk-core/src/command.rs` end-to-end.
2. Check for concrete failure patterns (overflow, unsafe/FFI UB, truncation, indexing, atomic TOCTOU, leaks, counter wrap risks).
3. Cross-check referenced usage points to confirm no demonstrable bug rooted in this file.

## Expected Behavior

No correctness, safety, or runtime bugs should be present in `command.rs` definitions/docs.

## Actual Behavior

No concrete, reproducible bug was identified in this file.

## Reproduction Rate

N/A (no bug found)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):**
- **C compiler (if murk-ffi C header/source):**

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
N/A - no concrete bug found in target file.
```

## Additional Context

Evidence reviewed in target file:
- `/home/john/murk/crates/murk-core/src/command.rs:1`
- `/home/john/murk/crates/murk-core/src/command.rs:33`
- `/home/john/murk/crates/murk-core/src/command.rs:77`
- `/home/john/murk/crates/murk-core/src/command.rs:153`

The file is type/data-model definitions plus docs only; no unsafe blocks, no arithmetic paths, no FFI entrypoints, no iterator truncation logic, and no indexing logic in this file.