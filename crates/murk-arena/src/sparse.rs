//! Sparse slab allocator for copy-on-write fields.
//!
//! [`SparseSlab`] tracks allocations for `Sparse`-mutability fields. These
//! fields are allocated once and shared across generations until mutated,
//! at which point a new copy is bump-allocated in the current generation's
//! segments and the old allocation is added to the free list.

use crate::error::ArenaError;
use crate::handle::{FieldHandle, FieldLocation};
use crate::segment::SegmentList;

use murk_core::FieldId;

/// A segment range that has been freed and is available for reuse.
///
/// Field sizes are fixed for the arena's lifetime (sealed at `Config` time),
/// so reuse is exact-size: `alloc()` searches for a `RetiredRange` whose `len`
/// matches the requested allocation. This eliminates fragmentation by design.
#[derive(Clone, Copy, Debug)]
struct RetiredRange {
    segment_index: u16,
    offset: u32,
    len: u32,
}

/// A single sparse allocation slot.
#[derive(Clone, Debug)]
pub struct SparseSlot {
    /// The field this slot belongs to.
    pub field: FieldId,
    /// Generation when this allocation was created.
    pub generation_created: u32,
    /// Segment index within the sparse segment list.
    pub segment_index: u16,
    /// Offset within the segment (in f32 elements).
    pub offset: u32,
    /// Length in f32 elements.
    pub len: u32,
    /// Whether this slot is currently live (not freed).
    pub live: bool,
}

/// Slab allocator for sparse (copy-on-write) fields.
///
/// Manages allocations in the sparse segment pool. When a sparse field is
/// written for the first time in a generation, a new allocation is made and
/// the previous allocation's slot is marked as reclaimable.
///
/// Reclamation in Lockstep mode is immediate (`&mut self` guarantees no
/// readers). In RealtimeAsync mode, reclamation is epoch-gated.
///
/// Segment memory reclamation uses a two-phase scheme:
/// - Ranges freed during the current tick go into `pending_retired` (the
///   published descriptor may still reference them).
/// - At the start of the next tick (after `publish()`), `flush_retired()`
///   moves them into `retired_ranges`, where they become available for reuse.
pub struct SparseSlab {
    /// All allocation slots (live and dead).
    slots: Vec<SparseSlot>,
    /// Indices of dead slots available for reuse.
    free_list: Vec<usize>,
    /// Current mapping: FieldId → slot index (the live slot for each field).
    live_map: indexmap::IndexMap<FieldId, usize>,
    /// Segment ranges that are safe to reuse (freed in previous ticks).
    ///
    /// Reuse relies on exact-size matching: `alloc()` searches for a retired
    /// range whose `len` equals the requested allocation length. This is correct
    /// because field sizes are fixed for the arena's lifetime — the same field
    /// CoW'd repeatedly always produces the same `len`. The fixed-size invariant
    /// is enforced at three independent layers:
    ///   1. `PingPongArena` exposes no `resize_field()` API.
    ///   2. `Config` is build-then-consume: field defs are sealed at `World` construction.
    ///   3. Per-world isolation: `reset()` replaces the entire `SparseSlab`,
    ///      so stale ranges from a previous schema cannot accumulate.
    ///
    /// If dynamic schema support is ever added, `retired_ranges` must be cleared
    /// on field resize or replaced with a best-fit allocator.
    retired_ranges: Vec<RetiredRange>,
    /// Segment ranges freed during the current tick (not yet safe to reuse).
    pending_retired: Vec<RetiredRange>,
}

impl SparseSlab {
    /// Create an empty sparse slab.
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_list: Vec::new(),
            live_map: indexmap::IndexMap::new(),
            retired_ranges: Vec::new(),
            pending_retired: Vec::new(),
        }
    }

    /// Allocate storage for a sparse field in the given segment list.
    ///
    /// If the field already has a live allocation, the old slot is marked dead
    /// and added to the free list (CoW semantics).
    pub fn alloc(
        &mut self,
        field: FieldId,
        len: u32,
        generation: u32,
        segments: &mut SegmentList,
    ) -> Result<FieldHandle, ArenaError> {
        // Try to reuse a retired segment range of the exact size before
        // bump-allocating. Retired ranges are guaranteed safe (freed in a
        // previous tick, after publish).
        let (segment_index, offset) =
            if let Some(pos) = self.retired_ranges.iter().position(|r| r.len == len) {
                let r = self.retired_ranges.swap_remove(pos);
                (r.segment_index, r.offset)
            } else {
                segments.alloc(len)?
            };

        // Mark old allocation as dead if it exists.
        if let Some(&old_idx) = self.live_map.get(&field) {
            let old = &self.slots[old_idx];
            self.pending_retired.push(RetiredRange {
                segment_index: old.segment_index,
                offset: old.offset,
                len: old.len,
            });
            self.slots[old_idx].live = false;
            self.free_list.push(old_idx);
        }

        let slot = SparseSlot {
            field,
            generation_created: generation,
            segment_index,
            offset,
            len,
            live: true,
        };

        // Reuse a free slot or push a new one.
        let slot_idx = if let Some(reuse_idx) = self.free_list.pop() {
            self.slots[reuse_idx] = slot;
            reuse_idx
        } else {
            let idx = self.slots.len();
            self.slots.push(slot);
            idx
        };

        self.live_map.insert(field, slot_idx);

        let location = FieldLocation::Sparse { segment_index };
        Ok(FieldHandle::new(generation, offset, len, location))
    }

    /// Get the current live handle for a sparse field.
    pub fn get_handle(&self, field: FieldId) -> Option<FieldHandle> {
        let &idx = self.live_map.get(&field)?;
        let slot = &self.slots[idx];
        if !slot.live {
            return None;
        }
        Some(FieldHandle::new(
            slot.generation_created,
            slot.offset,
            slot.len,
            FieldLocation::Sparse {
                segment_index: slot.segment_index,
            },
        ))
    }

    /// Check whether a field has a live sparse allocation.
    pub fn contains(&self, field: FieldId) -> bool {
        self.live_map.contains_key(&field)
    }

    /// Number of live allocations.
    pub fn live_count(&self) -> usize {
        self.live_map.len()
    }

    /// Total slots (live + dead).
    pub fn total_slots(&self) -> usize {
        self.slots.len()
    }

    /// Number of free (dead) slots available for reuse.
    pub fn free_count(&self) -> usize {
        self.free_list.len()
    }

    /// Promote pending retired ranges to the reusable pool.
    ///
    /// Must be called after `publish()` and before the next round of
    /// `alloc()` calls. At this point the published descriptor no longer
    /// references the pending ranges, so they are safe to hand out.
    pub fn flush_retired(&mut self) {
        self.retired_ranges.append(&mut self.pending_retired);
    }

    /// Number of segment ranges available for reuse.
    pub fn retired_range_count(&self) -> usize {
        self.retired_ranges.len()
    }

    /// Number of segment ranges pending promotion (freed this tick).
    pub fn pending_retired_count(&self) -> usize {
        self.pending_retired.len()
    }

    /// Iterate over all live field → slot index mappings.
    pub fn live_fields(&self) -> impl Iterator<Item = (&FieldId, &SparseSlot)> {
        self.live_map
            .iter()
            .map(|(field, &idx)| (field, &self.slots[idx]))
    }
}

impl Default for SparseSlab {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_segments() -> SegmentList {
        SegmentList::new(4096, 4)
    }

    #[test]
    fn alloc_creates_live_slot() {
        let mut slab = SparseSlab::new();
        let mut segs = make_segments();
        let handle = slab.alloc(FieldId(0), 100, 1, &mut segs).unwrap();
        assert_eq!(handle.len(), 100);
        assert_eq!(handle.generation(), 1);
        assert!(matches!(handle.location(), FieldLocation::Sparse { .. }));
        assert_eq!(slab.live_count(), 1);
    }

    #[test]
    fn realloc_marks_old_slot_dead() {
        let mut slab = SparseSlab::new();
        let mut segs = make_segments();

        let h1 = slab.alloc(FieldId(0), 100, 1, &mut segs).unwrap();
        let h2 = slab.alloc(FieldId(0), 100, 2, &mut segs).unwrap();

        // Old handle's generation differs from new.
        assert_eq!(h1.generation(), 1);
        assert_eq!(h2.generation(), 2);

        // Only one live allocation for this field.
        assert_eq!(slab.live_count(), 1);
        let live_h = slab.get_handle(FieldId(0)).unwrap();
        assert_eq!(live_h.generation(), 2);
    }

    #[test]
    fn get_handle_returns_none_for_unknown() {
        let slab = SparseSlab::new();
        assert!(slab.get_handle(FieldId(99)).is_none());
    }

    #[test]
    fn multiple_fields_tracked_independently() {
        let mut slab = SparseSlab::new();
        let mut segs = make_segments();

        slab.alloc(FieldId(0), 50, 1, &mut segs).unwrap();
        slab.alloc(FieldId(1), 75, 1, &mut segs).unwrap();

        assert_eq!(slab.live_count(), 2);
        assert_eq!(slab.get_handle(FieldId(0)).unwrap().len(), 50);
        assert_eq!(slab.get_handle(FieldId(1)).unwrap().len(), 75);
    }

    #[test]
    fn free_slots_are_reused() {
        let mut slab = SparseSlab::new();
        let mut segs = make_segments();

        // First alloc creates slot 0.
        slab.alloc(FieldId(0), 50, 1, &mut segs).unwrap();
        // Realloc kills slot 0, but it gets reused for the new allocation.
        slab.alloc(FieldId(0), 50, 2, &mut segs).unwrap();

        // The dead slot was immediately reused, so total slots should stay at 1.
        // (free_list pops the old slot, then it's reused for the new one.)
        assert_eq!(slab.total_slots(), 1);
    }

    #[test]
    fn retired_range_reused_after_flush() {
        let mut slab = SparseSlab::new();
        let mut segs = make_segments();

        // Gen 0: initial allocation.
        slab.alloc(FieldId(0), 100, 0, &mut segs).unwrap();
        let used_after_init = segs.total_used();

        // Gen 1: CoW write — old range goes to pending_retired.
        slab.alloc(FieldId(0), 100, 1, &mut segs).unwrap();
        let used_after_cow1 = segs.total_used();
        assert!(used_after_cow1 > used_after_init);
        assert_eq!(slab.pending_retired_count(), 1);
        assert_eq!(slab.retired_range_count(), 0);

        // Simulate publish + begin_tick: flush pending → retired.
        slab.flush_retired();
        assert_eq!(slab.pending_retired_count(), 0);
        assert_eq!(slab.retired_range_count(), 1);

        // Gen 2: CoW write — should reuse the retired range.
        slab.alloc(FieldId(0), 100, 2, &mut segs).unwrap();
        assert_eq!(
            segs.total_used(),
            used_after_cow1,
            "no new segment memory consumed"
        );
        assert_eq!(slab.retired_range_count(), 0, "retired range was consumed");
    }

    #[test]
    fn pending_retired_not_reused_before_flush() {
        let mut slab = SparseSlab::new();
        let mut segs = make_segments();

        // Gen 0: initial allocation.
        slab.alloc(FieldId(0), 100, 0, &mut segs).unwrap();
        // Gen 1: CoW — range goes to pending_retired.
        slab.alloc(FieldId(0), 100, 1, &mut segs).unwrap();
        let used_before = segs.total_used();

        // Gen 2: another CoW WITHOUT flush — must bump-allocate.
        slab.alloc(FieldId(0), 100, 2, &mut segs).unwrap();
        assert!(
            segs.total_used() > used_before,
            "should bump-allocate because pending ranges are not reusable"
        );
    }

    #[test]
    fn many_cow_writes_with_flush_stays_bounded() {
        let mut slab = SparseSlab::new();
        // Small segments to make exhaustion easy to detect.
        let mut segs = SegmentList::new(1024, 2);

        slab.alloc(FieldId(0), 100, 0, &mut segs).unwrap();

        // 50 ticks of CoW writes with flush between each — should never
        // exhaust the pool (only 200 f32s ever live at once).
        for gen in 1..=50u32 {
            slab.flush_retired();
            slab.alloc(FieldId(0), 100, gen, &mut segs).unwrap();
        }
        // Pool should still be well within bounds.
        assert!(segs.total_used() <= 300);
    }

    #[test]
    fn alloc_falls_back_to_bump_when_no_size_match() {
        let mut slab = SparseSlab::new();
        let mut segs = make_segments();

        // Gen 0: allocate two fields of size 100.
        slab.alloc(FieldId(0), 100, 0, &mut segs).unwrap();
        slab.alloc(FieldId(1), 100, 0, &mut segs).unwrap();

        // Gen 1: CoW both — their ranges go to pending_retired.
        slab.alloc(FieldId(0), 100, 1, &mut segs).unwrap();
        slab.alloc(FieldId(1), 100, 1, &mut segs).unwrap();
        slab.flush_retired();
        assert_eq!(slab.retired_range_count(), 2);

        let used_before = segs.total_used();

        // Request size 200 — no retired range matches, must bump-allocate.
        slab.alloc(FieldId(2), 200, 2, &mut segs).unwrap();
        assert!(
            segs.total_used() > used_before,
            "should bump-allocate when no retired range matches the requested size"
        );
        // Both size-100 retired ranges should be untouched.
        assert_eq!(slab.retired_range_count(), 2);
    }

    #[test]
    fn three_fields_same_size_all_ranges_retired() {
        let mut slab = SparseSlab::new();
        let mut segs = make_segments();

        // Gen 0: allocate three fields of equal size.
        slab.alloc(FieldId(0), 100, 0, &mut segs).unwrap();
        slab.alloc(FieldId(1), 100, 0, &mut segs).unwrap();
        slab.alloc(FieldId(2), 100, 0, &mut segs).unwrap();

        // Gen 1: CoW all three — old ranges go to pending_retired.
        slab.alloc(FieldId(0), 100, 1, &mut segs).unwrap();
        slab.alloc(FieldId(1), 100, 1, &mut segs).unwrap();
        slab.alloc(FieldId(2), 100, 1, &mut segs).unwrap();
        slab.flush_retired();
        assert_eq!(slab.retired_range_count(), 3);

        let used_before = segs.total_used();

        // Gen 2: CoW all three again — should reuse all three retired ranges.
        slab.alloc(FieldId(0), 100, 2, &mut segs).unwrap();
        slab.alloc(FieldId(1), 100, 2, &mut segs).unwrap();
        slab.alloc(FieldId(2), 100, 2, &mut segs).unwrap();

        assert_eq!(
            segs.total_used(),
            used_before,
            "no new segment memory consumed — all retired ranges reused"
        );
        assert_eq!(slab.retired_range_count(), 0, "all retired ranges consumed");
    }

    #[test]
    fn different_size_ranges_not_mixed() {
        let mut slab = SparseSlab::new();
        let mut segs = make_segments();

        // Two fields with different sizes.
        slab.alloc(FieldId(0), 100, 0, &mut segs).unwrap();
        slab.alloc(FieldId(1), 200, 0, &mut segs).unwrap();

        // CoW both — retire both ranges.
        slab.alloc(FieldId(0), 100, 1, &mut segs).unwrap();
        slab.alloc(FieldId(1), 200, 1, &mut segs).unwrap();
        slab.flush_retired();

        let used_before = segs.total_used();

        // Request size 200 — should reuse the size-200 range, not size-100.
        slab.alloc(FieldId(1), 200, 2, &mut segs).unwrap();
        assert_eq!(segs.total_used(), used_before);
        // The size-100 range should still be available.
        assert_eq!(slab.retired_range_count(), 1);
    }

    #[cfg(not(miri))]
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #[test]
            fn live_count_equals_distinct_fields(
                field_ids in proptest::collection::vec(0u32..16, 1..20),
            ) {
                let mut slab = SparseSlab::new();
                let mut segs = make_segments();
                for (gen, &fid) in field_ids.iter().enumerate() {
                    let _ = slab.alloc(FieldId(fid), 10, gen as u32, &mut segs);
                }
                // Live count = number of distinct field IDs allocated.
                let distinct: std::collections::HashSet<_> = field_ids.iter().collect();
                prop_assert_eq!(slab.live_count(), distinct.len());
            }

            #[test]
            fn get_handle_returns_latest_generation(
                gens in proptest::collection::vec(1u32..100, 2..10),
            ) {
                let mut slab = SparseSlab::new();
                let mut segs = make_segments();
                let field = FieldId(0);
                let mut last_gen = 0;
                for &gen in &gens {
                    let _ = slab.alloc(field, 10, gen, &mut segs);
                    last_gen = gen;
                }
                let handle = slab.get_handle(field).unwrap();
                prop_assert_eq!(handle.generation(), last_gen);
            }

            #[test]
            fn total_slots_bounded_by_alloc_count(
                ops in proptest::collection::vec((0u32..8, 1u32..50), 1..30),
            ) {
                let mut slab = SparseSlab::new();
                let mut segs = make_segments();
                let mut alloc_count = 0u32;
                for (gen, (fid, len)) in ops.iter().enumerate() {
                    if slab.alloc(FieldId(*fid), *len, gen as u32, &mut segs).is_ok() {
                        alloc_count += 1;
                    }
                }
                // Total slots can be less than alloc count due to free list reuse.
                prop_assert!(slab.total_slots() as u32 <= alloc_count);
            }
        }
    }
}
