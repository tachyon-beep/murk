# Epoch-Based Reclamation Design — `murk-engine` (RealtimeAsync)

**Status:** Design document for WP-12
**Scope:** Epoch reclamation, shutdown state machine, adaptive backoff
**HLD refs:** §8.3, §7.2, §17, Decision E, Decision J, R-ACT-2, P-1, P-3

---

## Table of Contents

1. [Overview](#1-overview)
2. [Why Custom, Not crossbeam-epoch](#2-why-custom-not-crossbeam-epoch)
3. [Core Types](#3-core-types)
4. [Epoch Lifecycle](#4-epoch-lifecycle)
5. [Reclamation Protocol](#5-reclamation-protocol)
6. [Stalled Worker Detection and Teardown](#6-stalled-worker-detection-and-teardown)
7. [Thread Pool](#7-thread-pool)
8. [Memory Bound Proof](#8-memory-bound-proof)
9. [Shutdown State Machine](#9-shutdown-state-machine)
10. [Adaptive Backoff Parameters](#10-adaptive-backoff-parameters)
11. [Integration Points](#11-integration-points)
12. [Testing Strategy](#12-testing-strategy)

---

## 1. Overview

RealtimeAsync mode runs a TickEngine on a dedicated thread at ~60Hz. Each tick
publishes a new snapshot (arena generation) into a ring buffer. An egress thread
pool concurrently reads these snapshots to execute ObsPlans. The central
problem: **when can an arena generation be reclaimed?**

Reference-counting is the obvious answer, but `Arc`/`AtomicUsize` refcount
traffic on every ObsPlan acquire/release creates cache-line ping-pong under
high obs throughput (HLD §8.2, I-14). Epoch-based reclamation avoids per-object
atomic traffic: workers pin an epoch before access and unpin after, and the
reclaimer checks a single `min(pinned)` to determine reclaimable generations.

```
TickEngine thread                    Egress pool (N workers)
   │                                    │
   │  publish(gen G)                    │
   │──────────────────►  ring[G % K]    │
   │                                    │  worker pins epoch = G
   │  publish(gen G+1)                  │  ... execute ObsPlan ...
   │──────────────────►  ring[(G+1)%K]  │  worker unpins (epoch = MAX)
   │                                    │
   │  reclaim check:                    │
   │  min_pinned = min(worker.pinned)   │
   │  reclaim gens < min_pinned - 1     │
```

---

## 2. Why Custom, Not crossbeam-epoch

`crossbeam-epoch` is a general-purpose epoch-based reclamation library. We
don't use it because Murk's arena has specific semantics that make a custom
mechanism simpler and more efficient:

| Property | crossbeam-epoch | Custom (this design) |
|----------|----------------|---------------------|
| Scope | Arbitrary heap objects | Arena generations (bounded count) |
| Reclamation unit | Individual allocations via `defer_destroy` | Whole generation (bump-pointer reset) |
| Epoch granularity | Global, advanced by any thread | Per-tick, advanced only by TickEngine |
| Pin/unpin | `Guard` RAII with thread-local collector | Single `AtomicU64` per worker |
| Stalled detection | None (assumes cooperative progress) | Built-in: `last_quiesce` + timeout |
| Code size | ~2,500 lines + dependencies | ~200 lines |

The custom mechanism maps 1:1 to the arena's generation model: one epoch per
tick, one reclamation unit per generation, bounded ring buffer. crossbeam-epoch
would require adapting arena generations into its deferred-destruction model —
added complexity for no benefit.

---

## 3. Core Types

```rust
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

/// Global epoch counter, incremented by TickEngine at each snapshot publication.
/// Monotonically increasing. Never wraps in practice (u64 overflow at 60Hz
/// would take ~9.7 billion years).
pub(crate) struct EpochCounter {
    current: AtomicU64,
}

impl EpochCounter {
    pub fn new() -> Self {
        Self { current: AtomicU64::new(0) }
    }

    /// Called by TickEngine after publishing a new snapshot.
    /// Returns the new epoch value.
    pub fn advance(&self) -> u64 {
        self.current.fetch_add(1, Ordering::Release) + 1
    }

    pub fn current(&self) -> u64 {
        self.current.load(Ordering::Acquire)
    }
}

/// Per-worker epoch state. Each egress worker holds one of these.
/// Padded to avoid false sharing (each on its own cache line).
#[repr(align(128))]
pub(crate) struct WorkerEpoch {
    /// The epoch this worker is currently pinned to.
    /// u64::MAX means "unpinned" (not holding any generation).
    pinned: AtomicU64,

    /// Monotonic timestamp of the last time this worker unpinned.
    /// Used for stalled-worker detection.
    last_quiesce_ns: AtomicU64,

    /// Cooperative cancellation flag. Set by TickEngine/shutdown to
    /// request this worker abandon its current ObsPlan execution.
    cancel: AtomicBool,

    /// Worker index (for diagnostics).
    worker_id: u32,
}

/// Sentinel value meaning "this worker is not pinned to any epoch."
const EPOCH_UNPINNED: u64 = u64::MAX;

impl WorkerEpoch {
    pub fn new(worker_id: u32) -> Self {
        Self {
            pinned: AtomicU64::new(EPOCH_UNPINNED),
            last_quiesce_ns: AtomicU64::new(0),
            cancel: AtomicBool::new(false),
            worker_id,
        }
    }

    /// Pin this worker to the given epoch before accessing a snapshot.
    /// MUST be called before resolving any FieldHandle from the snapshot.
    pub fn pin(&self, epoch: u64) {
        self.pinned.store(epoch, Ordering::Release);
    }

    /// Unpin this worker after finishing with the snapshot.
    /// Updates the quiescence timestamp.
    pub fn unpin(&self) {
        self.pinned.store(EPOCH_UNPINNED, Ordering::Release);
        // Monotonic clock, converted to nanos for AtomicU64 storage.
        let now_ns = monotonic_nanos();
        self.last_quiesce_ns.store(now_ns, Ordering::Release);
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned.load(Ordering::Acquire) != EPOCH_UNPINNED
    }

    pub fn pinned_epoch(&self) -> u64 {
        self.pinned.load(Ordering::Acquire)
    }

    pub fn last_quiesce_ns(&self) -> u64 {
        self.last_quiesce_ns.load(Ordering::Acquire)
    }

    /// Check if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    /// Request cancellation (called by TickEngine or shutdown).
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }

    /// Clear cancellation flag (called when worker is restarted/recycled).
    pub fn clear_cancel(&self) {
        self.cancel.store(false, Ordering::Release);
    }
}

/// Configuration for the epoch reclamation subsystem.
pub struct EpochConfig {
    /// Ring buffer capacity (number of generations retained).
    /// Default: 8. Range: 2..=64.
    pub ring_capacity: usize,

    /// Maximum time a worker may hold an epoch before being considered stalled.
    /// Default: 100ms (6 ticks at 60Hz).
    pub max_epoch_hold: Duration,

    /// Grace period after cancellation before treating epoch as force-unpinned.
    /// Default: 10ms.
    pub cancel_grace: Duration,

    /// Number of egress worker threads.
    /// Default: num_cpus / 2, clamped to [2, 16].
    pub worker_count: usize,
}

impl Default for EpochConfig {
    fn default() -> Self {
        let cpus = num_cpus::get();
        Self {
            ring_capacity: 8,
            max_epoch_hold: Duration::from_millis(100),
            cancel_grace: Duration::from_millis(10),
            worker_count: (cpus / 2).clamp(2, 16),
        }
    }
}
```

### Cache-Line Padding Rationale

`WorkerEpoch` is `#[repr(align(128))]` — two cache lines. This prevents false
sharing between adjacent workers. The TickEngine reads all `pinned` fields
during reclamation checks; without padding, adjacent workers' `pin`/`unpin`
writes would invalidate the TickEngine's cache line, adding ~50ns per
invalidation under contention.

128 bytes (not 64) because some architectures (e.g., Apple M-series) use
128-byte cache lines, and `WorkerEpoch` contains 3 atomics + 1 u32 (25 bytes
minimum), which fits comfortably.

---

## 4. Epoch Lifecycle

### Worker Side (Egress Thread)

Each egress worker follows this protocol for every ObsPlan execution:

```rust
fn execute_obs_request(
    worker: &WorkerEpoch,
    ring: &SnapshotRing,
    plan: &ObsPlan,
    buffer: &mut [f32],
    mask: &mut [u8],
) -> Result<ObsResult, ObsError> {
    // 1. Select snapshot (latest, or specific tick if requested).
    let (snapshot, epoch) = ring.select_snapshot(/* ... */)?;

    // 2. Pin to this epoch BEFORE accessing any field data.
    worker.pin(epoch);

    // 3. Execute ObsPlan. Check cancellation between region iterations.
    let result = plan.execute_with_cancel(snapshot, buffer, mask, || {
        worker.is_cancelled()
    });

    // 4. ALWAYS unpin, even on error or cancellation.
    worker.unpin();

    match result {
        Ok(obs_result) => Ok(obs_result),
        Err(e) if e.is_cancelled() => Err(ObsError::ExecutionFailed {
            reason: ErrorReason::WorkerCancelled,
        }),
        Err(e) => Err(e),
    }
}
```

**Critical invariant:** `unpin()` MUST be called on every exit path. The
`pin`/`unpin` pair is NOT RAII (no `Guard` type) because the worker loop
already handles this — adding a `Guard` would create a borrow conflict with
the mutable buffer references. Instead, the protocol is enforced by the
single call site in the worker loop.

### TickEngine Side (Publisher)

```rust
fn publish_tick(&mut self) {
    // 1. Publish snapshot to ring buffer.
    let evicted = self.ring.push(self.staging.finalize());

    // 2. Advance global epoch.
    let new_epoch = self.epoch_counter.advance();

    // 3. If a generation was evicted from the ring, mark it for reclamation.
    if let Some(evicted_gen) = evicted {
        self.pending_reclaim.push(evicted_gen);
    }

    // 4. Run reclamation check.
    self.try_reclaim();
}
```

---

## 5. Reclamation Protocol

### Determining Reclaimable Generations

```rust
impl EpochReclaimer {
    /// Compute the minimum pinned epoch across all workers.
    /// Returns EPOCH_UNPINNED if no worker is pinned (all reclaimable).
    fn min_pinned_epoch(&self) -> u64 {
        self.workers
            .iter()
            .map(|w| w.pinned_epoch())
            .min()
            .unwrap_or(EPOCH_UNPINNED)
    }

    /// Attempt to reclaim pending generations.
    /// Called by TickEngine after each snapshot publication.
    fn try_reclaim(&mut self) {
        let min_pinned = self.min_pinned_epoch();

        // Generations older than (min_pinned - 1) are safe to reclaim.
        // The "-1" provides a one-generation grace period: a worker that
        // just loaded the epoch counter but hasn't called pin() yet is
        // still safe because it will pin to current or current-1.
        let reclaim_threshold = min_pinned.saturating_sub(1);

        self.pending_reclaim.retain(|gen| {
            if gen.epoch < reclaim_threshold {
                // Safe to reclaim: no worker can possibly access this generation.
                gen.arena_segment.reclaim(); // bump-pointer reset, O(1)
                false // remove from pending list
            } else {
                true // keep in pending list
            }
        });
    }
}
```

### Ring Buffer Eviction Priority

The ring buffer has a fixed capacity `K` (default 8). When a new snapshot is
published and the ring is full, the oldest generation is evicted regardless of
worker pin state:

```
Ring buffer eviction takes precedence over epoch pinning.
```

This is HLD §8.3 requirement #3. Workers holding an evicted generation receive
`PLAN_INVALIDATED` on their next field resolve attempt:

```rust
impl SnapshotRing {
    /// Push a new snapshot. Returns the evicted generation if ring was full.
    fn push(&mut self, snapshot: Snapshot) -> Option<EvictedGeneration> {
        let evicted = if self.len() >= self.capacity {
            Some(self.pop_oldest())
        } else {
            None
        };
        self.buf[self.write_pos % self.capacity] = snapshot;
        self.write_pos += 1;
        evicted
    }
}
```

When a worker is pinned to an evicted epoch, the `ReadArena::resolve()` call
will detect the generation mismatch (the `FieldHandle`'s generation no longer
exists in the arena) and return `Err(ArenaError::GenerationEvicted)`. The
worker translates this to `ObsError::PlanInvalidated`.

**P-1 satisfaction:** The worker returns `PLAN_INVALIDATED`, not silence. The
system always returns a response — invalidation is a response with metadata.

---

## 6. Stalled Worker Detection and Teardown

A worker is "stalled" if it holds a pinned epoch for longer than
`max_epoch_hold`. This can happen due to:

- Unexpectedly large ObsPlan execution (100K+ cell region)
- Thread scheduling delay under system load
- Bug in ObsPlan execution (infinite loop in region iterator)

### Detection

```rust
impl EpochReclaimer {
    /// Check for stalled workers. Called by TickEngine periodically
    /// (every tick, or every N ticks if overhead is a concern).
    fn detect_stalled_workers(&self) -> Vec<u32> {
        let now_ns = monotonic_nanos();
        let threshold_ns = self.config.max_epoch_hold.as_nanos() as u64;

        self.workers
            .iter()
            .filter(|w| {
                if !w.is_pinned() {
                    return false;
                }
                // Worker is pinned. Check how long since last quiescence.
                let last_q = w.last_quiesce_ns();
                // If last_quiesce_ns is 0, worker has never quiesced (just started).
                // Use the worker's pin time as baseline.
                now_ns.saturating_sub(last_q) > threshold_ns
            })
            .map(|w| w.worker_id)
            .collect()
    }
}
```

### Teardown Sequence

```
1. Set cancellation flag     ──►  worker.request_cancel()
2. Wait cancel_grace (10ms)  ──►  worker checks flag between region iterations
3. Force-unpin               ──►  treat epoch as EPOCH_UNPINNED for reclamation
4. Log stall event           ──►  worker_id, held_epoch, duration
```

```rust
impl EpochReclaimer {
    /// Tear down a stalled worker. Returns the epoch that was force-released.
    fn teardown_stalled_worker(&self, worker_id: u32) -> Option<u64> {
        let worker = &self.workers[worker_id as usize];
        let held_epoch = worker.pinned_epoch();

        if held_epoch == EPOCH_UNPINNED {
            return None; // Worker recovered on its own.
        }

        // Step 1: Request cooperative cancellation.
        worker.request_cancel();

        // Step 2: Wait for grace period.
        // (In practice, this is checked on the next reclamation cycle,
        // not via sleep — the TickEngine doesn't block.)

        // Step 3: If still pinned after grace, force-unpin for reclamation.
        // The worker's epoch is treated as EPOCH_UNPINNED when computing
        // min_pinned. The worker itself may still be running, but its
        // generation is now eligible for reclamation.
        //
        // IMPORTANT: We do NOT write to worker.pinned — the worker thread
        // owns that field. Instead, the reclaimer maintains a "force_unpinned"
        // set that is consulted during min_pinned computation.

        log::warn!(
            "Stalled worker {} force-unpinned (held epoch {} for {:?})",
            worker_id,
            held_epoch,
            Duration::from_nanos(
                monotonic_nanos() - worker.last_quiesce_ns()
            ),
        );

        Some(held_epoch)
    }
}
```

### Cancellation Granularity

The cancellation flag is checked **between region iterations** in ObsPlan
execution, not between individual cell accesses. This is an explicit design
choice from HLD §8.3:

```rust
// Inside ObsPlan::execute_with_cancel
for region in &self.regions {
    // Check cancellation between regions, not per-cell.
    if cancel_check() {
        return Err(ObsError::cancelled());
    }
    for coord in region.iter() {
        // Branch-free gather — no cancellation check here.
        buffer[idx] = snapshot.read_field(field)?[offset];
        idx += 1;
    }
}
```

**Worst-case cancellation latency:** ~1ms for a large region (100K cells at
~10ns/cell). Acceptable because:
- `cancel_grace` is 10ms (10× the worst case)
- The inner gather loop is branch-free (no per-cell branch overhead)
- Region count is typically 5-20 per ObsPlan

### Force-Unpin vs Worker.pinned

The reclaimer does NOT directly write to `worker.pinned`. That would create a
data race (worker thread reads its own `pinned` to decide what it's accessing).
Instead, the reclaimer maintains a separate `force_unpinned: IndexSet<u32>`
that overrides `min_pinned` computation:

```rust
fn min_pinned_epoch_with_overrides(&self) -> u64 {
    self.workers
        .iter()
        .filter(|w| !self.force_unpinned.contains(&w.worker_id))
        .map(|w| w.pinned_epoch())
        .min()
        .unwrap_or(EPOCH_UNPINNED)
}
```

When the worker eventually unpins (either cooperatively or by completing its
work), it clears its own `pinned` field and the reclaimer removes it from the
override set.

### P-1 Satisfaction

A stalled worker that is torn down returns `ObsError::ExecutionFailed` with
reason `WORKER_STALLED` to its caller. The system always returns — individual
worker failures are reported, never swallowed (HLD §8.3, P-1).

---

## 7. Thread Pool

### Configuration

```rust
pub struct EgressPool {
    workers: Vec<JoinHandle<()>>,
    epochs: Arc<[WorkerEpoch]>,
    task_queue: crossbeam_channel::Sender<ObsTask>,
    config: EpochConfig,
}
```

| Parameter | Default | Range | Rationale |
|-----------|---------|-------|-----------|
| `worker_count` | `num_cpus / 2` | `2..=16` | Egress is I/O-bound (memory reads). Half the cores leaves room for TickEngine + application threads. |
| Minimum 2 | — | — | One worker can be stalled while the other serves requests (P-1). |
| Maximum 16 | — | — | Beyond 16, memory bandwidth saturates and cache contention dominates. |

### Lifecycle

Workers are spawned at `RealtimeAsyncWorld::new()`:

```rust
impl EgressPool {
    fn new(config: &EpochConfig, ring: Arc<SnapshotRing>) -> Self {
        let epochs: Arc<[WorkerEpoch]> = (0..config.worker_count)
            .map(|i| WorkerEpoch::new(i as u32))
            .collect::<Vec<_>>()
            .into();

        let (tx, rx) = crossbeam_channel::bounded(config.worker_count * 4);

        let workers: Vec<JoinHandle<()>> = (0..config.worker_count)
            .map(|i| {
                let epoch = epochs.clone();
                let rx = rx.clone();
                let ring = ring.clone();
                std::thread::Builder::new()
                    .name(format!("murk-egress-{i}"))
                    .spawn(move || {
                        worker_loop(&epoch[i], &rx, &ring);
                    })
                    .expect("failed to spawn egress worker")
            })
            .collect();

        Self { workers, epochs, task_queue: tx, config: config.clone() }
    }
}
```

Workers are joined at shutdown (see §9).

---

## 8. Memory Bound Proof

**Claim:** At any time, the number of live (non-reclaimable) arena generations
is bounded by `K + S`, where `K` is the ring buffer capacity and `S` is the
number of stalled workers.

**Proof:**

1. The ring buffer holds at most `K` generations. Eviction is immediate when
   capacity is exceeded (§5, ring buffer eviction priority).

2. A non-stalled worker pins an epoch for at most `max_epoch_hold` duration.
   During this window, the pinned generation is not reclaimable. However, the
   worker's pinned epoch must be one of the `K` generations in the ring (a
   worker can only pin to epochs it received from the ring). Therefore,
   non-stalled workers do not increase the generation count beyond `K`.

3. A stalled worker's epoch may have been evicted from the ring but the arena
   segment is not yet reclaimed (because the worker is still pinned). After
   teardown, the epoch is force-unpinned and the generation becomes reclaimable.
   At most `S` workers can be in the stalled state simultaneously.

4. Therefore: `live_generations ≤ K + S`.

**Concrete bounds (default configuration):**

| Parameter | Value |
|-----------|-------|
| K (ring capacity) | 8 |
| S (max stalled workers = pool size) | 8 (worst case with pool size 8) |
| Max live generations | 16 |
| Reference profile per generation | ~400KB (10K cells × 5 fields × 8 bytes) |
| **Max arena memory** | **~6.4MB** |

With the default pool size of `num_cpus/2` clamped to `[2, 16]`:
- 4-core machine: pool=2, max live = 8 + 2 = **10 generations**
- 8-core machine: pool=4, max live = 8 + 4 = **12 generations**
- 16-core machine: pool=8, max live = 8 + 8 = **16 generations**
- 32-core machine: pool=16, max live = 8 + 16 = **24 generations**

All well within acceptable memory bounds for the reference profile.

**Note:** This bound assumes `pending_reclaim` is processed every tick. If the
TickEngine skips reclamation checks (it shouldn't), pending generations
accumulate. The implementation MUST run `try_reclaim()` on every tick.

---

## 9. Shutdown State Machine

RealtimeAsync shutdown is a 4-state machine that reuses the epoch reclamation
infrastructure for safe teardown. Total worst-case time: ≤300ms.

### State Diagram

```
                ┌─────────┐
                │ Running │
                └────┬────┘
                     │ drop() or explicit shutdown()
                     ▼
              ┌──────────────┐
              │   Draining   │◄── reject ingress with SHUTTING_DOWN
              │ (≤33ms)      │    TickEngine completes current tick, stops
              └──────┬───────┘
                     │ TickEngine stopped (or timeout)
                     ▼
              ┌──────────────┐
              │  Quiescing   │◄── signal workers via cancel flag
              │ (≤200ms)     │    wait for all epochs unpinned
              └──────┬───────┘
                     │ all epochs unpinned (or timeout + force-unpin)
                     ▼
              ┌──────────────┐
              │   Dropped    │◄── join threads, drop arenas
              │ (≤10ms)      │
              └──────────────┘
```

### Types

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShutdownState {
    Running,
    Draining,
    Quiescing,
    Dropped,
}

/// Result of a shutdown operation.
#[derive(Debug)]
pub struct ShutdownResult {
    /// Time spent in each phase: [draining_ms, quiescing_ms, join_ms].
    pub phase_times: [u64; 3],

    /// Number of workers that required stall teardown during shutdown.
    pub stalled_workers: usize,

    /// Number of threads that didn't join within the timeout and were detached.
    pub leaked_threads: usize,
}
```

### Phase 1: Running → Draining

Triggered by `drop()` or explicit `shutdown()`.

```rust
fn begin_draining(&mut self) {
    self.state = ShutdownState::Draining;

    // 1. Close ingress: new commands rejected with SHUTTING_DOWN.
    self.ingress.close();

    // 2. Signal TickEngine to finish current tick and stop.
    self.tick_engine_stop.store(true, Ordering::Release);

    // 3. Wait for TickEngine to acknowledge stop.
    //    Timeout: 2× tick budget = 2 × 16.67ms ≈ 33ms.
    let deadline = Instant::now() + Duration::from_millis(33);
    while !self.tick_engine_stopped.load(Ordering::Acquire) {
        if Instant::now() >= deadline {
            // Timeout: TickEngine is stuck. Force-abandon staging.
            // This is equivalent to a tick rollback — no partial state.
            log::error!("TickEngine drain timeout; forcing abandon");
            break;
        }
        std::thread::yield_now();
    }
}
```

**Invariant:** After draining, no new snapshots will be published. The ring
buffer is frozen.

### Phase 2: Draining → Quiescing

```rust
fn begin_quiescing(&mut self) {
    self.state = ShutdownState::Quiescing;

    // 1. Signal ALL egress workers to cancel.
    for worker in self.epochs.iter() {
        worker.request_cancel();
    }

    // 2. Close the task queue so workers exit their recv loop.
    drop(self.task_sender.take());

    // 3. Wait for all workers to unpin (release epoch references).
    //    Timeout: 2× max_epoch_hold = 2 × 100ms = 200ms.
    let deadline = Instant::now() + self.config.max_epoch_hold * 2;

    loop {
        let all_unpinned = self.epochs.iter().all(|w| !w.is_pinned());
        if all_unpinned {
            break;
        }
        if Instant::now() >= deadline {
            // Force-unpin remaining stalled workers.
            let stalled: Vec<_> = self.epochs
                .iter()
                .filter(|w| w.is_pinned())
                .map(|w| w.worker_id)
                .collect();
            for id in &stalled {
                log::error!("Worker {} still pinned at quiesce timeout", id);
                self.force_unpinned.insert(*id);
            }
            self.stalled_count = stalled.len();
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}
```

**Invariant:** After quiescing, no thread holds a reference into any arena
generation. The arena is safe to drop.

### Phase 3: Quiescing → Dropped

```rust
fn finalize_drop(&mut self) -> ShutdownResult {
    self.state = ShutdownState::Dropped;
    let mut leaked = 0;

    // 1. Join TickEngine thread.
    if let Some(handle) = self.tick_thread.take() {
        match handle.join_timeout(Duration::from_millis(10)) {
            Ok(_) => {}
            Err(_) => {
                log::error!("TickEngine thread did not join; detaching");
                leaked += 1;
                // Thread is detached (handle dropped without join).
            }
        }
    }

    // 2. Join egress worker threads.
    for handle in self.worker_threads.drain(..) {
        match handle.join_timeout(Duration::from_millis(10)) {
            Ok(_) => {}
            Err(_) => {
                log::error!("Egress worker did not join; detaching");
                leaked += 1;
            }
        }
    }

    // 3. Drop ring buffer and arenas.
    //    Safe because all epoch references are released (step 2 above).
    drop(self.ring.take());
    drop(self.arena.take());

    ShutdownResult {
        phase_times: [
            self.drain_time_ms,
            self.quiesce_time_ms,
            self.join_time_ms,
        ],
        stalled_workers: self.stalled_count,
        leaked_threads: leaked,
    }
}
```

**Note on `join_timeout`:** `std::thread::JoinHandle` does not have a native
`join_timeout`. Implementation options:
1. Use a `Condvar`-based signal from each thread's exit path.
2. Use `thread::park_timeout` + polling.
3. Accept that `join()` may block; the 10ms budget is a soft target.

The recommended approach is (1): each thread signals a shared `Condvar` on exit,
and `finalize_drop` waits on the `Condvar` with timeout.

### Interaction with tick_disabled (Decision J)

If `tick_disabled` is set (3 consecutive rollbacks), the TickEngine thread is
still alive but not executing ticks. The shutdown sequence proceeds normally:

1. **Draining:** TickEngine is already idle (no tick in progress). Drain
   completes immediately.
2. **Quiescing:** Workers may be serving the last good snapshot. Cancel as
   usual.
3. **Dropped:** Join threads as usual.

The `tick_disabled` flag does not interfere with shutdown.

### C ABI and Python Integration

```
murk_destroy(world)       →  shutdown().wait(DEFAULT_TIMEOUT)  [blocking]
Python __exit__           →  murk_destroy() with GIL released
```

The C ABI `murk_destroy()` is a blocking call that runs the full shutdown
state machine and returns the `ShutdownResult` (or a timeout error code).
Python releases the GIL via `py.allow_threads()` for the duration.

---

## 10. Adaptive Backoff Parameters

Adaptive backoff prevents stale-action rejection oscillation (HLD R-ACT-2,
stress test §23 #17). When too many commands are rejected for staleness, the
allowed `max_tick_skew` widens temporarily, then decays back to normal.

### Configuration

```rust
/// Adaptive backoff configuration. All parameters configurable via WorldConfig.
pub struct AdaptiveBackoffConfig {
    /// Maximum allowed tick skew at startup.
    /// Default: 2 ticks.
    pub initial_max_skew: u64,

    /// Multiplicative factor when rejection rate exceeds threshold.
    /// Default: 1.5x.
    pub backoff_factor: f64,

    /// Hard ceiling for max_tick_skew (prevents unbounded growth).
    /// Default: 10 ticks.
    pub max_skew_cap: u64,

    /// Decay rate: reduce effective_max_skew by 1 tick per this many
    /// ticks of no rejections.
    /// Default: 60 ticks (1 second at 60Hz).
    pub decay_interval: u64,

    /// Rejection rate threshold that triggers backoff.
    /// Measured as: rejected_stale / total_commands over the last
    /// `decay_interval` ticks.
    /// Default: 0.20 (20%).
    pub rejection_rate_threshold: f64,
}

impl Default for AdaptiveBackoffConfig {
    fn default() -> Self {
        Self {
            initial_max_skew: 2,
            backoff_factor: 1.5,
            max_skew_cap: 10,
            decay_interval: 60,
            rejection_rate_threshold: 0.20,
        }
    }
}
```

### State Machine

```rust
pub(crate) struct AdaptiveBackoff {
    config: AdaptiveBackoffConfig,

    /// Current effective max_tick_skew.
    effective_max_skew: u64,

    /// Rolling window counters (last `decay_interval` ticks).
    window_total_commands: u64,
    window_stale_rejections: u64,

    /// Ticks since last rejection (for decay).
    ticks_since_rejection: u64,
}

impl AdaptiveBackoff {
    pub fn new(config: AdaptiveBackoffConfig) -> Self {
        Self {
            effective_max_skew: config.initial_max_skew,
            window_total_commands: 0,
            window_stale_rejections: 0,
            ticks_since_rejection: 0,
            config,
        }
    }

    /// Called by TickEngine at each tick with the tick's command statistics.
    pub fn update(&mut self, total_commands: u64, stale_rejections: u64) {
        self.window_total_commands += total_commands;
        self.window_stale_rejections += stale_rejections;

        if stale_rejections > 0 {
            self.ticks_since_rejection = 0;
        } else {
            self.ticks_since_rejection += 1;
        }

        // Check rejection rate over the window.
        if self.window_total_commands > 0 {
            let rate = self.window_stale_rejections as f64
                / self.window_total_commands as f64;

            if rate > self.config.rejection_rate_threshold {
                // Backoff: widen the skew allowance.
                self.effective_max_skew = ((self.effective_max_skew as f64
                    * self.config.backoff_factor)
                    .ceil() as u64)
                    .min(self.config.max_skew_cap);
            }
        }

        // Decay: reduce effective_max_skew toward initial after sustained
        // period of no rejections.
        if self.ticks_since_rejection >= self.config.decay_interval {
            self.effective_max_skew = self.effective_max_skew
                .saturating_sub(1)
                .max(self.config.initial_max_skew);
            self.ticks_since_rejection = 0;
        }

        // Reset window every decay_interval ticks.
        // (Sliding window approximated by periodic reset for simplicity.)
        if self.window_total_commands > self.config.decay_interval * 100 {
            self.window_total_commands = 0;
            self.window_stale_rejections = 0;
        }
    }

    /// The current maximum allowed tick skew for stale-action evaluation.
    pub fn max_skew(&self) -> u64 {
        self.effective_max_skew
    }
}
```

### Lockstep Mode: No Adaptive Backoff

In Lockstep mode, `max_tick_skew` is fixed at 0 or 1 (configurable). Adaptive
backoff is disabled. The synchronous observation delivery eliminates the
staleness feedback loop entirely (HLD P-3).

```rust
impl WorldConfig {
    pub fn lockstep_max_skew(&self) -> u64 {
        // Fixed: 0 (strict) or 1 (lenient). No backoff.
        self.lockstep_skew.unwrap_or(0)
    }
}
```

### Dynamics Under Stress

The backoff parameters are tuned for the §23 #17 stress test:
50 agents, degraded tick rate, rejection CV < 0.3.

```
Time →
Rejection rate: ───────────────────────────────────────────
  20% ─ ─ ─ ─ ─ ─ ─ ─ threshold ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─

                    ╱╲
  spike hits ──►  ╱    ╲  backoff widens skew
                ╱        ╲──────────── steady state
              ╱                         ╲
  0% ────────                             ╲──── decay back

  max_skew:   2    3     4.5    4.5   ...  3    2
```

The 1.5× factor produces a gradual ramp (not a step function), and the
per-60-tick decay ensures the system returns to tight skew when load subsides.

---

## 11. Integration Points

### With TickEngine

| Event | Epoch Action |
|-------|-------------|
| Snapshot published | `epoch_counter.advance()`, ring push, `try_reclaim()` |
| Tick start | `detect_stalled_workers()` (optional: every N ticks) |
| Tick rollback | No epoch advance (generation not published) |
| `tick_disabled` | Epoch counter frozen; no new publications |

### With ObsPlan (murk-obs)

ObsPlan does not know about epochs directly. It reads through `&dyn
SnapshotAccess` (Decision N). The epoch machinery is transparent:

1. Worker selects snapshot from ring → pins epoch.
2. Worker creates `&dyn SnapshotAccess` reference to the snapshot.
3. ObsPlan executes against the reference.
4. Worker unpins.

### With Ingress

Ingress is not directly affected by epoch reclamation. The `arrival_seq`
counter and command queue are independent. However, the adaptive backoff state
(§10) is consulted during stale-action evaluation at command drain time.

### With WorldConfig

```rust
pub struct WorldConfig {
    // ... existing fields ...

    /// Epoch reclamation configuration (RealtimeAsync only).
    pub epoch_config: EpochConfig,

    /// Adaptive backoff configuration (RealtimeAsync only).
    pub backoff_config: AdaptiveBackoffConfig,
}
```

---

## 12. Testing Strategy

### Unit Tests

| Test | Validates |
|------|-----------|
| `test_epoch_advance` | Counter monotonicity, Acquire/Release ordering |
| `test_worker_pin_unpin` | Pin stores epoch, unpin stores MAX, quiesce timestamp updates |
| `test_min_pinned_no_workers` | Returns EPOCH_UNPINNED when pool is empty |
| `test_min_pinned_mixed` | Correct minimum across pinned/unpinned workers |
| `test_reclaim_threshold` | Generations below `min_pinned - 1` are reclaimed |
| `test_ring_eviction_overrides_pin` | Evicted generation reclaimable despite worker pin |
| `test_stall_detection` | Worker exceeding `max_epoch_hold` is flagged |
| `test_force_unpin_override` | Force-unpinned worker excluded from min_pinned |
| `test_cancel_flag` | Cancel flag set/cleared correctly, checked between regions |

### Property Tests (proptest)

| Property | Strategy |
|----------|----------|
| **No premature reclamation** | Random pin/unpin sequences; assert no generation is reclaimed while any non-override worker holds it |
| **Bounded generations** | Random publish/pin/unpin/stall sequences; assert `live_gens ≤ K + S` |
| **Epoch monotonicity** | Concurrent advance calls; assert observed epochs are monotonically increasing per thread |

### Stress Tests (§23 alignment)

| §23 Test | Epoch Coverage |
|----------|---------------|
| #15 Death spiral | 2× obs load at 80% utilization. Verify epoch reclamation keeps pace; memory bounded. |
| #16 Mass invalidation | 200 plans invalidated. Verify ring eviction + PLAN_INVALIDATED delivery. |
| #17 Rejection oscillation | 50 agents, degraded tick rate. Verify adaptive backoff converges (CV < 0.3). |

### Shutdown Tests

| Test | Validates |
|------|-----------|
| `test_clean_shutdown` | All phases complete within budget; `ShutdownResult.leaked_threads == 0` |
| `test_shutdown_during_tick` | Draining waits for tick completion |
| `test_shutdown_with_stalled_worker` | Quiescing applies §6 teardown; reports stalled count |
| `test_shutdown_timeout` | Each phase respects its timeout; total ≤ 300ms |
| `test_shutdown_with_tick_disabled` | Shutdown succeeds even when `tick_disabled` is set |
| `test_ingress_rejects_during_shutdown` | Commands receive `SHUTTING_DOWN` error |
| `test_shutdown_idempotent` | Multiple shutdown calls are safe (no double-join) |

### Adaptive Backoff Tests

| Test | Validates |
|------|-----------|
| `test_backoff_triggers_on_threshold` | Skew widens when rejection rate > 20% |
| `test_backoff_respects_cap` | `effective_max_skew` never exceeds `max_skew_cap` |
| `test_decay_reduces_skew` | After 60 ticks of no rejections, skew decreases by 1 |
| `test_lockstep_no_backoff` | Lockstep mode uses fixed skew, ignores backoff config |

---

## Appendix: `monotonic_nanos` Helper

```rust
/// Returns monotonic nanoseconds since an arbitrary epoch.
/// Used for stall detection timestamps. NOT wall-clock time.
fn monotonic_nanos() -> u64 {
    // On Linux, this is CLOCK_MONOTONIC via Instant.
    // Instant::now() is guaranteed monotonic.
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    let epoch = EPOCH.get_or_init(Instant::now);
    Instant::now().duration_since(*epoch).as_nanos() as u64
}
```

---

## Appendix: Summary of Configurable Parameters

| Parameter | Default | WorldConfig field | Affects |
|-----------|---------|-------------------|---------|
| `ring_capacity` | 8 | `epoch_config.ring_capacity` | Max retained snapshots |
| `max_epoch_hold` | 100ms | `epoch_config.max_epoch_hold` | Stall detection threshold |
| `cancel_grace` | 10ms | `epoch_config.cancel_grace` | Time after cancel before force-unpin |
| `worker_count` | `num_cpus/2` | `epoch_config.worker_count` | Egress thread pool size |
| `initial_max_skew` | 2 | `backoff_config.initial_max_skew` | Starting skew tolerance |
| `backoff_factor` | 1.5× | `backoff_config.backoff_factor` | Skew widening rate |
| `max_skew_cap` | 10 | `backoff_config.max_skew_cap` | Maximum skew tolerance |
| `decay_interval` | 60 ticks | `backoff_config.decay_interval` | Ticks before skew decay |
| `rejection_rate_threshold` | 20% | `backoff_config.rejection_rate_threshold` | Rate that triggers backoff |
