# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

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

`PropagatorDef::register` can leak a `TrampolineData` heap allocation on two error paths. `Box::into_raw(data)` is called at line 89, converting ownership to a raw pointer. If `CString::new()` at line 101 fails (name contains interior NUL byte), or if `config.add_propagator_handle()` at line 141 fails, the raw pointer is never reclaimed.

The `murk_propagator_create` failure path (lines 131-137) IS properly handled with a `Box::from_raw` cleanup. But the CString and `add_propagator_handle` error paths are not.

## Steps to Reproduce

1. Create a `PropagatorDef` with a name containing an interior NUL byte (e.g., `"foo\x00bar"`).
2. Call `propagator_def.register(config)`.
3. The `CString::new()` fails, the function returns early, and the `TrampolineData` allocation is leaked.

## Expected Behavior

All error paths should clean up the raw pointer by converting it back to a `Box` and dropping it, or by deferring the `Box::into_raw` until after all fallible operations succeed.

## Actual Behavior

Memory leak of `TrampolineData` (which contains a `Py<PyAny>` reference to the Python callable, along with field ID vectors).

## Reproduction Rate

- Deterministic when the CString conversion fails or `add_propagator_handle` returns an error.

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
import murk

config = murk.Config()
# ... set up config ...

def my_step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
    pass

prop = murk.PropagatorDef("name\x00with_nul", my_step_fn, writes=[(0, murk.WriteMode.Full)])
prop.register(config)  # CString::new fails, TrampolineData leaked
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-python/src/propagator.rs.md
**Verified lines:** `crates/murk-python/src/propagator.rs:83-89,101-102,141-143`
**Root cause:** `Box::into_raw` is called before all fallible operations complete. The `murk_propagator_create` error path has cleanup, but the `CString::new` and `add_propagator_handle` error paths do not.
**Suggested fix:** Keep `TrampolineData` as `Box<TrampolineData>` during setup. Only call `Box::into_raw` after `CString::new` and FFI propagator creation succeed. Alternatively, add explicit cleanup in each early-return path after `Box::into_raw`.
