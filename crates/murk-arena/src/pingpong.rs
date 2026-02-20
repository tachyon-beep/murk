//! Double-buffered ping-pong arena orchestrator.
//!
//! [`PingPongArena`] is the top-level arena type. It maintains two per-tick
//! segment pools (buffer A and buffer B) that alternate between "staging"
//! (writable) and "published" (readable) roles. On [`PingPongArena::publish`], the
//! staging buffer becomes published and the old published buffer becomes
//! the next staging buffer (reset for reuse).
//!
//! The lifecycle per tick is:
//! 1. `begin_tick()` — pre-allocate all PerTick fields in the staging buffer
//! 2. Propagators write via `WriteArena` (from the `TickGuard`)
//! 3. `publish()` — swap buffers, update generation
//! 4. `snapshot()` — borrow published buffer as a `Snapshot`

use std::sync::Arc;

use murk_core::id::{FieldId, ParameterVersion, TickId, WorldGenerationId};
use murk_core::{FieldDef, FieldMutability};

use crate::config::ArenaConfig;
use crate::descriptor::FieldDescriptor;
use crate::error::ArenaError;
use crate::handle::{FieldHandle, FieldLocation};
use crate::read::{OwnedSnapshot, Snapshot};
use crate::scratch::ScratchRegion;
use crate::segment::SegmentList;
use crate::sparse::SparseSlab;
use crate::static_arena::SharedStaticArena;
use crate::write::WriteArena;

/// Tick guard providing write + read access during a tick.
///
/// Created by [`PingPongArena::begin_tick()`] and consumed before
/// [`PingPongArena::publish()`]. Holds mutable borrows into the staging
/// buffer, preventing any other access to the arena during the tick.
#[must_use]
pub struct TickGuard<'a> {
    /// Mutable write access to the staging buffer.
    pub writer: WriteArena<'a>,
    /// Scratch space for temporary propagator allocations.
    pub scratch: &'a mut ScratchRegion,
}

/// Double-buffered arena with ping-pong swap.
///
/// This is the main arena type used by the tick engine. It manages:
/// - Two per-tick segment pools (A and B) that alternate roles
/// - A dedicated sparse segment pool (not ping-pong'd)
/// - A shared static arena for generation-0 data
/// - Two field descriptors (staging and published) that are swapped
///
/// # Buffer layout
///
/// ```text
/// buffer_a: SegmentList  ←─── staging (even generations) / published (odd)
/// buffer_b: SegmentList  ←─── published (even generations) / staging (odd)
/// sparse:   SegmentList  ←─── dedicated, never reset
/// static:   StaticArena  ←─── generation 0 forever
/// ```
pub struct PingPongArena {
    /// Per-tick segment pool A.
    buffer_a: SegmentList,
    /// Per-tick segment pool B.
    buffer_b: SegmentList,
    /// Dedicated sparse segment pool.
    sparse_segments: SegmentList,
    /// Sparse slab for CoW tracking.
    sparse_slab: SparseSlab,
    /// Shared static arena.
    static_arena: SharedStaticArena,
    /// Descriptor for the staging buffer.
    staging_descriptor: FieldDescriptor,
    /// Descriptor for the published buffer.
    published_descriptor: FieldDescriptor,
    /// Current arena generation (incremented on publish).
    generation: u32,
    /// Generation computed by `begin_tick()`, consumed by `publish()`.
    next_generation: u32,
    /// Whether a tick is in progress (`begin_tick()` called, `publish()` not yet called).
    tick_in_progress: bool,
    /// Which buffer is currently staging (false = A staging, true = B staging).
    b_is_staging: bool,
    /// Scratch region for temporary allocations.
    scratch: ScratchRegion,
    /// Arena configuration.
    config: ArenaConfig,
    /// Last published tick ID.
    last_tick_id: TickId,
    /// Last published parameter version.
    last_param_version: ParameterVersion,
    /// Field definitions (kept for reset).
    field_defs: Vec<(FieldId, FieldDef)>,
}

impl PingPongArena {
    /// Create a new ping-pong arena.
    ///
    /// `field_defs` are the registered fields for the simulation world.
    /// `static_arena` should already contain initialised static field data.
    /// `config` controls segment sizing and capacity limits.
    ///
    /// Returns `Err(ArenaError)` if initial sparse allocations fail (e.g.
    /// field size exceeds segment capacity) or if a `Static` field declared
    /// in `field_defs` is missing from the provided `static_arena`.
    pub fn new(
        config: ArenaConfig,
        field_defs: Vec<(FieldId, FieldDef)>,
        static_arena: SharedStaticArena,
    ) -> Result<Self, ArenaError> {
        // Validate segment_size: must be a power of two and at least 1024,
        // as documented on ArenaConfig::segment_size.
        if !config.segment_size.is_power_of_two() || config.segment_size < 1024 {
            return Err(ArenaError::InvalidConfig {
                reason: format!(
                    "segment_size must be a power of two and >= 1024 (got {})",
                    config.segment_size,
                ),
            });
        }

        // Three pools (buffer_a, buffer_b, sparse) each preallocate one
        // segment, so we need at least 3 to satisfy the budget invariant.
        if config.max_segments < 3 {
            return Err(ArenaError::InvalidConfig {
                reason: format!(
                    "max_segments must be >= 3 (got {}); \
                     the arena requires at least one segment per pool \
                     (buffer_a, buffer_b, sparse)",
                    config.max_segments,
                ),
            });
        }

        let descriptor = FieldDescriptor::from_field_defs(&field_defs, config.cell_count)?;

        // Compute per-pool segment budgets that respect the global limit.
        // Three pools share max_segments: buffer_a, buffer_b, sparse.
        // Division: each per-tick buffer gets ⌊max/3⌋, sparse gets the remainder.
        let per_tick_max = config.max_segments / 3;
        let sparse_max = config.max_segments - 2 * per_tick_max;

        // Initial sparse allocations for all Sparse fields.
        let mut sparse_segments = SegmentList::new(config.segment_size, sparse_max);
        let mut sparse_slab = SparseSlab::new();
        let mut staging_descriptor = descriptor.clone();

        for (&field_id, entry) in descriptor.iter() {
            if entry.meta.mutability == FieldMutability::Sparse {
                let handle =
                    sparse_slab.alloc(field_id, entry.meta.total_len, 0, &mut sparse_segments)?;
                staging_descriptor.update_handle(field_id, handle);
            }
            if entry.meta.mutability == FieldMutability::Static {
                let (off, len) = static_arena
                    .field_location(field_id)
                    .ok_or(ArenaError::UnknownField { field: field_id })?;
                let handle =
                    FieldHandle::new(0, off, len, FieldLocation::Static { offset: off, len });
                staging_descriptor.update_handle(field_id, handle);
            }
        }

        // Pre-allocate PerTick fields in both buffers so that reads from
        // the published buffer at generation 0 (before any begin_tick/publish
        // cycle) return valid zero-filled data instead of hitting unallocated
        // memory. This fixes BUG-028 (segment slice beyond cursor) and
        // BUG-013 (placeholder PerTick handles in snapshot).
        let mut buffer_a = SegmentList::new(config.segment_size, per_tick_max);
        let mut buffer_b = SegmentList::new(config.segment_size, per_tick_max);

        let per_tick_fields: Vec<(FieldId, u32)> = staging_descriptor
            .iter()
            .filter(|(_, e)| e.meta.mutability == FieldMutability::PerTick)
            .map(|(&id, e)| (id, e.meta.total_len))
            .collect();

        for (field_id, total_len) in &per_tick_fields {
            // Allocate in buffer_b (initial published buffer when b_is_staging=false).
            let (seg_idx, offset) = buffer_b.alloc(*total_len)?;
            let handle = FieldHandle::new(
                0,
                offset,
                *total_len,
                FieldLocation::PerTick {
                    segment_index: seg_idx,
                },
            );
            staging_descriptor.update_handle(*field_id, handle);

            // Also allocate in buffer_a (will become staging on first begin_tick,
            // where it will be reset and re-allocated — but this makes both
            // buffers consistent from the start).
            let _ = buffer_a.alloc(*total_len)?;
        }

        let published_descriptor = staging_descriptor.clone();

        Ok(Self {
            buffer_a,
            buffer_b,
            sparse_segments,
            sparse_slab,
            static_arena,
            staging_descriptor,
            published_descriptor,
            generation: 0,
            next_generation: 0,
            tick_in_progress: false,
            b_is_staging: false,
            scratch: ScratchRegion::new(config.cell_count as usize * 4),
            config,
            last_tick_id: TickId(0),
            last_param_version: ParameterVersion(0),
            field_defs,
        })
    }

    /// Begin a new tick, pre-allocating all PerTick fields in the staging buffer.
    ///
    /// Returns a [`TickGuard`] providing write access to the staging buffer
    /// and scratch space. The guard must be dropped before calling `publish()`.
    pub fn begin_tick(&mut self) -> Result<TickGuard<'_>, ArenaError> {
        let next_gen = self.generation.checked_add(1).ok_or(ArenaError::InvalidConfig {
            reason: "generation counter overflow (u32::MAX ticks reached)".into(),
        })?;

        // Reset the staging buffer (it was the published buffer last tick).
        if self.b_is_staging {
            self.buffer_b.reset();
        } else {
            self.buffer_a.reset();
        }

        // Collect PerTick field IDs and sizes before mutating segments.
        // (Can't iterate descriptor and mutate segments simultaneously.)
        let per_tick_fields: Vec<(FieldId, u32)> = self
            .staging_descriptor
            .iter()
            .filter(|(_, e)| e.meta.mutability == FieldMutability::PerTick)
            .map(|(&id, e)| (id, e.meta.total_len))
            .collect();

        // Pre-allocate ALL PerTick fields in the staging buffer.
        // This ensures that after publish, the published descriptor points
        // entirely into the published buffer — no dangling handles.
        //
        // We collect allocations first, then update descriptor, to avoid
        // borrowing both &mut segments and &mut descriptor simultaneously.
        let staging = if self.b_is_staging {
            &mut self.buffer_b
        } else {
            &mut self.buffer_a
        };

        let mut alloc_results: Vec<(FieldId, FieldHandle)> =
            Vec::with_capacity(per_tick_fields.len());
        for (field_id, total_len) in &per_tick_fields {
            let (seg_idx, offset) = staging.alloc(*total_len)?;
            let handle = FieldHandle::new(
                next_gen,
                offset,
                *total_len,
                FieldLocation::PerTick {
                    segment_index: seg_idx,
                },
            );
            alloc_results.push((*field_id, handle));
        }

        for (field_id, handle) in alloc_results {
            self.staging_descriptor.update_handle(field_id, handle);
        }

        self.scratch.reset();
        self.tick_in_progress = true;
        self.next_generation = next_gen;

        // Construct TickGuard via helper to get clean split borrows.
        let guard = Self::make_tick_guard(
            if self.b_is_staging {
                &mut self.buffer_b
            } else {
                &mut self.buffer_a
            },
            &mut self.sparse_segments,
            &mut self.sparse_slab,
            &mut self.staging_descriptor,
            &mut self.scratch,
            next_gen,
        );

        Ok(guard)
    }

    /// Helper to construct a TickGuard from split borrows.
    fn make_tick_guard<'a>(
        per_tick_segments: &'a mut SegmentList,
        sparse_segments: &'a mut SegmentList,
        sparse_slab: &'a mut SparseSlab,
        descriptor: &'a mut FieldDescriptor,
        scratch: &'a mut ScratchRegion,
        generation: u32,
    ) -> TickGuard<'a> {
        TickGuard {
            writer: WriteArena::new(
                per_tick_segments,
                sparse_segments,
                sparse_slab,
                descriptor,
                generation,
            ),
            scratch,
        }
    }

    /// Publish the staging buffer, making it the new published generation.
    ///
    /// Returns `Err` if `begin_tick()` was not called first or if
    /// `publish()` is called twice without an intervening `begin_tick()`.
    ///
    /// After this call:
    /// - The staging descriptor becomes the published descriptor
    /// - The staging buffer becomes the published buffer
    /// - The old published buffer will be reset on the next `begin_tick()`
    /// - The generation counter advances to the value computed by `begin_tick()`
    pub fn publish(&mut self, tick_id: TickId, param_version: ParameterVersion) -> Result<(), ArenaError> {
        if !self.tick_in_progress {
            return Err(ArenaError::InvalidConfig {
                reason: "publish() called without a preceding begin_tick()".into(),
            });
        }

        self.generation = self.next_generation;
        self.tick_in_progress = false;

        // Swap descriptors.
        std::mem::swap(&mut self.staging_descriptor, &mut self.published_descriptor);

        // Clone the newly published descriptor back to staging as a starting point.
        // Sparse and Static handles carry over; PerTick handles will be replaced
        // at the next begin_tick().
        self.staging_descriptor = self.published_descriptor.clone();

        // Toggle which buffer is staging.
        self.b_is_staging = !self.b_is_staging;

        self.last_tick_id = tick_id;
        self.last_param_version = param_version;
        Ok(())
    }

    /// Get a read-only snapshot of the published generation.
    pub fn snapshot(&self) -> Snapshot<'_> {
        let published_segments = if self.b_is_staging {
            &self.buffer_a
        } else {
            &self.buffer_b
        };

        Snapshot::new(
            published_segments,
            &self.sparse_segments,
            &self.static_arena,
            &self.published_descriptor,
            self.last_tick_id,
            WorldGenerationId(self.generation as u64),
            self.last_param_version,
        )
    }

    /// Get an owned, thread-safe snapshot of the published generation.
    ///
    /// Unlike [`PingPongArena::snapshot()`], the returned `OwnedSnapshot` owns
    /// clones of the segment data and can be sent across thread boundaries.
    /// Used by RealtimeAsync mode to populate the snapshot ring buffer.
    pub fn owned_snapshot(&self) -> OwnedSnapshot {
        let published_segments = if self.b_is_staging {
            &self.buffer_a
        } else {
            &self.buffer_b
        };
        OwnedSnapshot::new(
            published_segments.clone(),
            self.sparse_segments.clone(),
            Arc::clone(&self.static_arena),
            self.published_descriptor.clone(),
            self.last_tick_id,
            WorldGenerationId(self.generation as u64),
            self.last_param_version,
        )
    }

    /// Access the scratch region (for use outside of tick processing).
    pub fn scratch(&mut self) -> &mut ScratchRegion {
        &mut self.scratch
    }

    /// Reset the arena to its initial state.
    ///
    /// Resets all buffers, the sparse slab, and generation counter.
    /// Static arena is untouched (it's shared and immutable).
    ///
    /// Returns `Err` if sparse re-initialisation fails (same conditions
    /// as [`PingPongArena::new`]).
    pub fn reset(&mut self) -> Result<(), ArenaError> {
        self.buffer_a.reset();
        self.buffer_b.reset();

        let sparse_max = self.config.max_segments - 2 * (self.config.max_segments / 3);
        self.sparse_segments = SegmentList::new(self.config.segment_size, sparse_max);
        self.sparse_slab = SparseSlab::new();

        // Rebuild descriptors from field defs.
        let descriptor = FieldDescriptor::from_field_defs(&self.field_defs, self.config.cell_count)?;
        self.staging_descriptor = descriptor.clone();
        self.published_descriptor = descriptor;

        // Re-initialise sparse and static handle entries.
        for (field_id, def) in &self.field_defs {
            if def.mutability == FieldMutability::Sparse {
                let total_len = self.config.cell_count * def.field_type.components();
                let handle =
                    self.sparse_slab
                        .alloc(*field_id, total_len, 0, &mut self.sparse_segments)?;
                self.staging_descriptor.update_handle(*field_id, handle);
                self.published_descriptor.update_handle(*field_id, handle);
            }
            if def.mutability == FieldMutability::Static {
                let (off, len) = self
                    .static_arena
                    .field_location(*field_id)
                    .ok_or(ArenaError::UnknownField { field: *field_id })?;
                let handle =
                    FieldHandle::new(0, off, len, FieldLocation::Static { offset: off, len });
                self.staging_descriptor.update_handle(*field_id, handle);
                self.published_descriptor.update_handle(*field_id, handle);
            }
        }

        // Pre-allocate PerTick fields in both buffers (same as new()) so
        // the published buffer is valid at generation 0.
        let per_tick_fields: Vec<(FieldId, u32)> = self
            .staging_descriptor
            .iter()
            .filter(|(_, e)| e.meta.mutability == FieldMutability::PerTick)
            .map(|(&id, e)| (id, e.meta.total_len))
            .collect();

        for (field_id, total_len) in &per_tick_fields {
            let (seg_idx, offset) = self.buffer_b.alloc(*total_len)?;
            let handle = FieldHandle::new(
                0,
                offset,
                *total_len,
                FieldLocation::PerTick {
                    segment_index: seg_idx,
                },
            );
            self.staging_descriptor.update_handle(*field_id, handle);
            self.published_descriptor.update_handle(*field_id, handle);
            let _ = self.buffer_a.alloc(*total_len)?;
        }

        self.generation = 0;
        self.next_generation = 0;
        self.tick_in_progress = false;
        self.b_is_staging = false;
        self.last_tick_id = TickId(0);
        self.last_param_version = ParameterVersion(0);
        Ok(())
    }

    /// Total memory usage across all arena buffers in bytes.
    pub fn memory_bytes(&self) -> usize {
        self.buffer_a.memory_bytes()
            + self.buffer_b.memory_bytes()
            + self.sparse_segments.memory_bytes()
            + self.static_arena.memory_bytes()
            + self.scratch.memory_bytes()
    }

    /// Current generation number.
    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Get a reference to the arena config.
    pub fn config(&self) -> &ArenaConfig {
        &self.config
    }

    /// Get a reference to the shared static arena.
    pub fn static_arena(&self) -> &SharedStaticArena {
        &self.static_arena
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::static_arena::StaticArena;
    use murk_core::traits::{FieldReader, FieldWriter, SnapshotAccess};
    use murk_core::{BoundaryBehavior, FieldType};

    fn make_field_defs() -> Vec<(FieldId, FieldDef)> {
        vec![
            (
                FieldId(0),
                FieldDef {
                    name: "temperature".into(),
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
                    name: "velocity".into(),
                    field_type: FieldType::Vector { dims: 3 },
                    mutability: FieldMutability::PerTick,
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
            (
                FieldId(3),
                FieldDef {
                    name: "resources".into(),
                    field_type: FieldType::Scalar,
                    mutability: FieldMutability::Sparse,
                    units: None,
                    bounds: None,
                    boundary_behavior: BoundaryBehavior::Clamp,
                },
            ),
        ]
    }

    fn make_arena() -> PingPongArena {
        let cell_count = 100u32;
        let config = ArenaConfig::new(cell_count);
        let field_defs = make_field_defs();

        // Build static arena with terrain data.
        let static_fields: Vec<(FieldId, u32)> = field_defs
            .iter()
            .filter(|(_, d)| d.mutability == FieldMutability::Static)
            .map(|(id, d)| (*id, cell_count * d.field_type.components()))
            .collect();

        let mut static_arena = StaticArena::new(&static_fields);
        // Fill terrain with recognisable data.
        if let Some(data) = static_arena.write_field(FieldId(2)) {
            for (i, v) in data.iter_mut().enumerate() {
                *v = i as f32;
            }
        }
        let shared_static = static_arena.into_shared();

        PingPongArena::new(config, field_defs, shared_static).unwrap()
    }

    #[test]
    fn new_arena_starts_at_generation_zero() {
        let arena = make_arena();
        assert_eq!(arena.generation(), 0);
    }

    #[test]
    fn begin_tick_and_write() {
        let mut arena = make_arena();
        let mut guard = arena.begin_tick().unwrap();
        let data = guard.writer.write(FieldId(0)).unwrap();
        assert_eq!(data.len(), 100); // cell_count * 1 component
        data[0] = 42.0;
    }

    #[test]
    fn publish_increments_generation() {
        let mut arena = make_arena();
        let _guard = arena.begin_tick().unwrap();
        // Let _guard go out of scope (it doesn't implement Drop).
        let _ = _guard;
        arena.publish(TickId(1), ParameterVersion(0)).unwrap();
        assert_eq!(arena.generation(), 1);
    }

    #[test]
    fn snapshot_reads_published_data() {
        let mut arena = make_arena();

        // Tick 1: write temperature.
        {
            let mut guard = arena.begin_tick().unwrap();
            let data = guard.writer.write(FieldId(0)).unwrap();
            data[0] = 42.0;
            data[99] = 99.0;
        }
        arena.publish(TickId(1), ParameterVersion(0)).unwrap();

        // Read snapshot.
        let snap = arena.snapshot();
        let data = snap.read(FieldId(0)).unwrap();
        assert_eq!(data[0], 42.0);
        assert_eq!(data[99], 99.0);
    }

    #[test]
    fn snapshot_reads_static_fields() {
        let mut arena = make_arena();

        // Even before any tick, static data should be readable.
        // We need at least one publish for the snapshot to be meaningful.
        {
            let _guard = arena.begin_tick().unwrap();
        }
        arena.publish(TickId(1), ParameterVersion(0)).unwrap();

        let snap = arena.snapshot();
        let terrain = snap.read_field(FieldId(2)).unwrap();
        assert_eq!(terrain[0], 0.0);
        assert_eq!(terrain[50], 50.0);
        assert_eq!(terrain[99], 99.0);
    }

    #[test]
    fn snapshot_metadata_matches_publish_args() {
        let mut arena = make_arena();
        {
            let _guard = arena.begin_tick().unwrap();
        }
        arena.publish(TickId(5), ParameterVersion(3)).unwrap();

        let snap = arena.snapshot();
        assert_eq!(snap.tick_id(), TickId(5));
        assert_eq!(snap.parameter_version(), ParameterVersion(3));
    }

    #[test]
    fn ping_pong_alternates_buffers() {
        let mut arena = make_arena();

        // Tick 1: write temp = 1.0
        {
            let mut guard = arena.begin_tick().unwrap();
            let data = guard.writer.write(FieldId(0)).unwrap();
            data[0] = 1.0;
        }
        arena.publish(TickId(1), ParameterVersion(0)).unwrap();

        // Verify tick 1 data in snapshot.
        assert_eq!(arena.snapshot().read(FieldId(0)).unwrap()[0], 1.0);

        // Tick 2: write temp = 2.0 (should be in different buffer).
        {
            let mut guard = arena.begin_tick().unwrap();
            let data = guard.writer.write(FieldId(0)).unwrap();
            // Pre-allocated zeroes (not the old 1.0, because this is a fresh buffer).
            assert_eq!(data[0], 0.0);
            data[0] = 2.0;
        }
        arena.publish(TickId(2), ParameterVersion(0)).unwrap();

        // Verify tick 2 data in snapshot.
        assert_eq!(arena.snapshot().read(FieldId(0)).unwrap()[0], 2.0);
    }

    #[test]
    fn vector_field_has_correct_size() {
        let mut arena = make_arena();
        {
            let mut guard = arena.begin_tick().unwrap();
            let vel = guard.writer.write(FieldId(1)).unwrap();
            // velocity is Vector{dims:3}, cell_count=100, so len = 300.
            assert_eq!(vel.len(), 300);
        }
    }

    #[test]
    fn scratch_resets_between_ticks() {
        let mut arena = make_arena();
        {
            let guard = arena.begin_tick().unwrap();
            guard.scratch.alloc(50).unwrap();
            assert_eq!(guard.scratch.used(), 50);
        }
        arena.publish(TickId(1), ParameterVersion(0)).unwrap();

        // Next tick: scratch should be reset.
        {
            let guard = arena.begin_tick().unwrap();
            assert_eq!(guard.scratch.used(), 0);
        }
    }

    #[test]
    fn reset_returns_to_initial_state() {
        let mut arena = make_arena();

        // Run a few ticks.
        for i in 1..=5 {
            {
                let mut guard = arena.begin_tick().unwrap();
                let data = guard.writer.write(FieldId(0)).unwrap();
                data[0] = i as f32;
            }
            arena.publish(TickId(i), ParameterVersion(0)).unwrap();
        }
        assert_eq!(arena.generation(), 5);

        arena.reset().unwrap();
        assert_eq!(arena.generation(), 0);
    }

    #[test]
    fn memory_bytes_is_positive() {
        let arena = make_arena();
        assert!(arena.memory_bytes() > 0);
    }

    #[test]
    fn multi_tick_round_trip() {
        let mut arena = make_arena();

        for tick in 1u64..=10 {
            {
                let mut guard = arena.begin_tick().unwrap();
                let data = guard.writer.write(FieldId(0)).unwrap();
                data[0] = tick as f32;
            }
            arena.publish(TickId(tick), ParameterVersion(0)).unwrap();

            let snap = arena.snapshot();
            assert_eq!(snap.read(FieldId(0)).unwrap()[0], tick as f32);
            assert_eq!(snap.tick_id(), TickId(tick));
        }
    }

    #[test]
    fn sparse_field_persists_across_ticks() {
        let mut arena = make_arena();

        // Tick 1: write sparse field.
        {
            let mut guard = arena.begin_tick().unwrap();
            let data = guard.writer.write(FieldId(3)).unwrap();
            data[0] = 77.0;
        }
        arena.publish(TickId(1), ParameterVersion(0)).unwrap();

        // Tick 2: don't write sparse field — it should persist.
        {
            let _guard = arena.begin_tick().unwrap();
        }
        arena.publish(TickId(2), ParameterVersion(0)).unwrap();

        let snap = arena.snapshot();
        let data = snap.read(FieldId(3)).unwrap();
        assert_eq!(data[0], 77.0);
    }

    #[test]
    fn new_fails_when_static_field_missing_from_static_arena() {
        let cell_count = 100u32;
        let config = ArenaConfig::new(cell_count);
        let field_defs = vec![(
            FieldId(0),
            FieldDef {
                name: "terrain".into(),
                field_type: FieldType::Scalar,
                mutability: FieldMutability::Static,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            },
        )];
        // Empty static arena — FieldId(0) is not present.
        let static_arena = StaticArena::new(&[]).into_shared();
        let result = PingPongArena::new(config, field_defs, static_arena);
        assert!(matches!(
            result,
            Err(ArenaError::UnknownField { field: FieldId(0) })
        ));
    }

    #[test]
    fn new_fails_when_sparse_field_exceeds_segment_size() {
        let cell_count = 2000u32;
        // Minimum valid segment (1024) that can't fit the sparse field (2000).
        let config = ArenaConfig {
            segment_size: 1024,
            max_segments: 16,
            max_generation_age: 1,
            cell_count,
        };
        let field_defs = vec![(
            FieldId(0),
            FieldDef {
                name: "resource".into(),
                field_type: FieldType::Scalar,
                mutability: FieldMutability::Sparse,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            },
        )];
        let static_arena = StaticArena::new(&[]).into_shared();
        let result = PingPongArena::new(config, field_defs, static_arena);
        assert!(matches!(result, Err(ArenaError::CapacityExceeded { .. })));
    }

    #[test]
    fn new_rejects_max_segments_below_3() {
        let cell_count = 10u32;
        let static_arena = StaticArena::new(&[]).into_shared();
        let field_defs = vec![(
            FieldId(0),
            FieldDef {
                name: "temp".into(),
                field_type: FieldType::Scalar,
                mutability: FieldMutability::PerTick,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            },
        )];

        for bad_max in [0u16, 1, 2] {
            let config = ArenaConfig {
                segment_size: 1024,
                max_segments: bad_max,
                max_generation_age: 1,
                cell_count,
            };
            let result = PingPongArena::new(config, field_defs.clone(), static_arena.clone());
            assert!(
                matches!(result, Err(ArenaError::InvalidConfig { .. })),
                "max_segments={bad_max} should be rejected"
            );
        }
    }

    #[test]
    fn new_accepts_max_segments_of_3() {
        let cell_count = 10u32;
        let static_arena = StaticArena::new(&[]).into_shared();
        let field_defs = vec![(
            FieldId(0),
            FieldDef {
                name: "temp".into(),
                field_type: FieldType::Scalar,
                mutability: FieldMutability::PerTick,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            },
        )];
        let config = ArenaConfig {
            segment_size: 1024,
            max_segments: 3,
            max_generation_age: 1,
            cell_count,
        };
        assert!(PingPongArena::new(config, field_defs, static_arena).is_ok());
    }

    #[test]
    fn test_owned_snapshot_from_arena() {
        let mut arena = make_arena();

        // Tick 1: write temperature.
        {
            let mut guard = arena.begin_tick().unwrap();
            let data = guard.writer.write(FieldId(0)).unwrap();
            data[0] = 42.0;
            data[99] = 99.0;
        }
        arena.publish(TickId(1), ParameterVersion(0)).unwrap();

        let owned = arena.owned_snapshot();
        let data = owned.read_field(FieldId(0)).unwrap();
        assert_eq!(data[0], 42.0);
        assert_eq!(data[99], 99.0);

        // Static field should also be readable.
        let terrain = owned.read_field(FieldId(2)).unwrap();
        assert_eq!(terrain[50], 50.0);

        // Metadata should match.
        assert_eq!(owned.tick_id(), TickId(1));
    }

    #[test]
    fn test_owned_snapshot_survives_mutation() {
        let mut arena = make_arena();

        // Tick 1.
        {
            let mut guard = arena.begin_tick().unwrap();
            let data = guard.writer.write(FieldId(0)).unwrap();
            data[0] = 42.0;
        }
        arena.publish(TickId(1), ParameterVersion(0)).unwrap();

        let owned = arena.owned_snapshot();

        // Tick 2: mutate the arena.
        {
            let mut guard = arena.begin_tick().unwrap();
            let data = guard.writer.write(FieldId(0)).unwrap();
            data[0] = 999.0;
        }
        arena.publish(TickId(2), ParameterVersion(0)).unwrap();

        // OwnedSnapshot from tick 1 should be unaffected.
        assert_eq!(owned.read_field(FieldId(0)).unwrap()[0], 42.0);
        assert_eq!(owned.tick_id(), TickId(1));

        // New snapshot should see tick 2 data.
        let snap = arena.snapshot();
        assert_eq!(snap.read_field(FieldId(0)).unwrap()[0], 999.0);
    }

    #[test]
    fn global_segment_budget_is_respected() {
        // With max_segments = 6, each per-tick buffer gets 2, sparse gets 2.
        // Total segments across all pools should never exceed 6.
        let cell_count = 10u32;
        let config = ArenaConfig {
            segment_size: 1024,
            max_segments: 6,
            max_generation_age: 1,
            cell_count,
        };
        let field_defs = vec![(
            FieldId(0),
            FieldDef {
                name: "temp".into(),
                field_type: FieldType::Scalar,
                mutability: FieldMutability::PerTick,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            },
        )];
        let static_arena = StaticArena::new(&[]).into_shared();
        let arena = PingPongArena::new(config, field_defs, static_arena).unwrap();

        // Verify memory is bounded: total segments should not exceed max_segments.
        // Each pool starts with 1 segment, so 3 segments total initially.
        // Maximum: per_tick_a(2) + per_tick_b(2) + sparse(2) = 6 = max_segments.
        let total_bytes = arena.memory_bytes();
        let max_allowed = 6 * 1024 * std::mem::size_of::<f32>();
        // memory_bytes includes static + scratch; just verify it's bounded.
        assert!(total_bytes <= max_allowed + arena.static_arena().memory_bytes() + 1024 * 4);
    }

    // ── segment_size validation ──────────────────────────────

    #[test]
    fn new_rejects_non_power_of_two_segment_size() {
        let config = ArenaConfig {
            segment_size: 1000, // not a power of two
            max_segments: 16,
            max_generation_age: 1,
            cell_count: 10,
        };
        let static_arena = StaticArena::new(&[]).into_shared();
        let result = PingPongArena::new(config, vec![], static_arena);
        assert!(
            matches!(result, Err(ArenaError::InvalidConfig { .. })),
            "segment_size=1000 (not power of two) should be rejected"
        );
    }

    #[test]
    fn new_rejects_segment_size_below_1024() {
        let config = ArenaConfig {
            segment_size: 512, // power of two but below 1024
            max_segments: 16,
            max_generation_age: 1,
            cell_count: 10,
        };
        let static_arena = StaticArena::new(&[]).into_shared();
        let result = PingPongArena::new(config, vec![], static_arena);
        assert!(
            matches!(result, Err(ArenaError::InvalidConfig { .. })),
            "segment_size=512 (below 1024) should be rejected"
        );
    }

    #[test]
    fn new_accepts_segment_size_of_1024() {
        let config = ArenaConfig {
            segment_size: 1024,
            max_segments: 16,
            max_generation_age: 1,
            cell_count: 10,
        };
        let static_arena = StaticArena::new(&[]).into_shared();
        assert!(PingPongArena::new(config, vec![], static_arena).is_ok());
    }

    // ── publish state guard (#54) ──────────────────────────────

    #[test]
    fn publish_without_begin_tick_returns_error() {
        let mut arena = make_arena();
        let result = arena.publish(TickId(1), ParameterVersion(0));
        assert!(matches!(result, Err(ArenaError::InvalidConfig { .. })));
    }

    #[test]
    fn double_publish_returns_error() {
        let mut arena = make_arena();
        {
            let _guard = arena.begin_tick().unwrap();
        }
        arena.publish(TickId(1), ParameterVersion(0)).unwrap();
        // Second publish without begin_tick should fail.
        let result = arena.publish(TickId(2), ParameterVersion(0));
        assert!(matches!(result, Err(ArenaError::InvalidConfig { .. })));
    }
}
