# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine
- [x] murk-ffi (reachability path)

## Engine Mode

- [x] Lockstep
- [x] RealtimeAsync
- [ ] Both / Unknown

## Summary

`IngressQueue::drain()` can misorder anonymous commands when a command has `source_id=None` but `source_seq=Some(_)`, violating the documented anonymous-arrival ordering contract.

## Steps to Reproduce

1. Submit two anonymous commands (both with `source_id=None`) in the same priority class:
   - Command A: `source_seq=Some(1)`, `arrival_seq=10`
   - Command B: `source_seq=None`, `arrival_seq=9`
2. Call `drain()`.
3. Sort key for A: `(p, MAX, 1, 10)` -- sort key for B: `(p, MAX, MAX, 9)`.
4. A sorts before B despite arriving later, violating the documented invariant that anonymous commands execute in arrival order.

The malformed `(source_id=None, source_seq=Some(_))` pair is reachable via FFI: `murk-ffi/src/command.rs:95-103` maps `source_id` and `source_seq` independently (source_id=0 -> None, source_seq!=0 -> Some).

## Expected Behavior

Anonymous commands (source_id=None) should always execute in arrival order within the same priority class, regardless of `source_seq` values.

## Actual Behavior

When `source_id=None` but `source_seq=Some(n)`, the sort comparator uses `n` instead of `u64::MAX`, causing the command to sort ahead of anonymous commands with `source_seq=None`.

## Reproduction Rate

Always (with the specific malformed input pattern)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [x] Breaks deterministic command ordering for affected inputs
- [ ] No determinism impact

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_engine::ingress::IngressQueue;
use murk_core::command::{Command, CommandPayload};
use murk_core::id::{TickId, ParameterKey};

let mut q = IngressQueue::new(10);
let cmd_a = Command {
    payload: CommandPayload::SetParameter { key: ParameterKey(0), value: 0.0 },
    expires_after_tick: TickId(100),
    source_id: None,
    source_seq: Some(1),  // malformed: source_seq without source_id
    priority_class: 1,
    arrival_seq: 0,
};
let cmd_b = Command {
    payload: CommandPayload::SetParameter { key: ParameterKey(0), value: 0.0 },
    expires_after_tick: TickId(100),
    source_id: None,
    source_seq: None,
    priority_class: 1,
    arrival_seq: 0,
};
q.submit(vec![cmd_b, cmd_a], false);  // B arrives first (index 0)
let result = q.drain(TickId(0));
// BUG: A (arrival_seq=1) sorts before B (arrival_seq=0)
// because A's sort key has source_seq=1 < B's source_seq=MAX
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-engine/src/ingress.rs.md
**Verified lines:** ingress.rs:162-168 (sort key), ingress.rs:94-127 (submit -- no invariant enforcement), murk-ffi/src/command.rs:95-103 (independent mapping)
**Root cause:** The code assumes `source_id` and `source_seq` are either both present or both absent, but that invariant is not enforced at ingress boundaries.
**Suggested fix:** In `IngressQueue::submit()`, normalize: if `cmd.source_id.is_none()`, force `cmd.source_seq = None`. Also harden the sort key in `drain()` so `source_seq` is only considered when `source_id.is_some()`.
