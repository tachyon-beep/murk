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

No concrete bug found in /home/john/murk/crates/murk-ffi/src/handle.rs.

## Steps to Reproduce

1. N/A
2. N/A
3. N/A

## Expected Behavior

No bug-triggering behavior identified in this file under normal operation.

## Actual Behavior

No concrete failure identified via static analysis.

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
N/A
```

## Additional Context

Evidence reviewed in `/home/john/murk/crates/murk-ffi/src/handle.rs`:
- Handle decode + bounds-safe lookup paths at lines `60-67` and `72-79`.
- Stale-handle rejection and generation checks at lines `63-65`, `75-77`, `91-93`.
- Generation wrap handling with slot retirement at lines `95-102`.
- Unit tests covering stale/double remove, slot reuse, and generation exhaustion at lines `135-230`.