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
- [x] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [ ] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [x] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`AgentMovementPropagator::step()` applies movement actions on the same tick it initializes agent positions (tick 0), violating its documented contract: "On tick 0 ... places agents at their initial positions. On subsequent ticks ... moves agents." If actions are queued in the action buffer before or during tick 0, they will be processed immediately after initialization, potentially moving agents away from their initial positions on the very first tick.

## Steps to Reproduce

1. Create an `AgentMovementPropagator` with initial positions.
2. Before tick 0, push movement actions into the action buffer.
3. Execute tick 0.
4. Observe that agents are moved from their initial positions in the same tick.

## Expected Behavior

On tick 0 (the initialization tick), the propagator should place agents at their initial positions and return early without processing any actions. Actions should only be processed on subsequent ticks (`tick > 0`).

## Actual Behavior

After the `all_zero` branch places agents at initial positions (lines 149-157), execution falls through to the action-processing loop (lines 159-213) without an early return. The only early exit is at line 159-161, gated on `actions_snapshot.is_empty()`, which does not trigger if actions were queued.

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
use murk_propagators::agent_movement::*;

let ab = new_action_buffer();
let prop = AgentMovementPropagator::new(
    ab.clone(),
    vec![(0, 4)], // agent 0 starts at center of 3x3 grid
);

// Queue an action BEFORE tick 0
ab.lock().unwrap().push(AgentAction {
    agent_id: 0,
    direction: Direction::North,
});

// Execute tick 0: agent is placed at index 4, then immediately moved north
// BUG: agent ends up at index 1 instead of staying at index 4
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-propagators/src/agent_movement.rs.md
**Verified lines:** agent_movement.rs:61-62 (documented contract), agent_movement.rs:149-157 (tick-0 init), agent_movement.rs:159-213 (action processing -- no early return after init)
**Root cause:** Tick-0 detection via `all_zero` correctly seeds initial positions but does not short-circuit action processing. No guard prevents the movement loop from running during the initialization tick.
**Suggested fix:** Add `return Ok(());` after the tick-0 initialization block (after line 157), before the action processing section. Alternatively, gate the movement loop with a `tick_id > 0` check from `StepContext` if tick ID is exposed.
