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
- [x] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [x] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

Every `extern "C"` FFI function in the murk-ffi crate acquires mutexes via `lock().unwrap()`. If any thread panics while holding one of these mutexes (e.g., `WORLDS`, `CONFIGS`, `OBS_PLANS`, or a per-world `Arc<Mutex<LockstepWorld>>`), the mutex becomes poisoned. Subsequent calls to `lock().unwrap()` on that poisoned mutex will panic. Since these panics occur inside `extern "C"` functions, they constitute undefined behavior (unwinding across the FFI boundary) and typically abort the host process.

The static analysis report identified `metrics.rs` lines 56, 62, 100, 106, but the issue is systemic across all FFI modules: `config.rs` (7 sites), `obs.rs` (11 sites), `world.rs` (10 sites), `metrics.rs` (4 sites), and `batched.rs` (11 sites) -- totaling 43+ `lock().unwrap()` calls in `extern "C"` functions.

## Steps to Reproduce

```
1. From C/Python, create a world and a propagator whose step_fn panics
   (e.g., a Rust propagator that panics on tick 2).
2. Call murk_lockstep_step -- the propagator panics while holding the
   per-world mutex, poisoning it.
3. Call murk_lockstep_step again on the same world handle.
4. The second call hits lock().unwrap() on the poisoned mutex and panics
   inside extern "C", causing UB / process abort.
```

## Expected Behavior

FFI functions should never panic. On encountering a poisoned mutex, they should return an appropriate error status code (e.g., a new `MurkStatus::InternalError` variant) so the caller can handle the error gracefully.

## Actual Behavior

`lock().unwrap()` panics on poisoned mutexes inside `extern "C"` functions, which is undefined behavior. In practice this aborts the host process with no recoverable error path.

## Reproduction Rate

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// Difficult to reproduce in a unit test without injecting a panic
// into a propagator step function. The scenario requires:
// 1. A panicking propagator that poisons the world mutex
// 2. A subsequent FFI call on the same world
//
// In practice, any bug in a C propagator callback that triggers
// a Rust panic (e.g., via an assert! macro) will poison the mutex.
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-ffi/src/metrics.rs.md
**Verified lines:** metrics.rs:56, 62, 100, 106; config.rs:63, 72, 214, 281, 306, 320, 334, 348, 362; obs.rs:172, 178, 188, 212, 233, 239, 298, 309, 315, 347, 359, 372; world.rs:27, 46, 80, 90, 134, 172, 201, 226, 236, 246, 256, 310
**Root cause:** All 43+ mutex acquisitions in `extern "C"` functions use `.lock().unwrap()` with no panic guard. Poisoned mutex -> unwrap panic -> UB at FFI boundary.
**Suggested fix:**
1. Create a crate-internal helper: `fn ffi_lock<T>(m: &Mutex<T>) -> Result<MutexGuard<T>, MurkStatus>` that maps `PoisonError` to `MurkStatus::InternalError` (new variant) or recovers the guard via `into_inner()` depending on policy.
2. Replace all `lock().unwrap()` in extern "C" functions with this helper, propagating the error status to the caller.
3. Additionally, wrap each FFI function body in `std::panic::catch_unwind` to prevent any panics from crossing the FFI boundary (defense in depth).
