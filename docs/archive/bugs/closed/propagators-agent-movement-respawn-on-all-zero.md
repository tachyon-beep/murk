# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent (wave-5)
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-propagators

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`AgentMovementPropagator` uses an all-zero field heuristic instead of tick ID to detect the initialization tick, causing agents to respawn on any later tick where all agent presence values are zero.

## Steps to Reproduce

1. Create an `AgentMovementPropagator` with `initial_positions = vec![(0, 4)]` on a 3x3 grid.
2. Run tick 0 so that initial placement occurs (agent placed at index 4).
3. On a subsequent tick (e.g. tick 5), have another propagator or external logic clear `AGENT_PRESENCE` to all zeros (e.g. all agents die or are removed).
4. Run tick 5 with no actions in the buffer.

## Expected Behavior

Initial placement should only happen on the very first tick. On later ticks, an all-zero presence field should remain all-zero (agents are dead/removed), and agents should NOT be respawned.

## Actual Behavior

Because the initialization guard at `agent_movement.rs:158-159` checks `all_zero && !self.initial_positions.is_empty()` without checking `ctx.tick_id()`, the propagator re-runs initial placement whenever all agents have been removed. This silently and incorrectly respawns agents on non-zero ticks, corrupting the simulation state.

Evidence in `crates/murk-propagators/src/agent_movement.rs`:
- Line 158: `let all_zero = presence.iter().all(|&v| v == 0.0);`
- Line 159: `if all_zero && !self.initial_positions.is_empty() {` -- no tick ID check
- Line 165: `return Ok(());` -- early return after re-initialization
- The struct doc comment (line 62) says "On tick 0 (when presence is all zeros)" confirming the intent is tick-0-only behavior.

## Reproduction Rate

Always (given all agents removed after tick 0).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** 0.1.8 / HEAD

## Determinism Impact

- [x] Bug is deterministic
- [ ] Bug is non-deterministic
- [ ] Replay divergence observed

## Logs / Backtrace

```
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
```

## Additional Context

Closed ticket #30 (`propagator-agent-movement-tick0-actions`) addressed a different issue: actions being processed during the init tick. That fix (early return at line 165) is correct but does not address this distinct problem of the init condition triggering on non-zero ticks.

**Root cause:** The initialization guard uses field-content heuristic (`all_zero`) instead of tick identity (`ctx.tick_id()`). Any scenario where agents are removed (death, reset, external write) causes the all-zero condition to become true, triggering spurious re-initialization.

**Suggested fix:** Gate initialization on `ctx.tick_id() == TickId(0)` (or an internal `initialized` flag) instead of the all-zero heuristic. Alternatively, combine both checks: `all_zero && !self.initial_positions.is_empty() && ctx.tick_id() == TickId(0)`.

(Source report: `docs/bugs/generated/crates/murk-propagators/src/agent_movement.rs.md`)
