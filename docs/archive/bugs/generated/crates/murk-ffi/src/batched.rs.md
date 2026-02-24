# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [ ] Low *(N/A: no bug found)*

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

No concrete bug found in /home/john/murk/crates/murk-ffi/src/batched.rs.

## Steps to Reproduce

1. N/A (static-analysis audit only).
2. Reviewed all exported `extern "C"` paths in `crates/murk-ffi/src/batched.rs`.
3. Verified panic-guard coverage, unsafe pointer checks, and length handling.

## Expected Behavior

FFI entrypoints should not panic across FFI, should validate raw pointers before dereference, and should reject invalid lengths/arguments via status codes.

## Actual Behavior

Matched expected behavior in the audited file; no concrete violation found.

## Reproduction Rate

N/A (no bug found)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** Any

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

Evidence reviewed in `crates/murk-ffi/src/batched.rs`:
- Panic guards (`ffi_guard!` / `ffi_guard_or!`) around exported paths: lines `40`, `153`, `211`, `251`, `278`, `310`, `326`, `345`, `365`, `384`.
- Null/argument checks before unsafe slice creation and pointer use:
  - `murk_batched_create`: lines `41-110`
  - `murk_batched_step_and_observe`: lines `177-187`
  - `murk_batched_observe_all`: lines `229-235`
  - `murk_batched_reset_all`: lines `290-298`
  - `convert_batch_commands`: lines `407-424`
- No `unwrap`/`expect` in exported `extern "C"` functions; no concrete unchecked overflow or demonstrable UB pattern identified in this file.