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
- [x] RealtimeAsync
- [ ] Both / Unknown

## Summary

`SetField` in `realtime_async.rs` is reported as adding a “second heat source,” but it is overwritten in the same tick by a full-field propagator write, so the command has no observable effect.

## Steps to Reproduce

1. Run `cargo run --example realtime_async`.
2. Wait for the `"Submitting SetField command at (0, 0) — second heat source..."` step.
3. Compare post-command observations/snapshot to expected behavior at `(0,0)`; the cell is not forced to `10.0` as a persistent or even one-tick source.

## Expected Behavior

A `SetField { coord: [0,0], field_id: HEAT, value: 10.0 }` command should materially affect subsequent observed heat values at `(0,0)` (as implied by “second heat source”).

## Actual Behavior

The command is accepted, but the propagator recomputes and fully overwrites `HEAT` from `reads_previous()` values in the same tick, erasing the staged command update before publish.

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

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// From realtime_async example context:
let cmd = Command {
    payload: CommandPayload::SetField {
        coord: smallvec![0, 0],
        field_id: HEAT,
        value: 10.0,
    },
    expires_after_tick: TickId(u64::MAX),
    source_id: None,
    source_seq: None,
    priority_class: 1,
    arrival_seq: 0,
};

let receipts = world.submit_commands(vec![cmd])?;
assert!(receipts[0].accepted);

std::thread::sleep(std::time::Duration::from_millis(100));
let snap = world.latest_snapshot().unwrap();
let heat = snap.read_field(HEAT).unwrap();

// Fails expectation of "second heat source":
assert_ne!(heat[0], 10.0);
```

## Additional Context

Evidence in target file:

- `/home/john/murk/crates/murk-engine/examples/realtime_async.rs:75`-`77` declares `HEAT` as `WriteMode::Full`.
- `/home/john/murk/crates/murk-engine/examples/realtime_async.rs:71`-`73` reads only `reads_previous()` for `HEAT`.
- `/home/john/murk/crates/murk-engine/examples/realtime_async.rs:266`-`272` submits `SetField` to that same `HEAT` field and describes it as a “second heat source.”

Execution-order evidence in engine:

- `/home/john/murk/crates/murk-engine/src/tick.rs:261`-`274` applies `SetField` into staging before propagators run.
- `/home/john/murk/crates/murk-engine/src/tick.rs:291`-`352` then runs propagators, and this propagator full-writes `HEAT`, overriding the command value.

Suggested fix:
- Use a separate command-only source field (as done in `quickstart.rs`) and have the propagator read it, or change write semantics/read routing so command updates are intentionally incorporated.