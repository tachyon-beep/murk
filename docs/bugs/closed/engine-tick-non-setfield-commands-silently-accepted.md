# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine
- [x] murk-core (contract mismatch)

## Engine Mode

- [x] Lockstep
- [x] RealtimeAsync
- [ ] Both / Unknown

## Summary

`TickEngine::execute_tick` reports non-`SetField` commands (SetParameter, SetParameterBatch, Move, Spawn, Despawn, Custom) as successfully applied even though they are never executed, causing silent command loss and param_version stagnation.

## Steps to Reproduce

1. Create a `LockstepWorld`.
2. Submit a `SetParameter` command via `step_sync()`.
3. Inspect the returned receipt: `accepted: true`, `applied_tick_id: Some(tick)`.
4. Check `param_version`: it remains at `ParameterVersion(0)`, never incremented.
5. The parameter value was never actually applied anywhere.

## Expected Behavior

- `SetParameter` and `SetParameterBatch` commands should be applied to the engine's parameter store and increment `param_version` (per `murk-core/src/id.rs:113` documentation).
- Unimplemented command types should not receive `accepted: true` + `applied_tick_id: Some(tick)` receipts.

## Actual Behavior

- All drained commands receive `accepted: true` receipts at tick.rs:209-216 before any payload handling.
- Only `SetField` is handled (tick.rs:218-233); all other variants fall through with no execution.
- `applied_tick_id` is set for all accepted receipts at tick.rs:305-307, regardless of whether the command was actually executed.
- `param_version` is never incremented (tick.rs:154, 297, 361), contradicting the documented contract.

## Reproduction Rate

Always (for any non-SetField command type)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [x] Silent command loss affects simulation correctness
- [x] param_version stagnation breaks stale-parameter detection

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_engine::LockstepWorld;
use murk_core::command::{Command, CommandPayload};
use murk_core::id::{TickId, ParameterKey};

let mut world = LockstepWorld::new(config)?;
let cmd = Command {
    payload: CommandPayload::SetParameter {
        key: ParameterKey(0),
        value: 42.0,
    },
    expires_after_tick: TickId(100),
    source_id: None,
    source_seq: None,
    priority_class: 1,
    arrival_seq: 0,
};
let result = world.step_sync(vec![cmd])?;
// Receipt says applied, but parameter was never set.
// param_version is still 0.
assert!(result.receipts[0].applied_tick_id.is_some()); // true, but misleading
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-engine/src/tick.rs.md
**Verified lines:** tick.rs:209-216 (pre-accept all), tick.rs:218-233 (only SetField handled), tick.rs:305-307 (applied_tick_id for all), tick.rs:154+297+361 (param_version never incremented), murk-core/src/command.rs:118 (SetParameter doc), murk-core/src/id.rs:113-114 (ParameterVersion doc)
**Root cause:** Receipt finalization is decoupled from actual command execution. The code pre-accepts all drained commands but only implements SetField handling.
**Suggested fix:** Replace the `if let SetField` block with an exhaustive `match` on `CommandPayload`. Implement SetParameter/SetParameterBatch application with param_version increment. For unimplemented variants, either reject with a reason code or clearly mark as not-yet-supported.
