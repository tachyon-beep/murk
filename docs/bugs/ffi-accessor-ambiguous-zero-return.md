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
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [x] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

Four world accessor FFI functions return `0` for invalid/stale handles, which is indistinguishable from valid world state:

- `murk_current_tick` (line 224-228): returns `0` on invalid handle, same as tick 0 after construction/reset
- `murk_is_tick_disabled` (line 234-238): returns `0` on invalid handle, same as "not disabled"
- `murk_consecutive_rollbacks` (line 244-248): returns `0` on invalid handle, same as zero rollbacks
- `murk_seed` (line 254-258): returns `0` on invalid handle, same as seed value `0`

This means C callers cannot distinguish between "the world is at tick 0" and "I'm using a stale/destroyed handle." This can silently mask use-after-destroy bugs in caller code, especially in RL training loops that frequently create/destroy environments.

## Steps to Reproduce

```c
uint64_t world = create_and_destroy_world();
// world handle is now stale
uint64_t tick = murk_current_tick(world);
// tick == 0, indistinguishable from a freshly-created world
// Caller thinks the world is valid and at tick 0
```

## Expected Behavior

Accessor functions should provide a way to signal invalid-handle errors, either through:
1. Status-returning variants with output pointers (e.g., `murk_current_tick_get(handle, &out_tick) -> status`), or
2. Sentinel values that are outside the valid domain (not possible for all four functions since 0 is valid for all of them)

## Actual Behavior

Invalid handles silently return `0`, which is a valid value in all four domains (tick, disabled flag, rollback count, seed).

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
use murk_ffi::world::{murk_current_tick, murk_lockstep_destroy};

fn demonstrate_ambiguity(world_h: u64) {
    murk_lockstep_destroy(world_h);
    let tick = murk_current_tick(world_h);
    assert_eq!(tick, 0); // Returns 0 -- same as a valid world at tick 0
    // No way to know the handle was invalid
}
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-ffi/src/world.rs.md
**Verified lines:** world.rs:221-258
**Root cause:** Scalar-returning accessor APIs have no out-of-band error channel. They use `0` as the error sentinel, but `0` is a valid value for all four quantities.
**Suggested fix:**
1. Add status-returning accessor variants that take an output pointer:
   ```c
   int murk_current_tick_get(uint64_t handle, uint64_t* out_tick);
   int murk_is_tick_disabled_get(uint64_t handle, uint8_t* out_disabled);
   int murk_consecutive_rollbacks_get(uint64_t handle, uint32_t* out_count);
   int murk_seed_get(uint64_t handle, uint64_t* out_seed);
   ```
   These return `MurkStatus::InvalidHandle` on stale handles.
2. Keep the existing functions for backward compatibility but document the ambiguity.
3. In murk-python, prefer the status-returning variants and raise exceptions on invalid handles.
