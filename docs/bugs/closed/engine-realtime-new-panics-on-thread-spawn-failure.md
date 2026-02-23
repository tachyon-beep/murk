# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [ ] Lockstep
- [x] RealtimeAsync
- [ ] Both / Unknown

## Summary

`RealtimeAsyncWorld::new()` and `reset()` use `.expect()` for thread spawn operations, panicking instead of returning `Err` when thread creation fails, and leaking already-spawned threads on partial startup failure.

## Steps to Reproduce

1. Constrain the process thread quota (e.g., `RLIMIT_NPROC`) so that one or more thread spawns fail.
2. Call `RealtimeAsyncWorld::new(config, async_config)`.
3. Observe panic from `.expect("failed to spawn tick thread")` or `.expect("failed to spawn egress worker")`.

## Expected Behavior

`new()` should return `Err(ConfigError::...)` and cleanly shut down any already-spawned threads before returning. `reset()` should also propagate a `Result` error instead of panicking.

## Actual Behavior

- `realtime.rs:246`: tick thread spawn uses `.expect("failed to spawn tick thread")`.
- `realtime.rs:455`: egress worker spawn uses `.expect("failed to spawn egress worker")`.
- `realtime.rs:676`: reset's tick thread respawn uses `.expect("failed to spawn tick thread")`.

If the tick thread spawns successfully but an egress worker fails, the panic unwinds before `RealtimeAsyncWorld` is constructed, so `Drop` never runs and the tick thread is orphaned. Similarly, `reset()` returns `Result<(), ConfigError>` but panics on spawn failure instead of returning `Err`.

## Reproduction Rate

Always (under constrained thread quota)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** 0.1.8 / HEAD (feat/release-0.1.9)

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
// Pseudocode: constrain RLIMIT_NPROC so only one extra thread can spawn.
// Tick thread succeeds, first egress worker fails.
//
// Expected: Err(ConfigError::...)
// Actual: panic("failed to spawn egress worker")
//         + orphaned tick thread
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-engine/src/realtime.rs.md`

**Affected lines:**
- Tick thread spawn in `new()`: `crates/murk-engine/src/realtime.rs:228-246`
- Egress worker spawn: `crates/murk-engine/src/realtime.rs:450-455`
- Tick thread respawn in `reset()`: `crates/murk-engine/src/realtime.rs:658-676`

**Root cause:** Three `.expect()` sites in the constructor and reset paths convert `std::io::Error` from thread spawn into panics instead of propagating as `ConfigError`.

**Suggested fix:** Replace `.expect()` with `Result` propagation (add a `ThreadSpawnFailed` variant to `ConfigError`). On partial startup failure, set the shutdown flag, drop channels, and join already-spawned threads before returning the error.
