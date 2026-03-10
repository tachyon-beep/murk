# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-propagators

## Engine Mode

- [x] Lockstep

## Summary

Two related issues with the agent presence field and `AgentMovementPropagator`:

### Issue 1: Field bounds conflict with actual values (H-2)
The `FieldDef` for `agent_presence` declares `bounds: Some((0.0, 1.0))` (fields.rs:47), but `AgentMovementPropagator` writes `(agent_id as f32) + 1.0` (agent_movement.rs:155). For agent_id=1 that's 2.0, agent_id=2 gives 3.0, etc. — all exceeding the declared upper bound of 1.0. If the engine ever enforces field bounds, valid agent markers would be rejected.

### Issue 2: Missing reads_previous() declaration (H-1)
`AgentMovementPropagator` uses `WriteMode::Incremental` for `AGENT_PRESENCE`, meaning the engine seeds the write buffer from the previous generation. However, `reads_previous()` returns `FieldSet::empty()` (default). The propagator effectively reads previous-tick data through the Incremental seeding mechanism, but the engine's dependency graph doesn't track this implicit read. If another propagator writes `AGENT_PRESENCE` before this one in the same tick, the dependency analysis won't catch the conflict.

## Expected Behavior

1. Bounds should accommodate actual marker values, or be removed.
2. `reads_previous()` should declare `AGENT_PRESENCE`.

## Actual Behavior

1. Bounds are `(0.0, 1.0)` but values reach `65536.0` for `u16::MAX` agents.
2. Invisible data dependency not tracked by pipeline validator.

## Additional Context

**Source:** murk-propagators audit, H-1 + H-2
**Files:** `crates/murk-propagators/src/fields.rs:47`, `crates/murk-propagators/src/agent_movement.rs:84-95,155`
