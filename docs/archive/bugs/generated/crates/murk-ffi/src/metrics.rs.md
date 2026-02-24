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

No concrete bug found in /home/john/murk/crates/murk-ffi/src/metrics.rs.

## Steps to Reproduce

1. N/A
2. N/A
3. N/A

## Expected Behavior

N/A

## Actual Behavior

N/A

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

```txt
N/A - found via static analysis
```

## Minimal Reproducer

```txt
N/A
```

## Additional Context

Reviewed `extern "C"` paths and unsafe pointer writes in `crates/murk-ffi/src/metrics.rs:114` and `crates/murk-ffi/src/metrics.rs:157`, including bounds/null checks and panic-guarding via `ffi_guard!`; no concrete, demonstrable defect was identified in this file.