# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent
**Severity:** [x] Critical | [ ] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`SetField` commands with an out-of-bounds coordinate or missing field are silently not applied, but the receipt still reports `accepted=true` and `applied_tick_id=Some(next_tick)`, giving callers a false confirmation that the write succeeded.

## Steps to Reproduce

1. Create a `LockstepWorld` with a `Line1D(4)` space and one field.
2. Submit a `SetField` command with an out-of-bounds coordinate (e.g., `coord=[99]`).
3. Call `step_sync` and inspect the receipt.

## Expected Behavior

A `SetField` command that cannot be applied (coordinate out of bounds, field missing, or rank exceeds buffer length) should be rejected or at minimum not have `applied_tick_id` set. The caller must be able to distinguish between a command that was actually executed and one that was silently dropped.

## Actual Behavior

The receipt is pre-set to `accepted: true` at `tick.rs:254-255`. The `SetField` processing at `tick.rs:264-276` silently skips the write when:
- `canonical_rank(coord)` returns `None` (line 270) -- coord out of bounds
- `guard.writer.write(field_id)` returns `None` (line 271) -- field does not exist
- `rank >= buf.len()` (line 272) -- rank exceeds buffer length

In all three cases, the receipt is NOT updated. At `tick.rs:380-384`, all `accepted` receipts receive `applied_tick_id = Some(next_tick)`. The caller sees a receipt that says the command was applied, but the snapshot is unchanged.

## Reproduction Rate

Always (for any out-of-bounds / non-applied SetField)

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
let mut world = LockstepWorld::new(config_line1d_4_cells()).unwrap();
let cmd = Command {
    payload: CommandPayload::SetField {
        coord: vec![99i32].into(), // out of bounds for len=4
        field_id: FieldId(0),
        value: 123.0,
    },
    expires_after_tick: TickId(100),
    source_id: None,
    source_seq: None,
    priority_class: 1,
    arrival_seq: 0,
};
let step = world.step_sync(vec![cmd]).unwrap();
assert!(step.receipts[0].accepted);                          // true - misleading
assert_eq!(step.receipts[0].applied_tick_id, Some(TickId(1))); // set - misleading
// But snapshot is unchanged: no write happened
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-engine/src/tick.rs.md`

**Affected lines:**
- Receipt pre-set: `crates/murk-engine/src/tick.rs:254-259`
- SetField silent skip: `crates/murk-engine/src/tick.rs:264-276`
- applied_tick_id assignment: `crates/murk-engine/src/tick.rs:380-384`

**Root cause:** The `SetField` write path exits early via nested `if let` without updating the receipt on failure. The blanket `applied_tick_id` assignment at line 383 does not distinguish between commands that were actually applied and those that were silently dropped.

**Suggested fix:** When the `SetField` write path exits early (coord not found, field missing, rank out of bounds), set `receipt.accepted = false` with an appropriate `reason_code` (e.g., `IngressError::InvalidCoord` or a new variant), or at minimum do not set `applied_tick_id`.
