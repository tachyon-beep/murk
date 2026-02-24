# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [ ] Low (N/A - no concrete bug)

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
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

No concrete bug found in /home/john/murk/crates/murk-engine/src/overlay.rs.

## Steps to Reproduce

1. Inspect `crates/murk-engine/src/overlay.rs` for arithmetic, indexing, unsafe, FFI, and cache-staleness faults.
2. Trace call sites in `crates/murk-engine/src/tick.rs` to validate runtime behavior of `BaseFieldCache`, `StagedFieldCache`, and `OverlayReader`.
3. Attempt to construct a concrete failing path; none found from static analysis.

## Expected Behavior

Overlay cache population/clearing and read routing should return correct `Option<&[f32]>` values without panic/UB and preserve stale-vs-empty distinction.

## Actual Behavior

Implementation behavior matches expectation in reviewed paths; no demonstrable panic, UB, truncation, overflow, or incorrect routing found in this file.

## Reproduction Rate

N/A (no concrete bug identified)

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

Evidence reviewed in `overlay.rs` includes cache invalidation/population at `crates/murk-engine/src/overlay.rs:102`, `crates/murk-engine/src/overlay.rs:104`, `crates/murk-engine/src/overlay.rs:109`, staged cache clear/insert at `crates/murk-engine/src/overlay.rs:146`, `crates/murk-engine/src/overlay.rs:153`, and routing logic at `crates/murk-engine/src/overlay.rs:195`.  
Integration call path checked in `crates/murk-engine/src/tick.rs:238`, `crates/murk-engine/src/tick.rs:297`, `crates/murk-engine/src/tick.rs:302`, and `crates/murk-engine/src/tick.rs:311`.  
Residual risk: dynamic/runtime-only behavior (e.g., unusual propagator contracts) was not executed here due static-only audit.