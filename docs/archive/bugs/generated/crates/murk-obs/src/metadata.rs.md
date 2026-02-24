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

No concrete bug found in /home/john/murk/crates/murk-obs/src/metadata.rs.

## Steps to Reproduce

1. Inspect `/home/john/murk/crates/murk-obs/src/metadata.rs:1`.
2. Review all declarations through `/home/john/murk/crates/murk-obs/src/metadata.rs:28`.
3. Verify there is no executable logic (no arithmetic, unsafe, FFI, indexing, atomics, or allocation lifecycle paths).

## Expected Behavior

No concrete runtime bug should exist if the file only contains a metadata struct definition.

## Actual Behavior

Matched expectation: file contains struct field declarations only; no demonstrable bug path identified.

## Reproduction Rate

Always (audit consistently finds no concrete bug in this file).

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
// N/A - no concrete bug found in target file.
```

## Additional Context

Static analysis evidence was limited to `/home/john/murk/crates/murk-obs/src/metadata.rs:1` and `/home/john/murk/crates/murk-obs/src/metadata.rs:28`; the file content is declarative metadata structure only, with no concrete failure mechanism to exercise.