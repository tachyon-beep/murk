# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
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
- [ ] murk-python
- [x] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [x] Lockstep
- [ ] RealtimeAsync
- [ ] Both / Unknown

## Summary

Actions injected before the first `step_sync()` in `lockstep_rl.rs` are silently discarded, so the first tick does not apply the intended per-agent action inputs.

## Steps to Reproduce

1. Run two lockstep worlds with identical config/seed; in world A, push one `AgentAction` before the first `step_sync()`, in world B push none.
2. Call `step_sync(vec![])` once on both worlds.
3. Compare `AGENT_PRESENCE` snapshots after tick 1.

## Expected Behavior

The action queued for world A should affect the first stepped state, producing a different `AGENT_PRESENCE` from world B.

## Actual Behavior

Both worlds produce identical post-step state at tick 1 because pre-tick-0 actions are drained and dropped during initialization.

## Reproduction Rate

Always

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
use murk_bench::reference_profile;
use murk_core::FieldReader;
use murk_engine::LockstepWorld;
use murk_propagators::agent_movement::{new_action_buffer, AgentAction, Direction};
#[allow(deprecated)]
use murk_propagators::fields::AGENT_PRESENCE;

fn main() {
    let ab_a = new_action_buffer();
    let ab_b = new_action_buffer();

    let mut world_a = LockstepWorld::new(reference_profile(42, ab_a.clone())).unwrap();
    let mut world_b = LockstepWorld::new(reference_profile(42, ab_b.clone())).unwrap();

    ab_a.lock().unwrap().push(AgentAction { agent_id: 0, direction: Direction::North });

    let a1 = world_a.step_sync(vec![]).unwrap();
    let b1 = world_b.step_sync(vec![]).unwrap();

    let pa = a1.snapshot.read(AGENT_PRESENCE).unwrap();
    let pb = b1.snapshot.read(AGENT_PRESENCE).unwrap();

    assert_eq!(pa, pb, "pre-first-step action was ignored");
}
```

## Additional Context

Evidence in target file:
- `/home/john/murk/crates/murk-bench/examples/lockstep_rl.rs:31`
- `/home/john/murk/crates/murk-bench/examples/lockstep_rl.rs:34`
- `/home/john/murk/crates/murk-bench/examples/lockstep_rl.rs:43`

Root-cause evidence in movement propagator:
- `/home/john/murk/crates/murk-propagators/src/agent_movement.rs:137` (locks and drains action buffer)
- `/home/john/murk/crates/murk-propagators/src/agent_movement.rs:155` (tick-0 initialization path)
- `/home/john/murk/crates/murk-propagators/src/agent_movement.rs:165` (early return; drained actions never applied)

Suggested fix in example:
- Perform one warm-up `step_sync(vec![])` before starting action injection, or start injecting actions from the second step onward so “one action per agent per tick” is actually true for processed ticks.