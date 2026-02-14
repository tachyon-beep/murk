//! Read-only snapshot view of a published arena generation.
//!
//! [`Snapshot`] borrows from the published buffer of a [`crate::PingPongArena`] and
//! implements both [`FieldReader`] and [`SnapshotAccess`]. It is the primary
//! interface for observation extraction.

use murk_core::id::{FieldId, ParameterVersion, TickId, WorldGenerationId};
use murk_core::traits::{FieldReader, SnapshotAccess};

use crate::descriptor::FieldDescriptor;
use crate::handle::FieldLocation;
use crate::segment::SegmentList;
use crate::static_arena::{SharedStaticArena, StaticArena};

/// A read-only view of a published arena generation.
///
/// Borrows from the published buffer segments, the sparse segments, and
/// the static arena. All data is immutable for the lifetime of the snapshot.
///
/// # Lifetime
///
/// `'a` is the borrow of the `PingPongArena`. The snapshot cannot outlive
/// the arena. For Phase 1 (Lockstep), this is always fine because
/// `&mut self` on `PingPongArena` means no concurrent snapshot can exist
/// during `begin_tick()`/`publish()`.
///
/// For RealtimeAsync (future WP), this will be refactored to use `Arc`
/// segments so snapshots can be shared across threads.
pub struct Snapshot<'a> {
    /// Per-tick segments from the published buffer.
    per_tick_segments: &'a SegmentList,
    /// Sparse segments (shared between published and staging).
    sparse_segments: &'a SegmentList,
    /// Static arena (generation 0 forever).
    static_arena: &'a StaticArena,
    /// Field descriptor for the published generation.
    descriptor: &'a FieldDescriptor,
    /// Tick when this snapshot was published.
    tick_id: TickId,
    /// Arena generation of this snapshot.
    world_generation_id: WorldGenerationId,
    /// Parameter version at the time of publication.
    parameter_version: ParameterVersion,
}

impl<'a> Snapshot<'a> {
    /// Create a new snapshot.
    pub(crate) fn new(
        per_tick_segments: &'a SegmentList,
        sparse_segments: &'a SegmentList,
        static_arena: &'a StaticArena,
        descriptor: &'a FieldDescriptor,
        tick_id: TickId,
        world_generation_id: WorldGenerationId,
        parameter_version: ParameterVersion,
    ) -> Self {
        Self {
            per_tick_segments,
            sparse_segments,
            static_arena,
            descriptor,
            tick_id,
            world_generation_id,
            parameter_version,
        }
    }

    /// Resolve a field to its data slice by dispatching on the field's location.
    fn resolve_field(&self, field: FieldId) -> Option<&'a [f32]> {
        let entry = self.descriptor.get(field)?;
        let handle = &entry.handle;

        match handle.location() {
            FieldLocation::PerTick { segment_index } => Some(self.per_tick_segments.slice(
                segment_index,
                handle.offset,
                handle.len(),
            )),
            FieldLocation::Sparse { segment_index } => Some(self.sparse_segments.slice(
                segment_index,
                handle.offset,
                handle.len(),
            )),
            FieldLocation::Static { .. } => self.static_arena.read_field(field),
        }
    }
}

impl FieldReader for Snapshot<'_> {
    fn read(&self, field: FieldId) -> Option<&[f32]> {
        self.resolve_field(field)
    }
}

impl SnapshotAccess for Snapshot<'_> {
    fn read_field(&self, field: FieldId) -> Option<&[f32]> {
        self.resolve_field(field)
    }

    fn tick_id(&self) -> TickId {
        self.tick_id
    }

    fn world_generation_id(&self) -> WorldGenerationId {
        self.world_generation_id
    }

    fn parameter_version(&self) -> ParameterVersion {
        self.parameter_version
    }
}

/// An owned, thread-safe snapshot of a published arena generation.
///
/// Unlike [`Snapshot`], which borrows from the `PingPongArena`, this type
/// owns clones of the segment data and an `Arc` reference to the static arena.
/// This makes it `Send + Sync`, allowing it to be shared across threads via
/// `Arc<OwnedSnapshot>` in a ring buffer.
///
/// Created by [`crate::PingPongArena::owned_snapshot()`] for use in
/// RealtimeAsync mode's egress thread pool.
pub struct OwnedSnapshot {
    /// Cloned per-tick segments from the published buffer.
    per_tick_segments: SegmentList,
    /// Cloned sparse segments.
    sparse_segments: SegmentList,
    /// Arc-cloned static arena (cheap reference count bump).
    static_arena: SharedStaticArena,
    /// Cloned field descriptor for this generation.
    descriptor: FieldDescriptor,
    /// Tick when this snapshot was published.
    tick_id: TickId,
    /// Arena generation of this snapshot.
    world_generation_id: WorldGenerationId,
    /// Parameter version at the time of publication.
    parameter_version: ParameterVersion,
}

// Compile-time assertion: OwnedSnapshot must be Send + Sync.
const _: fn() = || {
    fn assert<T: Send + Sync>() {}
    assert::<OwnedSnapshot>();
};

impl OwnedSnapshot {
    /// Create a new owned snapshot from cloned arena data.
    pub(crate) fn new(
        per_tick_segments: SegmentList,
        sparse_segments: SegmentList,
        static_arena: SharedStaticArena,
        descriptor: FieldDescriptor,
        tick_id: TickId,
        world_generation_id: WorldGenerationId,
        parameter_version: ParameterVersion,
    ) -> Self {
        Self {
            per_tick_segments,
            sparse_segments,
            static_arena,
            descriptor,
            tick_id,
            world_generation_id,
            parameter_version,
        }
    }

    /// Resolve a field to its data slice by dispatching on the field's location.
    fn resolve_field(&self, field: FieldId) -> Option<&[f32]> {
        let entry = self.descriptor.get(field)?;
        let handle = &entry.handle;

        match handle.location() {
            FieldLocation::PerTick { segment_index } => Some(self.per_tick_segments.slice(
                segment_index,
                handle.offset,
                handle.len(),
            )),
            FieldLocation::Sparse { segment_index } => Some(self.sparse_segments.slice(
                segment_index,
                handle.offset,
                handle.len(),
            )),
            FieldLocation::Static { .. } => self.static_arena.read_field(field),
        }
    }
}

impl FieldReader for OwnedSnapshot {
    fn read(&self, field: FieldId) -> Option<&[f32]> {
        self.resolve_field(field)
    }
}

impl SnapshotAccess for OwnedSnapshot {
    fn read_field(&self, field: FieldId) -> Option<&[f32]> {
        self.resolve_field(field)
    }

    fn tick_id(&self) -> TickId {
        self.tick_id
    }

    fn world_generation_id(&self) -> WorldGenerationId {
        self.world_generation_id
    }

    fn parameter_version(&self) -> ParameterVersion {
        self.parameter_version
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::descriptor::FieldDescriptor;
    use crate::handle::{FieldHandle, FieldLocation};
    use crate::segment::SegmentList;
    use crate::static_arena::StaticArena;
    use murk_core::{BoundaryBehavior, FieldDef, FieldMutability, FieldType};

    fn make_test_snapshot() -> (SegmentList, SegmentList, StaticArena, FieldDescriptor) {
        let defs = vec![
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
                    name: "terrain".into(),
                    field_type: FieldType::Scalar,
                    mutability: FieldMutability::Static,
                    units: None,
                    bounds: None,
                    boundary_behavior: BoundaryBehavior::Clamp,
                },
            ),
        ];

        let cell_count = 10u32;
        let mut desc = FieldDescriptor::from_field_defs(&defs, cell_count);

        // Set up per-tick segments with data.
        let mut per_tick = SegmentList::new(4096, 4);
        let (seg_idx, offset) = per_tick.alloc(cell_count).unwrap();
        {
            let data = per_tick.slice_mut(seg_idx, offset, cell_count);
            data[0] = 1.0;
            data[9] = 10.0;
        }
        desc.update_handle(
            FieldId(0),
            FieldHandle::new(
                1,
                offset,
                cell_count,
                FieldLocation::PerTick {
                    segment_index: seg_idx,
                },
            ),
        );

        // Set up static arena.
        let mut static_arena = StaticArena::new(&[(FieldId(1), cell_count)]);
        {
            let data = static_arena.write_field(FieldId(1)).unwrap();
            data[0] = 100.0;
        }
        let (s_off, s_len) = static_arena.field_location(FieldId(1)).unwrap();
        desc.update_handle(
            FieldId(1),
            FieldHandle::new(
                0,
                s_off,
                s_len,
                FieldLocation::Static {
                    offset: s_off,
                    len: s_len,
                },
            ),
        );

        let sparse = SegmentList::new(4096, 4);

        (per_tick, sparse, static_arena, desc)
    }

    #[test]
    fn read_per_tick_field() {
        let (per_tick, sparse, static_arena, desc) = make_test_snapshot();
        let snap = Snapshot::new(
            &per_tick,
            &sparse,
            &static_arena,
            &desc,
            TickId(1),
            WorldGenerationId(1),
            ParameterVersion(0),
        );

        let data = snap.read(FieldId(0)).unwrap();
        assert_eq!(data[0], 1.0);
        assert_eq!(data[9], 10.0);
    }

    #[test]
    fn read_static_field() {
        let (per_tick, sparse, static_arena, desc) = make_test_snapshot();
        let snap = Snapshot::new(
            &per_tick,
            &sparse,
            &static_arena,
            &desc,
            TickId(1),
            WorldGenerationId(1),
            ParameterVersion(0),
        );

        let data = snap.read_field(FieldId(1)).unwrap();
        assert_eq!(data[0], 100.0);
    }

    #[test]
    fn snapshot_metadata() {
        let (per_tick, sparse, static_arena, desc) = make_test_snapshot();
        let snap = Snapshot::new(
            &per_tick,
            &sparse,
            &static_arena,
            &desc,
            TickId(42),
            WorldGenerationId(7),
            ParameterVersion(3),
        );

        assert_eq!(snap.tick_id(), TickId(42));
        assert_eq!(snap.world_generation_id(), WorldGenerationId(7));
        assert_eq!(snap.parameter_version(), ParameterVersion(3));
    }

    #[test]
    fn unknown_field_returns_none() {
        let (per_tick, sparse, static_arena, desc) = make_test_snapshot();
        let snap = Snapshot::new(
            &per_tick,
            &sparse,
            &static_arena,
            &desc,
            TickId(1),
            WorldGenerationId(1),
            ParameterVersion(0),
        );

        assert!(snap.read(FieldId(99)).is_none());
    }

    // ── OwnedSnapshot tests ────────────────────────────────────

    use std::sync::Arc;

    fn make_owned_snapshot() -> OwnedSnapshot {
        let (per_tick, sparse, static_arena, desc) = make_test_snapshot();
        let shared_static = Arc::new(static_arena);
        OwnedSnapshot::new(
            per_tick,
            sparse,
            shared_static,
            desc,
            TickId(1),
            WorldGenerationId(1),
            ParameterVersion(0),
        )
    }

    #[test]
    fn test_owned_snapshot_reads_per_tick() {
        let snap = make_owned_snapshot();
        let data = snap.read(FieldId(0)).unwrap();
        assert_eq!(data[0], 1.0);
        assert_eq!(data[9], 10.0);
    }

    #[test]
    fn test_owned_snapshot_reads_static() {
        let snap = make_owned_snapshot();
        let data = snap.read_field(FieldId(1)).unwrap();
        assert_eq!(data[0], 100.0);
    }

    #[test]
    fn test_owned_snapshot_metadata() {
        let (per_tick, sparse, static_arena, desc) = make_test_snapshot();
        let shared_static = Arc::new(static_arena);
        let snap = OwnedSnapshot::new(
            per_tick,
            sparse,
            shared_static,
            desc,
            TickId(42),
            WorldGenerationId(7),
            ParameterVersion(3),
        );
        assert_eq!(snap.tick_id(), TickId(42));
        assert_eq!(snap.world_generation_id(), WorldGenerationId(7));
        assert_eq!(snap.parameter_version(), ParameterVersion(3));
    }

    #[test]
    fn test_owned_snapshot_unknown_field_none() {
        let snap = make_owned_snapshot();
        assert!(snap.read(FieldId(99)).is_none());
    }

    #[test]
    fn test_owned_snapshot_independent_of_source() {
        let (mut per_tick, sparse, static_arena, desc) = make_test_snapshot();
        let shared_static = Arc::new(static_arena);
        let snap = OwnedSnapshot::new(
            per_tick.clone(),
            sparse.clone(),
            shared_static,
            desc,
            TickId(1),
            WorldGenerationId(1),
            ParameterVersion(0),
        );

        // Mutate the original segments.
        let data = per_tick.slice_mut(0, 0, 10);
        data[0] = 999.0;

        // OwnedSnapshot should be unaffected.
        assert_eq!(snap.read(FieldId(0)).unwrap()[0], 1.0);
    }
}
