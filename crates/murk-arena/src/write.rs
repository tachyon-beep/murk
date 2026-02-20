//! Mutable arena access for the staging buffer during a tick.
//!
//! [`WriteArena`] provides [`FieldWriter`] implementation by holding mutable
//! references to the staging buffer's segments, sparse slab, and the field
//! descriptor. It is created by `PingPongArena::begin_tick()` and dropped
//! before `publish()`.

use murk_core::traits::FieldWriter;
use murk_core::{FieldId, FieldMutability};

use crate::descriptor::FieldDescriptor;
use crate::handle::FieldLocation;
use crate::segment::SegmentList;
use crate::sparse::SparseSlab;

/// Mutable access to the staging buffer for a single tick.
///
/// Created by [`PingPongArena::begin_tick()`](crate::PingPongArena::begin_tick).
/// Implements [`FieldWriter`] for propagator write access.
///
/// # Borrow-checker design
///
/// The split-borrow problem: `write()` needs to (1) look up field metadata
/// and (2) mutate segment data. If both lived in the same struct, `&self`
/// for lookup and `&mut self.segments` for data would conflict.
///
/// Solution: metadata (`FieldMeta`) is stored in a separate `IndexMap` that
/// is borrowed immutably, while segments are borrowed mutably. The
/// `FieldDescriptor` is used only to map FieldId → handle; actual segment
/// access goes through the handle's location enum.
pub struct WriteArena<'a> {
    /// Per-tick segment pool (staging buffer).
    per_tick_segments: &'a mut SegmentList,
    /// Sparse field segments (dedicated pool).
    sparse_segments: &'a mut SegmentList,
    /// Sparse slab for CoW tracking.
    sparse_slab: &'a mut SparseSlab,
    /// Field descriptor (staging copy — handles point into staging buffer).
    descriptor: &'a mut FieldDescriptor,
    /// Current generation being staged.
    generation: u32,
}

impl<'a> WriteArena<'a> {
    /// Create a new write arena for the current tick.
    ///
    /// # Safety contract (logical, not `unsafe`)
    ///
    /// The caller must ensure that `descriptor` handles point into
    /// `per_tick_segments` (for PerTick fields) or `sparse_segments`
    /// (for Sparse fields). This is guaranteed by `PingPongArena::begin_tick()`
    /// which pre-allocates all PerTick fields before creating the `WriteArena`.
    pub(crate) fn new(
        per_tick_segments: &'a mut SegmentList,
        sparse_segments: &'a mut SegmentList,
        sparse_slab: &'a mut SparseSlab,
        descriptor: &'a mut FieldDescriptor,
        generation: u32,
    ) -> Self {
        Self {
            per_tick_segments,
            sparse_segments,
            sparse_slab,
            descriptor,
            generation,
        }
    }

    /// Write a sparse field (CoW: allocate new copy in sparse segments).
    fn write_sparse(&mut self, field: FieldId, total_len: u32) -> Option<&mut [f32]> {
        // Allocate new storage for this generation.
        let handle = self
            .sparse_slab
            .alloc(field, total_len, self.generation, self.sparse_segments)
            .ok()?;

        // If there was a previous allocation, copy its data.
        if let Some(old_handle) = {
            // Look up old handle from descriptor before we update it.
            self.descriptor
                .get(field)
                .map(|e| e.handle)
                .filter(|h| h.generation() != self.generation)
        } {
            if let FieldLocation::Sparse {
                segment_index: old_seg,
            } = old_handle.location()
            {
                // Copy old data to new allocation.
                // We need to copy through a temp buffer to satisfy borrow checker
                // (can't have &segments and &mut segments simultaneously).
                let old_data: Vec<f32> = self
                    .sparse_segments
                    .slice(old_seg, old_handle.offset, old_handle.len())?
                    .to_vec();

                if let FieldLocation::Sparse {
                    segment_index: new_seg,
                } = handle.location()
                {
                    let new_data =
                        self.sparse_segments
                            .slice_mut(new_seg, handle.offset, handle.len())?;
                    let copy_len = old_data.len().min(new_data.len());
                    new_data[..copy_len].copy_from_slice(&old_data[..copy_len]);
                }
            }
        }

        // Update descriptor to point to new allocation.
        self.descriptor.update_handle(field, handle);

        // Return mutable slice to the new allocation.
        if let FieldLocation::Sparse { segment_index } = handle.location() {
            self.sparse_segments
                .slice_mut(segment_index, handle.offset, handle.len())
        } else {
            None
        }
    }

    /// Read a field's data from the staging buffer (for CoW copy-before-write).
    pub fn read(&self, field: FieldId) -> Option<&[f32]> {
        let entry = self.descriptor.get(field)?;
        let handle = &entry.handle;
        match handle.location() {
            FieldLocation::PerTick { segment_index } => self.per_tick_segments.slice(
                segment_index,
                handle.offset,
                handle.len(),
            ),
            FieldLocation::Sparse { segment_index } => self.sparse_segments.slice(
                segment_index,
                handle.offset,
                handle.len(),
            ),
            FieldLocation::Static { .. } => {
                // Static fields are read from the StaticArena, not through WriteArena.
                None
            }
        }
    }
}

impl FieldWriter for WriteArena<'_> {
    fn write(&mut self, field: FieldId) -> Option<&mut [f32]> {
        let entry = self.descriptor.get(field)?;
        // Extract only the scalar values we need — avoids cloning the
        // entire FieldMeta (which previously heap-allocated a String
        // copy on every write call).
        let mutability = entry.meta.mutability;
        let total_len = entry.meta.total_len;
        let handle = entry.handle;

        match mutability {
            FieldMutability::PerTick => {
                // PerTick fields were pre-allocated at begin_tick().
                // Just return a mutable slice to the pre-allocated region.
                if let FieldLocation::PerTick { segment_index } = handle.location() {
                    self.per_tick_segments.slice_mut(
                        segment_index,
                        handle.offset,
                        handle.len(),
                    )
                } else {
                    None
                }
            }
            FieldMutability::Sparse => {
                // CoW: if this field hasn't been written this generation,
                // allocate new storage and copy the old data.
                if handle.generation() == self.generation {
                    // Already written this tick — return existing allocation.
                    if let FieldLocation::Sparse { segment_index } = handle.location() {
                        self.sparse_segments.slice_mut(
                            segment_index,
                            handle.offset,
                            handle.len(),
                        )
                    } else {
                        None
                    }
                } else {
                    self.write_sparse(field, total_len)
                }
            }
            FieldMutability::Static => {
                // Static fields cannot be written after initialization.
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::FieldDescriptor;
    use crate::handle::FieldHandle;
    use crate::segment::SegmentList;
    use crate::sparse::SparseSlab;
    use murk_core::{BoundaryBehavior, FieldDef, FieldType};

    fn make_defs() -> Vec<(FieldId, FieldDef)> {
        vec![
            (
                FieldId(0),
                FieldDef {
                    name: "temp".into(),
                    field_type: FieldType::Scalar,
                    mutability: FieldMutability::PerTick,
                    units: None,
                    bounds: None,
                    boundary_behavior: BoundaryBehavior::Clamp,
                },
            ),
            (
                FieldId(1),
                FieldDef {
                    name: "resource".into(),
                    field_type: FieldType::Scalar,
                    mutability: FieldMutability::Sparse,
                    units: None,
                    bounds: None,
                    boundary_behavior: BoundaryBehavior::Clamp,
                },
            ),
            (
                FieldId(2),
                FieldDef {
                    name: "terrain".into(),
                    field_type: FieldType::Scalar,
                    mutability: FieldMutability::Static,
                    units: None,
                    bounds: None,
                    boundary_behavior: BoundaryBehavior::Clamp,
                },
            ),
        ]
    }

    /// Helper: set up a WriteArena with pre-allocated PerTick fields.
    fn setup_write_arena(
        cell_count: u32,
        generation: u32,
    ) -> (SegmentList, SegmentList, SparseSlab, FieldDescriptor) {
        let defs = make_defs();
        let mut desc = FieldDescriptor::from_field_defs(&defs, cell_count).unwrap();
        let mut per_tick = SegmentList::new(4096, 4);
        let sparse_segs = SegmentList::new(4096, 4);
        let slab = SparseSlab::new();

        // Pre-allocate PerTick fields (simulates begin_tick).
        let per_tick_fields: Vec<(FieldId, u32)> = desc
            .iter()
            .filter(|(_, e)| e.meta.mutability == FieldMutability::PerTick)
            .map(|(&id, e)| (id, e.meta.total_len))
            .collect();

        for (field_id, total_len) in per_tick_fields {
            let (seg_idx, offset) = per_tick.alloc(total_len).unwrap();
            let handle = FieldHandle::new(
                generation,
                offset,
                total_len,
                FieldLocation::PerTick {
                    segment_index: seg_idx,
                },
            );
            desc.update_handle(field_id, handle);
        }

        (per_tick, sparse_segs, slab, desc)
    }

    #[test]
    fn write_per_tick_returns_mutable_slice() {
        let (mut per_tick, mut sparse_segs, mut slab, mut desc) = setup_write_arena(10, 1);
        let mut wa = WriteArena::new(&mut per_tick, &mut sparse_segs, &mut slab, &mut desc, 1);
        let data = wa.write(FieldId(0)).unwrap();
        assert_eq!(data.len(), 10);
        data[0] = 42.0;
        assert_eq!(data[0], 42.0);
    }

    #[test]
    fn write_static_returns_none() {
        let (mut per_tick, mut sparse_segs, mut slab, mut desc) = setup_write_arena(10, 1);
        let mut wa = WriteArena::new(&mut per_tick, &mut sparse_segs, &mut slab, &mut desc, 1);
        assert!(wa.write(FieldId(2)).is_none());
    }

    #[test]
    fn write_sparse_allocates_new_copy() {
        let (mut per_tick, mut sparse_segs, mut slab, mut desc) = setup_write_arena(10, 1);

        // Initial sparse allocation at gen 0 (simulating prior state).
        let handle = slab.alloc(FieldId(1), 10, 0, &mut sparse_segs).unwrap();
        desc.update_handle(FieldId(1), handle);

        let mut wa = WriteArena::new(&mut per_tick, &mut sparse_segs, &mut slab, &mut desc, 1);

        // Writing should trigger CoW.
        let data = wa.write(FieldId(1)).unwrap();
        assert_eq!(data.len(), 10);
        data[0] = 99.0;
    }

    #[test]
    fn write_unknown_field_returns_none() {
        let (mut per_tick, mut sparse_segs, mut slab, mut desc) = setup_write_arena(10, 1);
        let mut wa = WriteArena::new(&mut per_tick, &mut sparse_segs, &mut slab, &mut desc, 1);
        assert!(wa.write(FieldId(99)).is_none());
    }

    #[test]
    fn read_per_tick_returns_data() {
        let (mut per_tick, mut sparse_segs, mut slab, mut desc) = setup_write_arena(10, 1);
        // Write some data first.
        {
            let mut wa = WriteArena::new(&mut per_tick, &mut sparse_segs, &mut slab, &mut desc, 1);
            let data = wa.write(FieldId(0)).unwrap();
            data[0] = 7.0;
        }
        // Now read it.
        let wa = WriteArena::new(&mut per_tick, &mut sparse_segs, &mut slab, &mut desc, 1);
        let data = wa.read(FieldId(0)).unwrap();
        assert_eq!(data[0], 7.0);
    }
}
