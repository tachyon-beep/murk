# Bug Report

**Date:** 2026-02-23
**Reporter:** static-analysis-agent
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [x] murk-propagator
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

No concrete bug found in /home/john/murk/crates/murk-propagator/src/lib.rs.

## Steps to Reproduce

1. Open `/home/john/murk/crates/murk-propagator/src/lib.rs`.
2. Review all lines for executable logic and Rust-specific bug classes (overflow, unsafe, FFI panic paths, indexing, atomics, leaks).
3. Confirm file contains only crate attributes, module declarations, and re-exports.

## Expected Behavior

The crate root should safely expose modules/types without introducing runtime bugs.

## Actual Behavior

Matches expected behavior in this file; no concrete defect identified.

## Reproduction Rate

N/A (no bug identified)

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

```text
N/A - found via static analysis
```

## Minimal Reproducer

```text
N/A - no concrete bug found in this file
```

## Additional Context

Evidence reviewed with line references:
- `/home/john/murk/crates/murk-propagator/src/lib.rs:19` (`#![deny(missing_docs)]`)
- `/home/john/murk/crates/murk-propagator/src/lib.rs:20` (`#![deny(rustdoc::broken_intra_doc_links)]`)
- `/home/john/murk/crates/murk-propagator/src/lib.rs:21` (`#![forbid(unsafe_code)]`)
- `/home/john/murk/crates/murk-propagator/src/lib.rs:23`
- `/home/john/murk/crates/murk-propagator/src/lib.rs:35`

No unsafe blocks, FFI entry points, arithmetic, iterator zips, indexing logic, or atomic coordination are present in this target file.