# Bug Report

**Date:** February 23, 2026  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [ ] Low

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

No concrete bug found in /home/john/murk/crates/murk-arena/src/handle.rs.

## Steps to Reproduce

1. Open `/home/john/murk/crates/murk-arena/src/handle.rs`.
2. Trace each `FieldHandle`/`FieldLocation` definition and constructor path.
3. Cross-check all callsites in `murk-arena` for arithmetic/unsafe/FFI/overflow misuse.

## Expected Behavior

`FieldHandle`/`FieldLocation` should remain a plain, copyable descriptor with no panic/UB/overflow behavior in this file.

## Actual Behavior

Matched expected behavior; no concrete runtime bug identified in this file.

## Reproduction Rate

N/A (no bug found)

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
N/A - no concrete bug found.
```

## Additional Context

Evidence lines reviewed in target file:
- `/home/john/murk/crates/murk-arena/src/handle.rs:15`
- `/home/john/murk/crates/murk-arena/src/handle.rs:28`
- `/home/john/murk/crates/murk-arena/src/handle.rs:38`
- `/home/john/murk/crates/murk-arena/src/handle.rs:43`
- `/home/john/murk/crates/murk-arena/src/handle.rs:53`
- `/home/john/murk/crates/murk-arena/src/handle.rs:59`
- `/home/john/murk/crates/murk-arena/src/handle.rs:73`
- `/home/john/murk/crates/murk-arena/src/handle.rs:85`

Cross-checked callsites where `offset/len/location/generation` are consumed:
- `/home/john/murk/crates/murk-arena/src/read.rs:74`
- `/home/john/murk/crates/murk-arena/src/read.rs:171`
- `/home/john/murk/crates/murk-arena/src/write.rs:127`
- `/home/john/murk/crates/murk-arena/src/write.rs:157`
- `/home/john/murk/crates/murk-arena/src/sparse.rs:159`
- `/home/john/murk/crates/murk-arena/src/pingpong.rs:158`