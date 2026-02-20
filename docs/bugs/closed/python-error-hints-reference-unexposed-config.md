# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

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
- [x] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

Recovery hints in error messages reference configuration knobs that are not exposed in the Python API:

- Error -4: "Increase ring_buffer_size in your config" (line 64)
- Error -6: "increase config.set_max_ingress_queue()" (line 79)
- Error -11: "increase ring_buffer_size" (line 117)
- Error -14: "increase max_epoch_hold_ms in your AsyncConfig" (line 140)

None of `ring_buffer_size`, `set_max_ingress_queue()`, `AsyncConfig`, or `max_epoch_hold_ms` are accessible from the Python `Config` class. Users who encounter these errors cannot follow the suggested remediation.

## Steps to Reproduce

1. Trigger any of the above error codes from Python.
2. Read the error hint.
3. Attempt to follow the suggested fix.
4. Discover that the referenced API does not exist in the Python bindings.

## Expected Behavior

Error hints should only reference actions available through the Python API, or the missing config setters should be exposed.

## Actual Behavior

Hints reference internal engine config knobs that have no Python-side equivalent.

## Reproduction Rate

- 100% when any of the affected error codes are triggered.

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)
- **Python version (if murk-python):** 3.10+

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```python
# Trigger error -4 (snapshot evicted from ring buffer)
# The error message will say: "Increase ring_buffer_size in your config"
# But Config has no set_ring_buffer_size() method
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-python/src/error.rs.md
**Verified lines:** `crates/murk-python/src/error.rs:64,79,117,140`, `crates/murk-python/src/config.rs` (searched for missing setters)
**Root cause:** Error hint strings were copied from engine-level guidance without checking parity against the Python binding's public API surface.
**Suggested fix:** Either (a) update hints to only reference available Python APIs and suggest workarounds, or (b) add the missing config setters (`set_ring_buffer_size`, `set_max_ingress_queue`, async config tuning) to the Python `Config` class.
