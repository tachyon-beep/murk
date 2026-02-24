# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [x] murk-engine
- [ ] murk-arena
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

`IngressQueue.next_arrival_seq` can overflow and wrap, breaking the strict arrival ordering guarantee used for deterministic command ordering in `crates/murk-engine/src/ingress.rs`.

## Steps to Reproduce

1. Construct an `IngressQueue` and force `next_arrival_seq = u64::MAX` (possible in an in-module test).
2. Submit two otherwise-equal commands back-to-back via `submit(...)`.
3. Drain and observe ordering by `arrival_seq` (`0` sorts before `u64::MAX` after wrap).

## Expected Behavior

Arrival sequence values should remain monotonic (or overflow should be handled explicitly), preserving submission order tie-breaking.

## Actual Behavior

`submit` does:
- `cmd.arrival_seq = self.next_arrival_seq;`
- `self.next_arrival_seq += 1;`

with no overflow guard, so counter wraps and newer commands can sort ahead of older ones.

Evidence:
- Field definition: `crates/murk-engine/src/ingress.rs:63`
- Init to zero: `crates/murk-engine/src/ingress.rs:75`
- Unchecked increment site: `crates/murk-engine/src/ingress.rs:119`
- Assignment site: `crates/murk-engine/src/ingress.rs:120`

## Reproduction Rate

Always (once counter is at/near `u64::MAX`).

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
use murk_core::command::{Command, CommandPayload};
use murk_core::id::{ParameterKey, TickId};

fn make_cmd(priority: u8, expires: u64) -> Command {
    Command {
        payload: CommandPayload::SetParameter {
            key: ParameterKey(0),
            value: 0.0,
        },
        expires_after_tick: TickId(expires),
        source_id: None,
        source_seq: None,
        priority_class: priority,
        arrival_seq: 0,
    }
}

#[test]
fn arrival_seq_wraps_breaks_sorting() {
    let mut q = IngressQueue::new(2);
    q.next_arrival_seq = u64::MAX;
    q.submit(vec![make_cmd(1, 100)], false); // arrival_seq = u64::MAX
    q.submit(vec![make_cmd(1, 100)], false); // wraps -> arrival_seq = 0

    let drained = q.drain(TickId(0));
    let first = drained.commands[0].command.arrival_seq;
    let second = drained.commands[1].command.arrival_seq;

    assert_eq!(
        (first, second),
        (u64::MAX, 0),
        "wrap makes newer command sort ahead of older one"
    );
}
```

## Additional Context

Root cause is unchecked `u64` increment for a monotonic ordering key. Suggested fix: use `checked_add(1)` and define explicit overflow handling (`Err`, reset with epoch extension, or widen counter type), so ordering invariants cannot silently break.