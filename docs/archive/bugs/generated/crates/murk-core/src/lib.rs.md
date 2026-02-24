# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

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

No concrete bug found in /home/john/murk/crates/murk-core/src/lib.rs.

## Steps to Reproduce

1. Open `/home/john/murk/crates/murk-core/src/lib.rs`.
2. Inspect all executable and declaration lines for concrete failure modes (overflow, unsafe, FFI panic/unwrap, resource ownership bugs, truncating zip, indexing errors, atomic TOCTOU).
3. Verify whether any runtime behavior in this file can trigger a demonstrable bug.

## Expected Behavior

The file should only expose safe module wiring and re-exports without unsafe behavior or runtime bug surface.

## Actual Behavior

The file contains crate attributes, module declarations, and public re-exports only; no concrete bug-triggering logic was found.

## Reproduction Rate

Not applicable (no concrete bug identified).

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

N/A (no concrete bug found).

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// N/A - no concrete bug found in /home/john/murk/crates/murk-core/src/lib.rs
```

## Additional Context

Evidence inspected with exact locations:
- `/home/john/murk/crates/murk-core/src/lib.rs:6`
- `/home/john/murk/crates/murk-core/src/lib.rs:7`
- `/home/john/murk/crates/murk-core/src/lib.rs:8`
- `/home/john/murk/crates/murk-core/src/lib.rs:10`
- `/home/john/murk/crates/murk-core/src/lib.rs:16`
- `/home/john/murk/crates/murk-core/src/lib.rs:21`