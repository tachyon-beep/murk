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

`SnapshotRing::latest()` can return `None` even when snapshots exist, because it gives up after a bounded retry loop (`capacity` attempts) under overwrite races, violating its documented guarantee.

## Steps to Reproduce

1. Create a `SnapshotRing` with small capacity (e.g., 2).
2. Have a fast producer pushing snapshots continuously.
3. Have a consumer calling `latest()` concurrently.
4. If the producer overwrites the target slot between the consumer's `write_pos` read and lock acquisition on every retry, all `capacity` attempts fail and `latest()` returns `None`.

## Expected Behavior

`latest()` should return `Some(snapshot)` whenever the ring is non-empty, as documented at ring.rs:89-90: "guarantee returning an available snapshot whenever the ring is non-empty."

## Actual Behavior

After `capacity` retries (ring.rs:97), `latest()` returns `None` (ring.rs:115) even though valid snapshots exist in the ring. The doc comment acknowledges this as unlikely but the guarantee is still violated.

## Reproduction Rate

Intermittent (requires fast producer lapping consumer during all retry attempts)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [ ] Breaks bit-exact determinism
- [x] Can cause observation failures (egress worker returns "no snapshot available" error)
- [ ] No determinism impact

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// Requires a timing race between producer and consumer threads.
// With capacity=2 and a fast producer, the consumer's retry
// window is very small, making the race more likely.
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-engine/src/ring.rs.md
**Verified lines:** ring.rs:89-90 (guarantee comment), ring.rs:97 (bounded retry), ring.rs:105-110 (tag mismatch -> continue), ring.rs:115 (return None)
**Root cause:** The bounded retry strategy assumes consumers can catch up within `capacity` attempts, but this assumption fails under extreme scheduling/throughput variance.
**Suggested fix:** After retry exhaustion, scan all slots and return the highest valid `(tag, snapshot)` as a fallback. Only return `None` for the truly empty ring (`write_pos == 0`).
