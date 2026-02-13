//! Fixed-capacity ring buffer of owned snapshots for RealtimeAsync mode.
//!
//! [`SnapshotRing`] stores `Arc<OwnedSnapshot>` slots with single-producer
//! push and multi-consumer read. This is the spike implementation using
//! `Mutex` per slot; production will replace `Arc` with epoch-based pinning.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use murk_arena::OwnedSnapshot;

/// A tagged slot: the `u64` is the monotonic write position when this
/// snapshot was stored, enabling consumers to detect overwrites.
type Slot = Option<(u64, Arc<OwnedSnapshot>)>;

/// A fixed-capacity ring buffer of `Arc<OwnedSnapshot>`.
///
/// Single-producer: only one thread calls [`push`]. Multi-consumer: any
/// thread can call [`latest`] or [`get_by_pos`] to read snapshots.
///
/// The write position is monotonically increasing (never wraps). Slot
/// index is computed as `pos % capacity`. Each slot stores a position
/// tag alongside the snapshot so that consumers can verify they are
/// reading the slot they intended, even under concurrent producer pushes.
pub struct SnapshotRing {
    /// Each slot holds `(position_tag, snapshot)`. The tag is the monotonic
    /// write position at which this snapshot was stored, enabling consumers
    /// to detect when a slot has been overwritten between their bounds check
    /// and their lock acquisition.
    slots: Vec<Mutex<Slot>>,
    write_pos: AtomicU64,
    capacity: usize,
}

// Compile-time assertion: SnapshotRing must be Send + Sync.
const _: fn() = || {
    fn assert<T: Send + Sync>() {}
    assert::<SnapshotRing>();
};

impl SnapshotRing {
    /// Create a new ring buffer with the given capacity.
    ///
    /// # Panics
    ///
    /// Panics if `capacity < 2`. A ring buffer needs at least 2 slots
    /// to be useful (one being written, one readable).
    pub fn new(capacity: usize) -> Self {
        assert!(capacity >= 2, "SnapshotRing capacity must be >= 2, got {capacity}");
        let slots = (0..capacity).map(|_| Mutex::new(None)).collect();
        Self {
            slots,
            write_pos: AtomicU64::new(0),
            capacity,
        }
    }

    /// Push a new snapshot into the ring. Single-producer only.
    ///
    /// Returns the evicted snapshot (if any) that was displaced by this push.
    /// The caller can use this for reclamation bookkeeping.
    pub fn push(&self, snapshot: OwnedSnapshot) -> Option<Arc<OwnedSnapshot>> {
        let pos = self.write_pos.load(Ordering::Relaxed);
        let slot_idx = (pos as usize) % self.capacity;

        let arc = Arc::new(snapshot);
        let evicted = {
            let mut slot = self.slots[slot_idx].lock().unwrap();
            let prev = slot.take().map(|(_tag, arc)| arc);
            *slot = Some((pos, Arc::clone(&arc)));
            prev
        };

        // Release-store ensures the snapshot data is visible before
        // consumers observe the new write_pos.
        self.write_pos.store(pos + 1, Ordering::Release);

        evicted
    }

    /// Get the latest (most recently pushed) snapshot.
    ///
    /// Returns `None` if no snapshots have been pushed yet.
    pub fn latest(&self) -> Option<Arc<OwnedSnapshot>> {
        let pos = self.write_pos.load(Ordering::Acquire);
        if pos == 0 {
            return None;
        }
        let target_pos = pos - 1;
        let slot_idx = (target_pos as usize) % self.capacity;
        let slot = self.slots[slot_idx].lock().unwrap();
        match slot.as_ref() {
            Some((tag, arc)) if *tag == target_pos => Some(Arc::clone(arc)),
            // Producer overwrote this slot between our write_pos read
            // and lock acquisition. The snapshot we wanted is gone.
            _ => None,
        }
    }

    /// Get a snapshot by its monotonic write position.
    ///
    /// Returns `None` if the position has been evicted (overwritten) or
    /// hasn't been written yet.
    pub fn get_by_pos(&self, pos: u64) -> Option<Arc<OwnedSnapshot>> {
        let current = self.write_pos.load(Ordering::Acquire);

        // Not yet written.
        if pos >= current {
            return None;
        }

        // Evicted: the position is older than what the ring retains.
        if current - pos > self.capacity as u64 {
            return None;
        }

        let slot_idx = (pos as usize) % self.capacity;
        let slot = self.slots[slot_idx].lock().unwrap();
        match slot.as_ref() {
            Some((tag, arc)) if *tag == pos => Some(Arc::clone(arc)),
            // The producer overwrote this slot between our bounds check
            // and lock acquisition — the requested position is gone.
            _ => None,
        }
    }

    /// Number of snapshots currently stored (up to `capacity`).
    pub fn len(&self) -> usize {
        let pos = self.write_pos.load(Ordering::Acquire) as usize;
        pos.min(self.capacity)
    }

    /// Whether the ring is empty.
    pub fn is_empty(&self) -> bool {
        self.write_pos.load(Ordering::Acquire) == 0
    }

    /// The ring buffer capacity.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The current monotonic write position.
    pub fn write_pos(&self) -> u64 {
        self.write_pos.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_arena::config::ArenaConfig;
    use murk_arena::pingpong::PingPongArena;
    use murk_arena::static_arena::StaticArena;
    use murk_core::id::{FieldId, ParameterVersion, TickId};
    use murk_core::traits::{FieldWriter as _, SnapshotAccess};
    use murk_core::{BoundaryBehavior, FieldDef, FieldMutability, FieldType};

    fn make_test_snapshot(tick: u64) -> OwnedSnapshot {
        let cell_count = 10u32;
        let config = ArenaConfig::new(cell_count);
        let field_defs = vec![(
            FieldId(0),
            FieldDef {
                name: "energy".into(),
                field_type: FieldType::Scalar,
                mutability: FieldMutability::PerTick,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            },
        )];
        let static_arena = StaticArena::new(&[]).into_shared();
        let mut arena = PingPongArena::new(config, field_defs, static_arena).unwrap();

        {
            let mut guard = arena.begin_tick().unwrap();
            let data = guard.writer.write(FieldId(0)).unwrap();
            data.fill(tick as f32);
        }
        arena.publish(TickId(tick), ParameterVersion(0));
        arena.owned_snapshot()
    }

    #[test]
    fn test_ring_new_empty() {
        let ring = SnapshotRing::new(4);
        assert_eq!(ring.len(), 0);
        assert!(ring.is_empty());
        assert_eq!(ring.capacity(), 4);
        assert_eq!(ring.write_pos(), 0);
        assert!(ring.latest().is_none());
    }

    #[test]
    fn test_ring_push_and_latest() {
        let ring = SnapshotRing::new(4);
        ring.push(make_test_snapshot(1));
        assert_eq!(ring.len(), 1);
        assert!(!ring.is_empty());

        let latest = ring.latest().unwrap();
        assert_eq!(latest.tick_id(), TickId(1));
    }

    #[test]
    fn test_ring_eviction() {
        let ring = SnapshotRing::new(4);

        // Push 4 — fills the ring.
        for i in 1..=4 {
            let evicted = ring.push(make_test_snapshot(i));
            assert!(evicted.is_none());
        }
        assert_eq!(ring.len(), 4);

        // Push 5th — evicts tick 1.
        let evicted = ring.push(make_test_snapshot(5));
        assert!(evicted.is_some());
        assert_eq!(evicted.unwrap().tick_id(), TickId(1));
        assert_eq!(ring.len(), 4);
    }

    #[test]
    fn test_ring_latest_is_newest() {
        let ring = SnapshotRing::new(4);
        for i in 1..=10 {
            ring.push(make_test_snapshot(i));
        }
        let latest = ring.latest().unwrap();
        assert_eq!(latest.tick_id(), TickId(10));
    }

    #[test]
    fn test_ring_get_by_pos() {
        let ring = SnapshotRing::new(4);
        for i in 1..=4 {
            ring.push(make_test_snapshot(i));
        }

        // Positions are 0-indexed (write_pos starts at 0).
        let snap = ring.get_by_pos(0).unwrap();
        assert_eq!(snap.tick_id(), TickId(1));

        let snap = ring.get_by_pos(3).unwrap();
        assert_eq!(snap.tick_id(), TickId(4));

        // Position 4 not yet written.
        assert!(ring.get_by_pos(4).is_none());
    }

    #[test]
    fn test_ring_get_evicted_returns_none() {
        let ring = SnapshotRing::new(4);
        for i in 1..=8 {
            ring.push(make_test_snapshot(i));
        }
        // Positions 0-3 have been evicted (overwritten by positions 4-7).
        assert!(ring.get_by_pos(0).is_none());
        assert!(ring.get_by_pos(3).is_none());

        // Positions 4-7 should still be available.
        let snap = ring.get_by_pos(4).unwrap();
        assert_eq!(snap.tick_id(), TickId(5));
    }

    #[test]
    #[should_panic(expected = "capacity must be >= 2")]
    fn test_ring_capacity_panics_below_2() {
        SnapshotRing::new(1);
    }

    #[test]
    fn test_get_by_pos_returns_none_after_concurrent_overwrite() {
        // Simulates the race: consumer reads write_pos, producer wraps
        // around and overwrites the target slot, consumer locks and
        // must detect the overwrite via position tag.
        let ring = SnapshotRing::new(4);

        // Fill positions 0-3.
        for i in 1..=4 {
            ring.push(make_test_snapshot(i));
        }
        assert_eq!(ring.get_by_pos(0).unwrap().tick_id(), TickId(1));

        // Now push 4 more: positions 4-7 overwrite slots 0-3.
        for i in 5..=8 {
            ring.push(make_test_snapshot(i));
        }

        // Position 0 was overwritten by position 4 (same slot index).
        // get_by_pos(0) must return None, not the snapshot at position 4.
        assert!(ring.get_by_pos(0).is_none());
        assert!(ring.get_by_pos(3).is_none());

        // Position 4 should still be accessible.
        let snap = ring.get_by_pos(4).unwrap();
        assert_eq!(snap.tick_id(), TickId(5));

        // Position 7 should be accessible.
        let snap = ring.get_by_pos(7).unwrap();
        assert_eq!(snap.tick_id(), TickId(8));
    }

    #[test]
    fn test_get_by_pos_tag_matches_position() {
        // Verify that get_by_pos returns the exact snapshot for the
        // requested position, not whatever happens to be in the slot.
        let ring = SnapshotRing::new(4);

        for i in 1..=4 {
            ring.push(make_test_snapshot(i * 10));
        }

        // Each position should return its exact snapshot.
        assert_eq!(ring.get_by_pos(0).unwrap().tick_id(), TickId(10));
        assert_eq!(ring.get_by_pos(1).unwrap().tick_id(), TickId(20));
        assert_eq!(ring.get_by_pos(2).unwrap().tick_id(), TickId(30));
        assert_eq!(ring.get_by_pos(3).unwrap().tick_id(), TickId(40));
    }

    // ── Cross-thread integration tests ────────────────────────────

    #[test]
    fn test_producer_consumer_cross_thread() {
        use crate::config::{BackoffConfig, WorldConfig};
        use crate::tick::TickEngine;
        use murk_space::{EdgeBehavior, Line1D};
        use murk_test_utils::ConstPropagator;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::thread;

        let config = WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![FieldDef {
                name: "energy".into(),
                field_type: FieldType::Scalar,
                mutability: FieldMutability::PerTick,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            }],
            propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), 42.0))],
            dt: 0.1,
            seed: 42,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: BackoffConfig::default(),
        };
        let mut engine = TickEngine::new(config).unwrap();

        let ring = Arc::new(SnapshotRing::new(8));
        let epoch_counter = Arc::new(crate::epoch::EpochCounter::new());
        let producer_done = Arc::new(AtomicBool::new(false));

        // Producer: run 100 ticks, push snapshots to ring, advance epoch.
        let ring_prod = Arc::clone(&ring);
        let epoch_prod = Arc::clone(&epoch_counter);
        let done_flag = Arc::clone(&producer_done);
        let producer = thread::spawn(move || {
            for _ in 0..100 {
                engine.execute_tick().unwrap();
                let snap = engine.owned_snapshot();
                ring_prod.push(snap);
                epoch_prod.advance();
            }
            done_flag.store(true, Ordering::Release);
        });

        // 4 consumer threads: read latest snapshot until producer is done.
        let consumers: Vec<_> = (0..4)
            .map(|id| {
                let ring_c = Arc::clone(&ring);
                let done_c = Arc::clone(&producer_done);
                let worker = Arc::new(crate::epoch::WorkerEpoch::new(id));
                thread::spawn(move || {
                    let mut reads = 0u64;
                    loop {
                        if let Some(snap) = ring_c.latest() {
                            let epoch = snap.tick_id().0;
                            worker.pin(epoch);
                            let data = snap.read_field(FieldId(0)).unwrap();
                            assert_eq!(data.len(), 10);
                            assert!(data.iter().all(|&v| v == 42.0));
                            worker.unpin();
                            reads += 1;
                        }
                        if done_c.load(Ordering::Acquire) && reads > 0 {
                            break;
                        }
                        thread::yield_now();
                    }
                    reads
                })
            })
            .collect();

        producer.join().unwrap();
        let mut total_reads = 0u64;
        for c in consumers {
            let reads = c.join().unwrap();
            assert!(reads > 0, "consumer should have read at least one snapshot");
            total_reads += reads;
        }

        // Verify final state.
        assert!(ring.len() <= 8);
        assert_eq!(epoch_counter.current(), 100);
        assert!(total_reads >= 4, "consumers collectively should have many reads");
    }

    #[test]
    fn test_epoch_pin_unpin_cross_thread() {
        use crate::epoch::WorkerEpoch;
        use std::thread;

        let workers: Vec<Arc<WorkerEpoch>> =
            (0..4).map(|i| Arc::new(WorkerEpoch::new(i))).collect();

        let handles: Vec<_> = workers
            .iter()
            .cloned()
            .map(|worker| {
                thread::spawn(move || {
                    for epoch in 0..100u64 {
                        worker.pin(epoch);
                        assert!(worker.is_pinned());
                        assert_eq!(worker.pinned_epoch(), epoch);
                        worker.unpin();
                        assert!(!worker.is_pinned());
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        // All workers should be unpinned.
        for w in &workers {
            assert!(!w.is_pinned());
            assert!(w.last_quiesce_ns() > 0);
        }
    }
}
