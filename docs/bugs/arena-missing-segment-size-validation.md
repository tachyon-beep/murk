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

`ArenaConfig` documents that `segment_size` must be a power of two and at least 1024, but no validation enforces these invariants, allowing invalid configs that produce misleading runtime errors.

## Steps to Reproduce

1. Create an `ArenaConfig` with `segment_size = 7` (not a power of two, less than 1024).
2. Pass it to `PingPongArena::new()`.
3. Observe that construction succeeds despite violating the documented constraint.
4. Subsequent allocations may fail with `CapacityExceeded` rather than `InvalidConfig`.

## Expected Behavior

`PingPongArena::new()` should return `Err(ArenaError::InvalidConfig { .. })` when `segment_size` is not a power of two or is less than 1024, consistent with the documented contract on `ArenaConfig::segment_size` (line 12) and the "Validated at construction" claim (line 6).

## Actual Behavior

Any `segment_size` value is silently accepted. Invalid values lead to downstream errors (`CapacityExceeded`) that do not indicate the root cause is a misconfigured segment size. The `segment_size` field is `pub`, so it can also be mutated after construction despite the "immutable after creation" documentation.

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
use murk_arena::{ArenaConfig, PingPongArena};
use murk_arena::static_arena::StaticArena;

let config = ArenaConfig {
    segment_size: 7, // violates "power of two, >= 1024"
    max_segments: 16,
    max_generation_age: 1,
    cell_count: 100,
};
// This succeeds but should return Err(InvalidConfig)
let static_arena = StaticArena::new(&[]).into_shared();
let arena = PingPongArena::new(config, vec![], static_arena);
assert!(arena.is_ok()); // BUG: should be Err
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-arena/src/config.rs.md
**Verified lines:** config.rs:6 (doc claim), config.rs:12-13 (constraint doc + pub field), config.rs:58 (unchecked constructor), pingpong.rs:107 (only max_segments validated)
**Root cause:** Config invariants documented on `ArenaConfig` are not enforced anywhere. `PingPongArena::new()` only validates `max_segments >= 3`.
**Suggested fix:** Add `ArenaConfig::validate(&self) -> Result<(), ArenaError>` enforcing `segment_size >= 1024` and `segment_size.is_power_of_two()`. Call it at the start of `PingPongArena::new()`. Optionally make config fields private and provide a builder with validation.
