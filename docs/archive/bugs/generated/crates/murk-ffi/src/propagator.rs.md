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

No concrete bug found in /home/john/murk/crates/murk-ffi/src/propagator.rs.

## Steps to Reproduce

1. Read `crates/murk-ffi/src/propagator.rs` end-to-end, focusing on all `unsafe` blocks and all `extern "C"` functions.
2. Verify pointer/null/length validation paths in `murk_propagator_create` and all trampolines.
3. Verify panic guarding at FFI boundary (`ffi_guard!` usage) and check for `unwrap`, unchecked arithmetic, zip truncation, off-by-one, and raw-pointer lifetime misuse.

## Expected Behavior

No concrete, demonstrable production bug in this file.

## Actual Behavior

No concrete, demonstrable production bug identified by static analysis.

## Reproduction Rate

N/A

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
N/A - no concrete bug found.
```

## Additional Context

Evidence checked in target file:

- `crates/murk-ffi/src/propagator.rs:244` (`ffi_guard!` wraps `murk_propagator_create`, protecting against unwind across FFI boundary).
- `crates/murk-ffi/src/propagator.rs:245` and `crates/murk-ffi/src/propagator.rs:246` (null checks for `def` and `out_handle`).
- `crates/murk-ffi/src/propagator.rs:268`, `crates/murk-ffi/src/propagator.rs:279`, `crates/murk-ffi/src/propagator.rs:291` (array pointer checks before `from_raw_parts`).
- `crates/murk-ffi/src/propagator.rs:175`, `crates/murk-ffi/src/propagator.rs:197`, `crates/murk-ffi/src/propagator.rs:219` (trampoline argument null checks before dereference/writeback).
- `crates/murk-ffi/src/propagator.rs:296` to `crates/murk-ffi/src/propagator.rs:299` (write mode discriminator validation prevents invalid enum interpretation).