# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [ ] Low (N/A: no bug found)

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

No concrete bug found in /home/john/murk/crates/murk-ffi/src/status.rs.

## Steps to Reproduce

1. N/A (no concrete bug identified).
2. N/A.
3. N/A.

## Expected Behavior

No incorrect behavior should be present in status-code definitions/conversions.

## Actual Behavior

No concrete incorrect behavior was found.

## Reproduction Rate

N/A

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

```
N/A - found via static analysis
```

## Minimal Reproducer

```text
N/A - no concrete bug found
```

## Additional Context

Static audit evidence inspected:
- Enum definitions and ABI-discriminant assignments: `/home/john/murk/crates/murk-ffi/src/status.rs:16`
- Error-to-status mappings (`StepError`, `TickError`, `ObsError`, `ConfigError`, `IngressError`): `/home/john/murk/crates/murk-ffi/src/status.rs:65`
- Tests covering status values and mappings: `/home/john/murk/crates/murk-ffi/src/status.rs:117`

No concrete bug (panic/UB path, unsafe misuse, overflow, truncation, off-by-one, or resource-leak pattern) is present in this file.