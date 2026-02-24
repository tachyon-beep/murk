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
- [x] murk-ffi
- [x] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [x] Lockstep
- [ ] RealtimeAsync
- [ ] Both / Unknown

## Summary

`StepMetrics::from_ffi` assembles metrics from two separate data sources without snapshot pinning. Aggregate fields (total_us, command_processing_us, etc.) come from the `MurkStepMetrics` struct returned by `murk_lockstep_step`, while per-propagator timings are fetched separately via `murk_step_metrics_propagator`, which queries `world.last_metrics()`. In a multi-threaded Python environment, another thread could call `step()` on the same world between these two operations, causing the per-propagator timings to belong to a different tick than the aggregate metrics.

## Steps to Reproduce

1. Create a world and share the `World` object across two Python threads.
2. From thread A, call `world.step(cmds)`.
3. From thread B, call `world.step(cmds)` concurrently.
4. Thread A's `step()` returns from `py.detach()`, then thread B's `step()` runs and completes, then thread A calls `StepMetrics::from_ffi` which queries `last_metrics()` from thread B's tick.

## Expected Behavior

`StepMetrics` should contain internally consistent data: all fields should correspond to the same tick.

## Actual Behavior

Aggregate metrics may come from tick N while per-propagator timings come from tick N+1 (or later), producing an internally inconsistent `StepMetrics` object.

## Reproduction Rate

- Requires multi-threaded access to the same World object, which is uncommon in typical RL usage but possible.

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)
- **Python version (if murk-python):** 3.10+

## Determinism Impact

- [ ] Bug is deterministic (same inputs always reproduce it)
- [x] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```python
import murk
import threading

world = murk.World(config)

def step_loop():
    for _ in range(1000):
        receipts, metrics = world.step(None)
        # metrics.propagator_us might not match metrics.total_us

t1 = threading.Thread(target=step_loop)
t2 = threading.Thread(target=step_loop)
t1.start(); t2.start()
t1.join(); t2.join()
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-python/src/metrics.rs.md
**Verified lines:** `crates/murk-python/src/metrics.rs:76-101`, `crates/murk-python/src/world.rs:68,106-128`, `crates/murk-ffi/src/metrics.rs:35,62-64`
**Root cause:** Per-propagator timings are fetched via a separate FFI call to `world.last_metrics()` rather than being part of the atomic step result. The world lock is released between the step and the per-propagator query.
**Suggested fix:** Include per-propagator timings in the `MurkStepMetrics` struct returned by `murk_lockstep_step`, or snapshot the propagator timings within the same lock acquisition as the step result. Alternatively, have `from_ffi` use `murk_step_metrics` (which also reads `last_metrics()`) rather than the step-returned struct, accepting that both sources are at least reading the same snapshot.

## Resolution

**Fixed:** 2026-02-21
**Commit branch:** feat/release-0.1.7

**Fix:** Thread-local propagator timing snapshot. During `murk_lockstep_step` (while the world lock is held), the per-propagator timings are cloned into a `thread_local!` buffer via `snapshot_propagator_timings()`. `murk_step_metrics_propagator` now reads from this thread-local buffer instead of re-acquiring the world lock. This guarantees the aggregate metrics and per-propagator timings always come from the same tick, regardless of concurrent stepping by other threads.

**Files changed:** `crates/murk-ffi/src/metrics.rs`, `crates/murk-ffi/src/world.rs`
**No new tests:** The race requires multi-threaded Python with GIL release and is inherently timing-dependent. The fix is verified by code inspection — the thread-local snapshot is populated atomically during step, making the race impossible.
