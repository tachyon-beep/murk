# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [ ] murk-core
- [x] murk-engine
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
- [x] RealtimeAsync
- [ ] Both / Unknown

## Summary

No concrete bug found in /home/john/murk/crates/murk-engine/src/egress.rs.

## Steps to Reproduce

1. N/A (static audit only).
2. N/A.
3. N/A.

## Expected Behavior

No concrete correctness/panic/UB/resource-leak bug in this file.

## Actual Behavior

No concrete bug was identified in this file.

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

```text
N/A - found via static analysis
```

## Minimal Reproducer

```text
N/A
```

## Additional Context

Audit evidence reviewed in `crates/murk-engine/src/egress.rs:89`, `crates/murk-engine/src/egress.rs:132`, `crates/murk-engine/src/egress.rs:157`, `crates/murk-engine/src/egress.rs:210`, and `crates/murk-engine/src/egress.rs:265` (plus related call/contract checks in `crates/murk-engine/src/realtime.rs:363` and `crates/murk-obs/src/plan.rs:761`). No concrete defect meeting the requested severity definitions was confirmed.