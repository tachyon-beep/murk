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
- [x] murk-propagators
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

`AgentMovementPropagator` re-runs initial placement on any later tick where `agent_presence` is all zeros, causing unintended agent respawn because tick-0 detection uses field contents instead of `tick_id`.

## Steps to Reproduce

1. Create `AgentMovementPropagator` with non-empty `initial_positions`, e.g. `vec![(0, 4)]`.
2. Run one step (tick 0) so initial placement occurs, then clear `AGENT_PRESENCE` to all zeros (e.g., another propagator or test harness writes zeros).
3. Run another step with `TickId > 0` and no actions.

## Expected Behavior

Initial placement should happen only on tick 0. On later ticks, an all-zero `AGENT_PRESENCE` should remain empty unless actions/other logic explicitly add agents.

## Actual Behavior

Because the code checks `all_zero && !initial_positions.is_empty()` without checking tick ID, it places initial agents again and returns early, effectively respawning agents on non-zero ticks.

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
use murk_core::{FieldWriter, TickId};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::Propagator;
use murk_propagator::scratch::ScratchRegion;
use murk_propagators::agent_movement::{new_action_buffer, AgentMovementPropagator};
#[allow(deprecated)]
use murk_propagators::fields::AGENT_PRESENCE;
use murk_space::{EdgeBehavior, Square4};
use murk_test_utils::{MockFieldReader, MockFieldWriter};

fn main() {
    let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
    let prop = AgentMovementPropagator::new(new_action_buffer(), vec![(0, 4)]);

    let reader = MockFieldReader::new();
    let mut writer = MockFieldWriter::new();
    writer.add_field(AGENT_PRESENCE, 9);
    let mut scratch = ScratchRegion::new(0);

    // Tick 0: initializes at index 4.
    {
        let mut ctx = StepContext::new(&reader, &reader, &mut writer, &mut scratch, &grid, TickId(0), 0.01);
        prop.step(&mut ctx).unwrap();
    }

    // Simulate all agents removed by other logic.
    writer.write(AGENT_PRESENCE).unwrap().fill(0.0);

    // Tick 5: should stay empty, but respawns due to all_zero check.
    {
        let mut ctx = StepContext::new(&reader, &reader, &mut writer, &mut scratch, &grid, TickId(5), 0.01);
        prop.step(&mut ctx).unwrap();
    }

    let p = writer.get_field(AGENT_PRESENCE).unwrap();
    assert_eq!(p[4], 0.0, "BUG: agent was respawned on non-zero tick");
}
```

## Additional Context

Root-cause evidence in `crates/murk-propagators/src/agent_movement.rs`:
- `crates/murk-propagators/src/agent_movement.rs:158` computes `all_zero` from field contents.
- `crates/murk-propagators/src/agent_movement.rs:159` gates initialization only on `all_zero` and `initial_positions`, not tick number.
- `crates/murk-propagators/src/agent_movement.rs:165` returns early after re-initialization.

Suggested fix:
- Gate init on `ctx.tick_id() == TickId(0)` (or equivalent) and remove/limit the all-zero heuristic so non-zero ticks cannot respawn implicitly.