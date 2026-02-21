# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [ ] Lockstep
- [x] RealtimeAsync
- [ ] Both / Unknown

## Summary

Egress `worker_loop_inner` reads `epoch_counter.current()` twice -- once for pinning and once for `execute_task` metadata -- allowing `age_ticks` to be overstated by at least 1 tick if the tick thread advances between reads.

## Steps to Reproduce

1. Start a `RealtimeAsyncWorld` with a fast tick rate (e.g. 1000 Hz).
2. Submit observation tasks that are dispatched to egress workers.
3. Under contention, the tick thread advances between `egress.rs:124` (pin read) and `egress.rs:128` (metadata read).
4. The observation metadata reports `engine_tick` from the second read, which is newer than the pinned epoch.

## Expected Behavior

The `engine_tick` used to compute `age_ticks` in observation metadata should be consistent with the pinned epoch, since the observation is reading data from the pinned snapshot.

## Actual Behavior

`age_ticks` can be inflated by 1 or more ticks, because `engine_tick` comes from a second, unsynchronized read of the epoch counter that may have advanced since the pin.

## Reproduction Rate

Intermittent (requires tick advancement between two atomic reads on the same worker iteration)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [ ] Breaks bit-exact determinism
- [x] Metadata inaccuracy (age_ticks off by 1+)
- [ ] No determinism impact

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// Not easily reproducible in a unit test due to timing dependency.
// The race window is between egress.rs:124 and egress.rs:128.
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-engine/src/egress.rs.md
**Verified lines:** egress.rs:124-128 (two separate `epoch_counter.current()` calls)
**Root cause:** `worker_loop_inner` performs two unsynchronized reads of the epoch counter for one observation task.
**Suggested fix:** Reuse the already-captured `epoch` value (from line 124) at line 128 instead of calling `epoch_counter.current()` again.
