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

No concrete bug found in /home/john/murk/crates/murk-engine/src/lib.rs.

## Steps to Reproduce

1. Open `/home/john/murk/crates/murk-engine/src/lib.rs`.
2. Inspect lines `1-36`.
3. Confirm file only contains crate docs, lint settings, `pub mod` declarations, and `pub use` re-exports.

## Expected Behavior

Concrete bug(s) would be present in target file if implementation logic existed there.

## Actual Behavior

No executable logic exists in the target file, so no concrete overflow/UB/panic/race/leak bug is demonstrable in this file itself.

## Reproduction Rate

Always

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):**
- **C compiler (if murk-ffi C header/source):**

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
// N/A: no concrete bug in /home/john/murk/crates/murk-engine/src/lib.rs
```

## Additional Context

Evidence: `/home/john/murk/crates/murk-engine/src/lib.rs:1-36` contains only declarations/re-exports and no runtime logic (no arithmetic, unsafe blocks, extern "C" functions, indexing, atomics, or allocation/reclamation code paths).