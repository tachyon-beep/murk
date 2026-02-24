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
- [ ] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [x] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

No concrete bug found in /home/john/murk/crates/murk/src/lib.rs.

## Steps to Reproduce

1. Open `/home/john/murk/crates/murk/src/lib.rs`.
2. Review all executable items and module-level attributes for panic/UB/logic hazards.
3. Confirm file only contains crate docs, `pub use` re-exports, and `prelude` re-exports.

## Expected Behavior

No runtime bug should exist in this facade file; it should only re-export symbols and docs.

## Actual Behavior

Matched expectation. No concrete arithmetic, unsafe, FFI, panic, leak, indexing, zip-truncation, or atomic-consistency bug was found in this file.

## Reproduction Rate

Always (static result for this file at HEAD).

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
N/A - no concrete bug identified in this file
```

## Additional Context

Evidence points inspected:
- `/home/john/murk/crates/murk/src/lib.rs:68`
- `/home/john/murk/crates/murk/src/lib.rs:69`
- `/home/john/murk/crates/murk/src/lib.rs:70`
- `/home/john/murk/crates/murk/src/lib.rs:76`
- `/home/john/murk/crates/murk/src/lib.rs:83`
- `/home/john/murk/crates/murk/src/lib.rs:90`
- `/home/john/murk/crates/murk/src/lib.rs:96`
- `/home/john/murk/crates/murk/src/lib.rs:103`
- `/home/john/murk/crates/murk/src/lib.rs:109`
- `/home/john/murk/crates/murk/src/lib.rs:115`
- `/home/john/murk/crates/murk/src/lib.rs:121`
- `/home/john/murk/crates/murk/src/lib.rs:131`

Root-cause note: this file is a pure facade/re-export surface with `#![forbid(unsafe_code)]` and no executable logic paths beyond symbol exposure.