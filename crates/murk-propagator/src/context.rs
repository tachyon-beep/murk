//! Execution context passed to propagators during tick execution.
//!
//! [`StepContext`] provides split-borrow field access through two read views
//! (in-tick overlay and frozen tick-start) plus mutable write access, scratch
//! memory, and the spatial topology.

use crate::scratch::ScratchRegion;
use murk_core::{FieldReader, FieldWriter, TickId};
use murk_entity::{EntityOverlayReader, EntitySnapshot, PropertyStaging};
use murk_space::Space;

/// Execution context passed to each propagator's `step()` method.
///
/// Uses dynamic dispatch (`&dyn FieldReader`, `&mut dyn FieldWriter`) to
/// keep the [`Propagator`](crate::Propagator) trait object-safe while
/// supporting mock-based testing.
///
/// # Split-borrow semantics
///
/// - **`reads()`** returns the in-tick overlay view. A propagator reading
///   field X sees the most recent staged write from a prior propagator,
///   or the base generation if no prior propagator wrote X. This enables
///   Euler-style sequential integration.
///
/// - **`reads_previous()`** returns the frozen tick-start view. Always sees
///   the base generation regardless of prior writes. This enables
///   Jacobi-style parallel integration.
pub struct StepContext<'a> {
    reads: &'a dyn FieldReader,
    reads_previous: &'a dyn FieldReader,
    writes: &'a mut dyn FieldWriter,
    scratch: &'a mut ScratchRegion,
    space: &'a dyn Space,
    entity_snapshot: Option<EntitySnapshot<'a>>,
    entity_staging: Option<&'a mut PropertyStaging>,
    tick_id: TickId,
    dt: f64,
}

impl<'a> StepContext<'a> {
    /// Construct a new step context.
    ///
    /// Typically called by the engine, not by propagators directly.
    /// For testing, construct with mock readers/writers from `murk-test-utils`.
    pub fn new(
        reads: &'a dyn FieldReader,
        reads_previous: &'a dyn FieldReader,
        writes: &'a mut dyn FieldWriter,
        scratch: &'a mut ScratchRegion,
        space: &'a dyn Space,
        tick_id: TickId,
        dt: f64,
    ) -> Self {
        Self {
            reads,
            reads_previous,
            writes,
            scratch,
            space,
            entity_snapshot: None,
            entity_staging: None,
            tick_id,
            dt,
        }
    }

    /// Construct a step context with entity access enabled.
    ///
    /// Typically called by the engine after command application, so entity
    /// reads include entities spawned or moved earlier in the current tick.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_entities(
        reads: &'a dyn FieldReader,
        reads_previous: &'a dyn FieldReader,
        writes: &'a mut dyn FieldWriter,
        scratch: &'a mut ScratchRegion,
        space: &'a dyn Space,
        entity_snapshot: EntitySnapshot<'a>,
        entity_staging: &'a mut PropertyStaging,
        tick_id: TickId,
        dt: f64,
    ) -> Self {
        Self {
            reads,
            reads_previous,
            writes,
            scratch,
            space,
            entity_snapshot: Some(entity_snapshot),
            entity_staging: Some(entity_staging),
            tick_id,
            dt,
        }
    }

    /// In-tick overlay reader.
    ///
    /// Sees staged writes from prior propagators in this tick.
    pub fn reads(&self) -> &dyn FieldReader {
        self.reads
    }

    /// Frozen tick-start reader.
    ///
    /// Always sees the base generation, ignoring in-tick writes.
    pub fn reads_previous(&self) -> &dyn FieldReader {
        self.reads_previous
    }

    /// Mutable field writer for the current propagator's declared outputs.
    pub fn writes(&mut self) -> &mut dyn FieldWriter {
        self.writes
    }

    /// Entity snapshot visible to the current propagator.
    ///
    /// Returns `None` when the world was not configured with entity support.
    pub fn entities(&self) -> Option<EntitySnapshot<'_>> {
        self.entity_snapshot
    }

    /// Frozen entity snapshot from the start of this propagator's dispatch.
    ///
    /// Returns `None` when the world was not configured with entity support.
    pub fn entities_previous(&self) -> Option<EntitySnapshot<'_>> {
        self.entity_snapshot
    }

    /// Euler-style entity reader with staged property writes overlaid.
    ///
    /// Returns `None` when the world was not configured with entity support.
    pub fn entities_overlaid(&self) -> Option<EntityOverlayReader<'_, '_>> {
        let snapshot = self.entity_snapshot?;
        let staging = self.entity_staging.as_deref()?;
        Some(EntityOverlayReader::new(snapshot, staging))
    }

    /// Mutable entity property staging for the current tick.
    ///
    /// Staged values are visible through [`Self::entities`] and are committed
    /// to the entity store only after the full propagator pipeline succeeds.
    pub fn entity_writes(&mut self) -> Option<&mut PropertyStaging> {
        self.entity_staging.as_deref_mut()
    }

    /// Scratch memory allocator. Reset between propagators.
    pub fn scratch(&mut self) -> &mut ScratchRegion {
        self.scratch
    }

    /// Spatial topology. Use `space().downcast_ref::<T>()` for
    /// topology-specific optimizations.
    pub fn space(&self) -> &dyn Space {
        self.space
    }

    /// Current tick ID.
    pub fn tick_id(&self) -> TickId {
        self.tick_id
    }

    /// Configured timestep in seconds.
    pub fn dt(&self) -> f64 {
        self.dt
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::ScratchRegion;
    use murk_core::{EntityManifest, FieldId, PropertyIndex};
    use murk_entity::{EntityStore, PropertyStaging};
    use murk_space::{EdgeBehavior, Line1D};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    #[test]
    fn context_provides_reads_and_writes() {
        let field_a = FieldId(0);
        let mut reader = MockFieldReader::new();
        reader.set_field(field_a, vec![1.0, 2.0, 3.0]);
        let mut writer = MockFieldWriter::new();
        writer.add_field(field_a, 3);

        let mut scratch = ScratchRegion::new(0);
        let space = Line1D::new(3, EdgeBehavior::Absorb).unwrap();

        let mut ctx = StepContext::new(
            &reader,
            &reader,
            &mut writer,
            &mut scratch,
            &space,
            TickId(1),
            0.1,
        );

        // Read
        let data = ctx.reads().read(field_a).unwrap();
        assert_eq!(data, &[1.0, 2.0, 3.0]);

        // Write
        let out = ctx.writes().write(field_a).unwrap();
        out.copy_from_slice(&[10.0, 20.0, 30.0]);

        // Metadata
        assert_eq!(ctx.tick_id(), TickId(1));
        assert_eq!(ctx.dt(), 0.1);
        assert_eq!(ctx.space().cell_count(), 3);
    }

    #[test]
    fn split_borrow_reads_vs_reads_previous() {
        let field_a = FieldId(0);

        // "reads" shows overlaid data (simulating a prior propagator's write)
        let mut overlay = MockFieldReader::new();
        overlay.set_field(field_a, vec![10.0, 20.0]);

        // "reads_previous" shows base generation
        let mut base = MockFieldReader::new();
        base.set_field(field_a, vec![1.0, 2.0]);

        let mut writer = MockFieldWriter::new();
        let mut scratch = ScratchRegion::new(0);
        let space = Line1D::new(2, EdgeBehavior::Absorb).unwrap();

        let ctx = StepContext::new(
            &overlay,
            &base,
            &mut writer,
            &mut scratch,
            &space,
            TickId(0),
            0.1,
        );

        assert_eq!(ctx.reads().read(field_a).unwrap(), &[10.0, 20.0]);
        assert_eq!(ctx.reads_previous().read(field_a).unwrap(), &[1.0, 2.0]);
    }

    #[test]
    fn scratch_is_accessible() {
        let reader = MockFieldReader::new();
        let mut writer = MockFieldWriter::new();
        let mut scratch = ScratchRegion::new(16);
        let space = Line1D::new(4, EdgeBehavior::Absorb).unwrap();

        let mut ctx = StepContext::new(
            &reader,
            &reader,
            &mut writer,
            &mut scratch,
            &space,
            TickId(0),
            0.1,
        );

        let buf = ctx.scratch().alloc(8).unwrap();
        assert_eq!(buf.len(), 8);
    }

    #[test]
    fn entity_access_is_optional() {
        let reader = MockFieldReader::new();
        let mut writer = MockFieldWriter::new();
        let mut scratch = ScratchRegion::new(16);
        let space = Line1D::new(4, EdgeBehavior::Absorb).unwrap();

        let mut ctx = StepContext::new(
            &reader,
            &reader,
            &mut writer,
            &mut scratch,
            &space,
            TickId(0),
            0.1,
        );

        assert!(ctx.entities().is_none());
        assert!(ctx.entities_previous().is_none());
        assert!(ctx.entities_overlaid().is_none());
        assert!(ctx.entity_writes().is_none());
    }

    #[test]
    fn context_exposes_entities_and_entity_writes() {
        let manifest = EntityManifest {
            property_names: vec!["alive".into(), "hp".into()],
            property_defaults: vec![1.0, 100.0],
            alive_property: PropertyIndex(0),
        };
        let mut store = EntityStore::new(4, manifest);
        let id = store.spawn(vec![0].into(), 0, &[]).unwrap();
        let snapshot = store.snapshot();
        let mut staging = PropertyStaging::new(4, 2);

        let reader = MockFieldReader::new();
        let mut writer = MockFieldWriter::new();
        let mut scratch = ScratchRegion::new(16);
        let space = Line1D::new(4, EdgeBehavior::Absorb).unwrap();

        let mut ctx = StepContext::new_with_entities(
            &reader,
            &reader,
            &mut writer,
            &mut scratch,
            &space,
            snapshot,
            &mut staging,
            TickId(1),
            0.1,
        );

        assert_eq!(
            ctx.entities().unwrap().property(id, PropertyIndex(1)),
            Some(100.0)
        );
        assert!(ctx.entity_writes().unwrap().set(id, PropertyIndex(1), 25.0));
        assert_eq!(
            ctx.entities_overlaid()
                .unwrap()
                .property(id, PropertyIndex(1)),
            Some(25.0)
        );
    }
}
