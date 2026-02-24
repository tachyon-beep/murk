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

`WorkerEpoch::pin_snapshot()` can return an inconsistent `(epoch, pin_start_ns)` pair under ABA (unpinned -> repinned to same epoch), causing false stalled-worker detection and incorrect forced unpin/cancel behavior.

## Steps to Reproduce

1. Keep a worker pinned at epoch `E` long enough to create an old `pin_start_ns`.
2. Concurrently run `pin_snapshot()` while another thread does `unpin(); pin(E);` (same epoch value).
3. Observe `pin_snapshot()` occasionally return `Some((E, old_start))` even though the worker has been freshly repinned.

## Expected Behavior

`pin_snapshot()` should only return a pair from one logical pin session (no stale `pin_start_ns` for the current pinned epoch).

## Actual Behavior

`pin_snapshot()` validates consistency only by checking `epoch1 == epoch2`; this misses ABA when the epoch value returns to the same value, so stale `pin_start_ns` can be returned as if current.

## Reproduction Rate

Intermittent (timing-dependent race).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

## Determinism Impact

- [ ] Bug is deterministic (same inputs always reproduce it)
- [x] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```text
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_engine::epoch::WorkerEpoch;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::Duration;

// Intermittent race reproducer: run multiple times.
fn main() {
    let w = Arc::new(WorkerEpoch::new(0));
    w.pin(7);
    thread::sleep(Duration::from_millis(50)); // make an "old" pin_start

    let run = Arc::new(AtomicBool::new(true));
    let w2 = Arc::clone(&w);
    let run2 = Arc::clone(&run);

    let t = thread::spawn(move || {
        while run2.load(Ordering::Relaxed) {
            w2.unpin();
            w2.pin(7); // ABA: same epoch value
        }
    });

    let mut stale_seen = false;
    for _ in 0..2_000_000 {
        if let Some((_e, snap_start)) = w.pin_snapshot() {
            let now_start = w.pin_start_ns();
            if snap_start < now_start {
                stale_seen = true;
                break;
            }
        }
    }

    run.store(false, Ordering::Relaxed);
    let _ = t.join();
    assert!(stale_seen, "expected stale snapshot under ABA race");
}
```

## Additional Context

Evidence in target file:

- `pin_snapshot()` accepts snapshot when only `epoch1 == epoch2` (`crates/murk-engine/src/epoch.rs:168`, `crates/murk-engine/src/epoch.rs:173`, `crates/murk-engine/src/epoch.rs:174`, `crates/murk-engine/src/epoch.rs:175`).
- `pin()` writes `pin_start_ns` then `pinned` (`crates/murk-engine/src/epoch.rs:111`, `crates/murk-engine/src/epoch.rs:113`).
- `unpin()` writes sentinel then later caller can repin same epoch (`crates/murk-engine/src/epoch.rs:119`).

Impact path:

- Stall logic consumes `pin_snapshot()` and can request cancel / force unpin based on `hold_ns` (`crates/murk-engine/src/tick_thread.rs:273`, `crates/murk-engine/src/tick_thread.rs:279`, `crates/murk-engine/src/tick_thread.rs:285`).

Root cause: ABA is undetectable with only `(epoch-before, epoch-after)` equality.  
Suggested fix: add a dedicated sequence/version counter (true seqlock pattern) or validate both epoch and timestamp stability with an update protocol that prevents ABA acceptance.