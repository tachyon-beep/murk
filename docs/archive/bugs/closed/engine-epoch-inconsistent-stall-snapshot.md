# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [ ] Lockstep
- [x] RealtimeAsync
- [ ] Both / Unknown

## Summary

Stall detection reads `is_pinned()` and `pin_start_ns()` as two independent atomics without a consistent snapshot, so concurrent unpin/repin cycles can produce mismatched pairs that falsely trigger cancellation or force-unpin of healthy workers.

## Steps to Reproduce

1. Start a `RealtimeAsyncWorld` with multiple egress workers under heavy observation load.
2. Workers rapidly pin/unpin as they process back-to-back tasks.
3. The tick thread's `check_stalled_workers()` reads `is_pinned()` (line 241), sees `true` from the old pin cycle.
4. Between that read and the `pin_start_ns()` read (line 248), the worker unpins and re-pins with a new `pin_start_ns`.
5. The tick thread computes `hold_ns` from the new (very recent) `pin_start_ns` against the old pin's `is_pinned()` state, OR reads a stale `pin_start_ns` from a previous cycle when the current pin is new, producing an inflated `hold_ns`.

## Expected Behavior

Stall detection should read a consistent pair of (pinned, pin_start_ns) from the same pin cycle, so hold duration is computed accurately.

## Actual Behavior

Under concurrent unpin/repin, the tick thread can observe a mismatched (pinned_from_cycle_N, pin_start_ns_from_cycle_M) pair, potentially inflating `hold_ns` and triggering false-positive cancellation/force-unpin.

## Reproduction Rate

Intermittent (requires specific timing between worker and tick thread)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [ ] Breaks bit-exact determinism
- [x] Can cause spurious observation failures via false force-unpin
- [ ] No determinism impact

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// Requires multi-threaded timing race - not easily unit-testable.
// The race window is between tick_thread.rs:241 (is_pinned check)
// and tick_thread.rs:248 (pin_start_ns read).
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-engine/src/epoch.rs.md
**Verified lines:** epoch.rs:110-113 (separate stores in pin()), epoch.rs:125-137 (separate readers), tick_thread.rs:241-258 (two independent reads in stall check)
**Root cause:** `WorkerEpoch` state is split across multiple atomics without a linearizable read path, so readers can observe mixed-generation values during concurrent transitions.
**Suggested fix:** Add a single `WorkerEpoch` read API that returns a consistent pin snapshot (e.g., load pinned, load pin_start_ns, reload pinned and retry until stable), and make stall detection use that API.
