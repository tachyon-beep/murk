//! Epoch-based reclamation primitives for RealtimeAsync mode.
//!
//! Provides [`EpochCounter`] (global monotonic epoch) and [`WorkerEpoch`]
//! (per-worker pin/unpin state with cache-line padding). These are the
//! building blocks for the epoch reclamation protocol described in
//! `docs/design/epoch-reclamation.md`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

/// Sentinel value meaning "this worker is not pinned to any epoch."
pub const EPOCH_UNPINNED: u64 = u64::MAX;

/// Global epoch counter, incremented by TickEngine at each snapshot publication.
///
/// Monotonically increasing. Never wraps in practice (u64 overflow at 60 Hz
/// would take ~9.7 billion years).
pub struct EpochCounter {
    current: AtomicU64,
}

impl Default for EpochCounter {
    fn default() -> Self {
        Self::new()
    }
}

// Compile-time assertion: EpochCounter must be Send + Sync.
const _: fn() = || {
    fn assert<T: Send + Sync>() {}
    assert::<EpochCounter>();
};

impl EpochCounter {
    /// Create a new epoch counter starting at 0.
    pub fn new() -> Self {
        Self {
            current: AtomicU64::new(0),
        }
    }

    /// Advance the epoch. Called by TickEngine after publishing a snapshot.
    /// Returns the new epoch value.
    pub fn advance(&self) -> u64 {
        self.current.fetch_add(1, Ordering::Release) + 1
    }

    /// Read the current epoch value.
    pub fn current(&self) -> u64 {
        self.current.load(Ordering::Acquire)
    }
}

/// Per-worker epoch state, padded to avoid false sharing.
///
/// Each egress worker holds one of these. The TickEngine reads all `pinned`
/// fields during reclamation checks; without padding, adjacent workers'
/// writes would invalidate each other's cache lines.
///
/// 128-byte alignment covers both 64-byte (x86) and 128-byte (Apple M-series)
/// cache line sizes.
#[repr(align(128))]
pub struct WorkerEpoch {
    /// The epoch this worker is currently pinned to.
    /// `EPOCH_UNPINNED` means not holding any generation.
    pinned: AtomicU64,

    /// Monotonic timestamp (nanos) when `pin()` was called.
    /// Used for stalled-worker detection: the tick thread computes
    /// `now - pin_start_ns` to get the actual pin hold duration.
    pin_start_ns: AtomicU64,

    /// Monotonic timestamp (nanos) of the last unpin.
    last_quiesce_ns: AtomicU64,

    /// Cooperative cancellation flag.
    cancel: AtomicBool,

    /// Worker index (for diagnostics in stalled-worker reporting).
    #[allow(dead_code)]
    worker_id: u32,
}

// Compile-time assertion: WorkerEpoch must be Send + Sync.
const _: fn() = || {
    fn assert<T: Send + Sync>() {}
    assert::<WorkerEpoch>();
};

impl WorkerEpoch {
    /// Create a new worker epoch in the unpinned state.
    ///
    /// `last_quiesce_ns` is seeded to the current monotonic time so
    /// that the first pin is never misclassified as a stall due to
    /// elapsed process uptime.
    pub fn new(worker_id: u32) -> Self {
        let now = monotonic_nanos();
        Self {
            pinned: AtomicU64::new(EPOCH_UNPINNED),
            pin_start_ns: AtomicU64::new(now),
            last_quiesce_ns: AtomicU64::new(now),
            cancel: AtomicBool::new(false),
            worker_id,
        }
    }

    /// Pin this worker to the given epoch before accessing a snapshot.
    /// Records the pin-start timestamp for stall detection.
    pub fn pin(&self, epoch: u64) {
        self.pin_start_ns.store(monotonic_nanos(), Ordering::Release);
        self.pinned.store(epoch, Ordering::Release);
    }

    /// Unpin this worker after finishing with the snapshot.
    /// Updates the quiescence timestamp.
    pub fn unpin(&self) {
        self.pinned.store(EPOCH_UNPINNED, Ordering::Release);
        let now_ns = monotonic_nanos();
        self.last_quiesce_ns.store(now_ns, Ordering::Release);
    }

    /// Whether this worker is currently pinned to an epoch.
    pub fn is_pinned(&self) -> bool {
        self.pinned.load(Ordering::Acquire) != EPOCH_UNPINNED
    }

    /// The epoch this worker is pinned to, or `EPOCH_UNPINNED`.
    pub fn pinned_epoch(&self) -> u64 {
        self.pinned.load(Ordering::Acquire)
    }

    /// Monotonic nanoseconds when `pin()` was last called.
    /// Used by the tick thread to measure actual pin hold duration.
    pub fn pin_start_ns(&self) -> u64 {
        self.pin_start_ns.load(Ordering::Acquire)
    }

    /// Monotonic nanoseconds of the last unpin event.
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

/// Compute the minimum pinned epoch across all workers.
///
/// Returns [`EPOCH_UNPINNED`] if no worker is pinned (all reclaimable).
pub fn min_pinned_epoch(workers: &[WorkerEpoch]) -> u64 {
    workers
        .iter()
        .map(|w| w.pinned_epoch())
        .min()
        .unwrap_or(EPOCH_UNPINNED)
}

/// Returns monotonic nanoseconds since an arbitrary process-local epoch.
///
/// Uses `OnceLock<Instant>` to lazily initialise a baseline. NOT wall-clock
/// time — only for relative duration comparisons (stall detection).
///
/// This is the single source of truth for monotonic timestamps in the
/// engine. All callers (epoch, tick_thread, egress) must use this
/// function to avoid clock-skew between independent `OnceLock` statics.
pub(crate) fn monotonic_nanos() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = EPOCH.get_or_init(Instant::now);
    Instant::now().duration_since(*epoch).as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_epoch_advance() {
        let counter = EpochCounter::new();
        assert_eq!(counter.current(), 0);
        assert_eq!(counter.advance(), 1);
        assert_eq!(counter.advance(), 2);
        assert_eq!(counter.advance(), 3);
        assert_eq!(counter.current(), 3);

        // Monotonicity: each advance returns a strictly larger value.
        let mut prev = counter.current();
        for _ in 0..100 {
            let next = counter.advance();
            assert!(next > prev);
            prev = next;
        }
    }

    #[test]
    fn test_worker_pin_unpin() {
        let worker = WorkerEpoch::new(0);

        // Starts unpinned, with a seeded quiesce timestamp.
        assert!(!worker.is_pinned());
        assert_eq!(worker.pinned_epoch(), EPOCH_UNPINNED);
        let initial_quiesce = worker.last_quiesce_ns();
        assert!(initial_quiesce > 0, "quiesce time should be seeded at creation");

        // Pin to epoch 5.
        worker.pin(5);
        assert!(worker.is_pinned());
        assert_eq!(worker.pinned_epoch(), 5);

        // Unpin — stores MAX and updates quiesce timestamp.
        worker.unpin();
        assert!(!worker.is_pinned());
        assert_eq!(worker.pinned_epoch(), EPOCH_UNPINNED);
        assert!(worker.last_quiesce_ns() >= initial_quiesce);
    }

    #[test]
    fn test_min_pinned_no_workers() {
        let workers: Vec<WorkerEpoch> = vec![];
        assert_eq!(min_pinned_epoch(&workers), EPOCH_UNPINNED);
    }

    #[test]
    fn test_min_pinned_mixed() {
        let workers: Vec<WorkerEpoch> = (0..4).map(WorkerEpoch::new).collect();

        // Workers 0 and 2 are pinned, 1 and 3 are unpinned.
        workers[0].pin(10);
        workers[2].pin(5);

        assert_eq!(min_pinned_epoch(&workers), 5);

        // Unpin worker 2 — min should now be 10.
        workers[2].unpin();
        assert_eq!(min_pinned_epoch(&workers), 10);

        // Unpin worker 0 — all unpinned, result is EPOCH_UNPINNED.
        workers[0].unpin();
        assert_eq!(min_pinned_epoch(&workers), EPOCH_UNPINNED);
    }

    #[test]
    fn test_cancel_flag() {
        let worker = WorkerEpoch::new(0);

        assert!(!worker.is_cancelled());

        worker.request_cancel();
        assert!(worker.is_cancelled());

        worker.clear_cancel();
        assert!(!worker.is_cancelled());
    }

    #[test]
    fn test_worker_epoch_alignment() {
        assert!(
            std::mem::align_of::<WorkerEpoch>() >= 128,
            "WorkerEpoch must be cache-line aligned (>= 128 bytes)"
        );
    }

    #[test]
    fn test_pin_start_ns_records_pin_time() {
        let worker = WorkerEpoch::new(0);

        // pin_start_ns is seeded at construction (not zero).
        let initial = worker.pin_start_ns();
        assert!(initial > 0, "pin_start_ns should be seeded at construction");

        // Sleep briefly then pin — pin_start_ns should advance.
        std::thread::sleep(std::time::Duration::from_millis(5));
        worker.pin(42);
        let after_pin = worker.pin_start_ns();
        assert!(
            after_pin > initial,
            "pin_start_ns should advance on pin(): initial={initial}, after={after_pin}"
        );

        // Unpin should NOT change pin_start_ns (only updates last_quiesce_ns).
        worker.unpin();
        let after_unpin = worker.pin_start_ns();
        assert_eq!(
            after_pin, after_unpin,
            "pin_start_ns should not change on unpin()"
        );
    }
}
