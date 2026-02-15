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
pub struct SparseSlab {
    /// All allocation slots (live and dead).
    slots: Vec<SparseSlot>,
    /// Indices of dead slots available for reuse.
    free_list: Vec<usize>,
    /// Current mapping: FieldId → slot index (the live slot for each field).
    live_map: indexmap::IndexMap<FieldId, usize>,
}

impl SparseSlab {
    /// Create an empty sparse slab.
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_list: Vec::new(),
            live_map: indexmap::IndexMap::new(),
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
        let (segment_index, offset) = segments.alloc(len)?;

        // Mark old allocation as dead if it exists.
        if let Some(&old_idx) = self.live_map.get(&field) {
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
