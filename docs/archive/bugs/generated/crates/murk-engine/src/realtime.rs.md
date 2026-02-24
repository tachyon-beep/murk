# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [x] Critical | [ ] High | [ ] Medium | [ ] Low

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

`RealtimeAsyncWorld::new()` can panic during worker thread spawn and leak already-started background threads because construction is not rolled back on partial startup failure.

## Steps to Reproduce

1. Constrain process thread quota so exactly one additional thread can be created (tick thread succeeds, first egress worker fails).
2. Call `RealtimeAsyncWorld::new(...)` with `AsyncConfig { worker_count: Some(1), .. }`.
3. Observe panic `"failed to spawn egress worker"`.

## Expected Behavior

`new()` should return an error (not panic), and any already-started tick/worker threads should be cleanly shut down and joined before returning.

## Actual Behavior

`new()` panics via `.expect(...)` and unwinds before `RealtimeAsyncWorld` is constructed, so no `Drop` cleanup runs; already spawned thread(s) remain orphaned.

## Reproduction Rate

Always (under constrained thread quota).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```text
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// Pseudocode: set RLIMIT_NPROC so only one extra thread can spawn.
// Then call RealtimeAsyncWorld::new with worker_count=1.
//
// Expected: Err(...)
// Actual: panic("failed to spawn egress worker")
```

## Additional Context

Evidence:
- Tick thread is spawned first: `/home/john/murk/crates/murk-engine/src/realtime.rs:228`
- Egress workers are spawned after tick thread: `/home/john/murk/crates/murk-engine/src/realtime.rs:249`
- Worker spawn uses panic-on-error: `/home/john/murk/crates/murk-engine/src/realtime.rs:455`
- World instance is only created later (so no `Drop` rollback on panic): `/home/john/murk/crates/murk-engine/src/realtime.rs:257`

Root cause:
- Thread spawn failures are handled with `.expect(...)` in constructor path instead of recoverable error handling plus rollback.

Suggested fix:
- Replace `.expect(...)` with `Result` propagation.
- On partial startup failure, set shutdown flag, drop channels, join already-started threads, then return a `ConfigError` variant for thread-spawn failure.

---

# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [x] Critical | [ ] High | [ ] Medium | [ ] Low

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

`RealtimeAsyncWorld::reset()` can panic on tick-thread respawn failure even though the API returns `Result`, causing production unwind instead of recoverable error.

## Steps to Reproduce

1. Create a `RealtimeAsyncWorld`.
2. Constrain thread creation quota so spawning the new tick thread fails.
3. Call `world.reset(new_seed)`.

## Expected Behavior

`reset()` should return `Err(...)` (for example a thread-spawn config/runtime error) and preserve recoverable state.

## Actual Behavior

`reset()` panics at thread spawn `.expect("failed to spawn tick thread")` and does not return `Err`.

## Reproduction Rate

Always (under constrained thread quota).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```text
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// Pseudocode: after constructing world, force thread spawn failure (RLIMIT_NPROC),
// then call world.reset(123).
//
// Expected: Err(...)
// Actual: panic("failed to spawn tick thread")
```

## Additional Context

Evidence:
- `reset()` is defined as fallible (`Result`): `/home/john/murk/crates/murk-engine/src/realtime.rs:607`
- Thread respawn in reset panics on spawn error: `/home/john/murk/crates/murk-engine/src/realtime.rs:676`
- State is only marked `Running` after that point: `/home/john/murk/crates/murk-engine/src/realtime.rs:688`

Root cause:
- Panic-based error handling (`expect`) in a runtime failure path inside a fallible API.

Suggested fix:
- Replace `expect` with error propagation (`ConfigError` variant).
- Restore `recovered_engine` or otherwise keep reset retry-safe when respawn fails.