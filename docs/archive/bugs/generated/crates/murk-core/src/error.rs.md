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

No concrete bug found in /home/john/murk/crates/murk-core/src/error.rs.

## Steps to Reproduce

1. Open `/home/john/murk/crates/murk-core/src/error.rs`.
2. Inspect all enum variants and impl blocks (`Display`, `Error`) for panic/UB/overflow/FFI hazards.
3. Verify no unsafe blocks, no arithmetic, no extern C boundary, and no fallible unwrap/indexing in this file.

## Expected Behavior

No concrete, demonstrable runtime bug should be present in this file.

## Actual Behavior

No concrete, demonstrable runtime bug was identified in this file.

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
N/A - no concrete bug found to reproduce.
```

## Additional Context

Evidence inspected with exact locations:

- Error enum definitions: `/home/john/murk/crates/murk-core/src/error.rs:13`, `/home/john/murk/crates/murk-core/src/error.rs:68`, `/home/john/murk/crates/murk-core/src/error.rs:117`, `/home/john/murk/crates/murk-core/src/error.rs:154`
- `Display` impls: `/home/john/murk/crates/murk-core/src/error.rs:39`, `/home/john/murk/crates/murk-core/src/error.rs:89`, `/home/john/murk/crates/murk-core/src/error.rs:135`, `/home/john/murk/crates/murk-core/src/error.rs:190`
- `Error` impls: `/home/john/murk/crates/murk-core/src/error.rs:54`, `/home/john/murk/crates/murk-core/src/error.rs:110`, `/home/john/murk/crates/murk-core/src/error.rs:148`, `/home/john/murk/crates/murk-core/src/error.rs:204`

No unsafe code, arithmetic operations, iterator truncation patterns, FFI boundary code, raw pointer usage, or panic-prone unwrap/index operations are present in this file.