# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [x] murk-arena
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

No concrete bug found in /home/john/murk/crates/murk-arena/src/lib.rs.

## Steps to Reproduce

1. Inspect `/home/john/murk/crates/murk-arena/src/lib.rs:1`.
2. Verify that contents are crate docs, lint attributes, module declarations, and re-exports only (`/home/john/murk/crates/murk-arena/src/lib.rs:33`, `/home/john/murk/crates/murk-arena/src/lib.rs:37`, `/home/john/murk/crates/murk-arena/src/lib.rs:50`).
3. Confirm there are no functions/unsafe blocks/arithmetic/indexing/FFI entrypoints in this file (`/home/john/murk/crates/murk-arena/src/lib.rs:56` is end of file).

## Expected Behavior

No runtime bug should be present in this file if it only defines module wiring and exports.

## Actual Behavior

Matches expectation; no concrete, demonstrable bug found in this file.

## Reproduction Rate

Always (no bug observed).

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

```text
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// N/A: no concrete bug found in /home/john/murk/crates/murk-arena/src/lib.rs.
```

## Additional Context

Evidence points inspected:
- `/home/john/murk/crates/murk-arena/src/lib.rs:33` (`#![deny(missing_docs)]`)
- `/home/john/murk/crates/murk-arena/src/lib.rs:35` (`#![deny(unsafe_code)]`)
- `/home/john/murk/crates/murk-arena/src/lib.rs:37` (module declarations begin)
- `/home/john/murk/crates/murk-arena/src/lib.rs:48` (module declarations end)
- `/home/john/murk/crates/murk-arena/src/lib.rs:50` (public re-exports begin)
- `/home/john/murk/crates/murk-arena/src/lib.rs:56` (file end)

No executable logic is present in this file, so the targeted concrete bug classes are not instantiated here.