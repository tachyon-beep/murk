# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [x] murk-arena
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

Unchecked `u32` generation counter increments in `PingPongArena` overflow in debug builds (panic) or wrap in release builds, breaking generation-based correctness invariants including sparse CoW logic.

## Steps to Reproduce

1. Create a `PingPongArena`.
2. Run `begin_tick()` / `publish()` in a loop for `u32::MAX` iterations.
3. On iteration `u32::MAX`, `self.generation + 1` at pingpong.rs:172 overflows.
4. In debug mode: arithmetic panic. In release mode: wraps to 0.
5. After wrap, `handle.generation() == self.generation` at write.rs:170 may falsely match old handles, skipping CoW copy.

## Expected Behavior

The arena should either detect generation overflow and return an error, or use a wide enough counter (u64) to avoid practical overflow.

## Actual Behavior

In `begin_tick()` (pingpong.rs:172), `self.generation + 1` is computed with unchecked arithmetic. In `publish()` (pingpong.rs:269), `self.generation += 1` is also unchecked. After `u32::MAX` ticks:
- Debug builds: panic on arithmetic overflow.
- Release builds: generation wraps to 0, causing the sparse CoW guard at write.rs:170 (`handle.generation() == self.generation`) to falsely identify stale handles as "already written this tick," silently skipping the copy-before-write step.

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
// Conceptual -- impractical to run 4B iterations in a test,
// but the arithmetic is verifiable by inspection:
//
// pingpong.rs:172:  let next_gen = self.generation + 1;  // overflows at u32::MAX
// pingpong.rs:269:  self.generation += 1;                // overflows at u32::MAX
//
// At 10K ticks/sec, overflow occurs after ~5 days of continuous operation.
// At 100K ticks/sec (batched training), overflow occurs after ~12 hours.
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-arena/src/pingpong.rs.md
**Verified lines:** pingpong.rs:172 (`self.generation + 1`, unchecked), pingpong.rs:269 (`self.generation += 1`, unchecked), pingpong.rs:300 (`WorldGenerationId(self.generation as u64)`, publishes wrapped value), write.rs:170 (equality check vulnerable to wrap collision)
**Root cause:** Generation counter is `u32` with unchecked increments. The arena assumes strictly monotonic generations but does not guard against overflow.
**Suggested fix:** Either (a) replace `u32` generation with `u64` across handle/snapshot paths to avoid practical overflow (u64 overflows after ~584 billion years at 1M ticks/sec), or (b) use `checked_add` and return `ArenaError::GenerationOverflow` from `begin_tick()`.
