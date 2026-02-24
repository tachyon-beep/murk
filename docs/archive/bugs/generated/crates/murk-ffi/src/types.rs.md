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
- [ ] murk-obs
- [ ] murk-replay
- [x] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

No concrete bug found in /home/john/murk/crates/murk-ffi/src/types.rs.

## Steps to Reproduce

1. Inspect `/home/john/murk/crates/murk-ffi/src/types.rs:1` through `/home/john/murk/crates/murk-ffi/src/types.rs:81`.
2. Trace FFI call sites using these types (notably `/home/john/murk/crates/murk-ffi/src/config.rs:218` through `/home/john/murk/crates/murk-ffi/src/config.rs:314`).
3. Verify whether `extern "C"` functions take Rust enums directly (UB risk) or accept primitives and convert internally.

## Expected Behavior

FFI boundary should avoid UB-prone direct Rust enum ABI exposure and use safe conversion/validation patterns.

## Actual Behavior

No concrete bug identified in the target file; enums are defined as C-like `#[repr(i32)]` types, and FFI entry points examined use primitive parameters rather than direct enum parameters.

## Reproduction Rate

N/A (no defect reproduced)

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
N/A - no concrete bug found in the target file.
```

## Additional Context

Evidence reviewed:
- Enum definitions only: `/home/john/murk/crates/murk-ffi/src/types.rs:1-81`
- FFI config API call surface uses primitive args and internal mapping: `/home/john/murk/crates/murk-ffi/src/config.rs:218-314`