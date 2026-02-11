//! Plan cache with automatic generation-based invalidation.
//!
//! [`ObsPlanCache`] wraps an [`ObsSpec`] and lazily compiles an
//! [`ObsPlan`] on first use. Subsequent calls to [`ObsPlanCache::get_or_compile`]
//! return the cached plan if the snapshot's `world_generation_id`
//! matches the plan's compiled generation; otherwise the plan is
//! recompiled automatically.

use murk_core::error::ObsError;
use murk_core::{SnapshotAccess, TickId, WorldGenerationId};
use murk_space::Space;

use crate::metadata::ObsMetadata;
use crate::spec::ObsSpec;
use crate::ObsPlan;

/// Cached observation plan with generation-based invalidation.
///
/// Holds an [`ObsSpec`] and an optional compiled [`ObsPlan`]. On each
/// call to [`execute`](Self::execute), checks whether the cached plan's
/// generation matches the snapshot's generation. On mismatch, the plan
/// is recompiled transparently.
///
/// # Example
///
/// ```ignore
/// let mut cache = ObsPlanCache::new(spec);
/// // First call compiles the plan:
/// let meta = cache.execute(&space, &snapshot, None, &mut output, &mut mask)?;
/// // Subsequent calls reuse it (same generation):
/// let meta = cache.execute(&space, &snapshot, None, &mut output, &mut mask)?;
/// ```
#[derive(Debug)]
pub struct ObsPlanCache {
    spec: ObsSpec,
    cached: Option<CachedPlan>,
}

/// Internal: a compiled plan with its generation and layout info.
#[derive(Debug)]
struct CachedPlan {
    plan: ObsPlan,
    generation: WorldGenerationId,
    output_len: usize,
    mask_len: usize,
    entry_shapes: Vec<Vec<usize>>,
}

impl ObsPlanCache {
    /// Create a new cache for the given observation spec.
    ///
    /// The plan is not compiled until the first call to
    /// [`execute`](Self::execute) or [`get_or_compile`](Self::get_or_compile).
    pub fn new(spec: ObsSpec) -> Self {
        Self { spec, cached: None }
    }

    /// Get the cached plan, recompiling if needed.
    ///
    /// Returns the plan and its layout info. The plan is recompiled
    /// if no cached plan exists or if `generation` differs from the
    /// cached plan's generation.
    pub fn get_or_compile(
        &mut self,
        space: &dyn Space,
        generation: WorldGenerationId,
    ) -> Result<&ObsPlan, ObsError> {
        let needs_recompile = match &self.cached {
            None => true,
            Some(cached) => cached.generation != generation,
        };

        if needs_recompile {
            let result = ObsPlan::compile_bound(&self.spec, space, generation)?;
            self.cached = Some(CachedPlan {
                plan: result.plan,
                generation,
                output_len: result.output_len,
                mask_len: result.mask_len,
                entry_shapes: result.entry_shapes,
            });
        }

        Ok(&self.cached.as_ref().unwrap().plan)
    }

    /// Execute the observation plan against a snapshot, recompiling if
    /// the generation has changed.
    ///
    /// This is the primary convenience method. It calls
    /// [`get_or_compile`](Self::get_or_compile) then
    /// [`ObsPlan::execute`].
    ///
    /// `engine_tick` is the current engine tick for computing
    /// [`ObsMetadata::age_ticks`]. Pass `None` in Lockstep mode
    /// (age is always 0).
    pub fn execute(
        &mut self,
        space: &dyn Space,
        snapshot: &dyn SnapshotAccess,
        engine_tick: Option<TickId>,
        output: &mut [f32],
        mask: &mut [u8],
    ) -> Result<ObsMetadata, ObsError> {
        let generation = snapshot.world_generation_id();
        let plan = self.get_or_compile(space, generation)?;
        plan.execute(snapshot, engine_tick, output, mask)
    }

    /// Output length of the currently cached plan, or `None` if no
    /// plan has been compiled yet.
    pub fn output_len(&self) -> Option<usize> {
        self.cached.as_ref().map(|c| c.output_len)
    }

    /// Mask length of the currently cached plan.
    pub fn mask_len(&self) -> Option<usize> {
        self.cached.as_ref().map(|c| c.mask_len)
    }

    /// Entry shapes of the currently cached plan.
    pub fn entry_shapes(&self) -> Option<&[Vec<usize>]> {
        self.cached.as_ref().map(|c| c.entry_shapes.as_slice())
    }

    /// Whether a compiled plan is currently cached.
    pub fn is_compiled(&self) -> bool {
        self.cached.is_some()
    }

    /// The generation of the currently cached plan, if any.
    pub fn cached_generation(&self) -> Option<WorldGenerationId> {
        self.cached.as_ref().map(|c| c.generation)
    }

    /// Invalidate the cached plan, forcing recompilation on next use.
    pub fn invalidate(&mut self) {
        self.cached = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{ObsDtype, ObsEntry, ObsTransform};
    use murk_core::{FieldId, ParameterVersion, TickId, WorldGenerationId};
    use murk_space::{EdgeBehavior, RegionSpec, Square4};
    use murk_test_utils::MockSnapshot;

    fn space() -> Square4 {
        Square4::new(3, 3, EdgeBehavior::Absorb).unwrap()
    }

    fn spec() -> ObsSpec {
        ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: RegionSpec::All,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        }
    }

    fn snap(gen: u64, tick: u64) -> MockSnapshot {
        let mut s = MockSnapshot::new(
            TickId(tick),
            WorldGenerationId(gen),
            ParameterVersion(0),
        );
        s.set_field(FieldId(0), vec![1.0; 9]);
        s
    }

    // ── Cache lifecycle tests ────────────────────────────────

    #[test]
    fn not_compiled_initially() {
        let cache = ObsPlanCache::new(spec());
        assert!(!cache.is_compiled());
        assert_eq!(cache.output_len(), None);
        assert_eq!(cache.cached_generation(), None);
    }

    #[test]
    fn first_execute_compiles_plan() {
        let space = space();
        let snapshot = snap(1, 10);
        let mut cache = ObsPlanCache::new(spec());

        let mut output = vec![0.0f32; 9];
        let mut mask = vec![0u8; 9];
        cache.execute(&space, &snapshot, None, &mut output, &mut mask).unwrap();

        assert!(cache.is_compiled());
        assert_eq!(cache.cached_generation(), Some(WorldGenerationId(1)));
        assert_eq!(cache.output_len(), Some(9));
    }

    #[test]
    fn same_generation_reuses_plan() {
        let space = space();
        let snap1 = snap(1, 10);
        let snap2 = snap(1, 11);
        let mut cache = ObsPlanCache::new(spec());

        let mut output = vec![0.0f32; 9];
        let mut mask = vec![0u8; 9];
        cache.execute(&space, &snap1, None, &mut output, &mut mask).unwrap();
        assert_eq!(cache.cached_generation(), Some(WorldGenerationId(1)));

        // Same generation — no recompile.
        cache.execute(&space, &snap2, None, &mut output, &mut mask).unwrap();
        assert_eq!(cache.cached_generation(), Some(WorldGenerationId(1)));
    }

    #[test]
    fn generation_change_triggers_recompile() {
        let space = space();
        let snap_gen1 = snap(1, 10);
        let snap_gen2 = snap(2, 20);
        let mut cache = ObsPlanCache::new(spec());

        let mut output = vec![0.0f32; 9];
        let mut mask = vec![0u8; 9];
        cache.execute(&space, &snap_gen1, None, &mut output, &mut mask).unwrap();
        assert_eq!(cache.cached_generation(), Some(WorldGenerationId(1)));

        // Different generation → recompile.
        cache.execute(&space, &snap_gen2, None, &mut output, &mut mask).unwrap();
        assert_eq!(cache.cached_generation(), Some(WorldGenerationId(2)));
    }

    #[test]
    fn invalidate_forces_recompile() {
        let space = space();
        let snapshot = snap(1, 10);
        let mut cache = ObsPlanCache::new(spec());

        let mut output = vec![0.0f32; 9];
        let mut mask = vec![0u8; 9];
        cache.execute(&space, &snapshot, None, &mut output, &mut mask).unwrap();
        assert!(cache.is_compiled());

        cache.invalidate();
        assert!(!cache.is_compiled());
        assert_eq!(cache.cached_generation(), None);

        // Re-executes fine.
        cache.execute(&space, &snapshot, None, &mut output, &mut mask).unwrap();
        assert!(cache.is_compiled());
    }

    // ── age_ticks tests ──────────────────────────────────────

    #[test]
    fn age_ticks_zero_when_engine_tick_none() {
        let space = space();
        let snapshot = snap(1, 42);
        let mut cache = ObsPlanCache::new(spec());

        let mut output = vec![0.0f32; 9];
        let mut mask = vec![0u8; 9];
        let meta = cache.execute(&space, &snapshot, None, &mut output, &mut mask).unwrap();

        assert_eq!(meta.age_ticks, 0);
    }

    #[test]
    fn age_ticks_zero_for_lockstep_same_tick() {
        let space = space();
        let snapshot = snap(1, 10);
        let mut cache = ObsPlanCache::new(spec());

        let mut output = vec![0.0f32; 9];
        let mut mask = vec![0u8; 9];
        let meta = cache
            .execute(&space, &snapshot, Some(TickId(10)), &mut output, &mut mask)
            .unwrap();

        assert_eq!(meta.age_ticks, 0);
    }

    #[test]
    fn age_ticks_positive_for_stale_snapshot() {
        let space = space();
        // Snapshot at tick 10, engine at tick 15 → age = 5.
        let snapshot = snap(1, 10);
        let mut cache = ObsPlanCache::new(spec());

        let mut output = vec![0.0f32; 9];
        let mut mask = vec![0u8; 9];
        let meta = cache
            .execute(&space, &snapshot, Some(TickId(15)), &mut output, &mut mask)
            .unwrap();

        assert_eq!(meta.age_ticks, 5);
    }

    #[test]
    fn age_ticks_saturates_on_underflow() {
        let space = space();
        // Engine tick < snapshot tick (shouldn't happen, but saturating_sub handles it).
        let snapshot = snap(1, 100);
        let mut cache = ObsPlanCache::new(spec());

        let mut output = vec![0.0f32; 9];
        let mut mask = vec![0u8; 9];
        let meta = cache
            .execute(&space, &snapshot, Some(TickId(5)), &mut output, &mut mask)
            .unwrap();

        assert_eq!(meta.age_ticks, 0);
    }

    // ── get_or_compile tests ─────────────────────────────────

    #[test]
    fn get_or_compile_returns_bound_plan() {
        let space = space();
        let mut cache = ObsPlanCache::new(spec());

        let plan = cache.get_or_compile(&space, WorldGenerationId(42)).unwrap();
        assert_eq!(plan.compiled_generation(), Some(WorldGenerationId(42)));
    }

    #[test]
    fn get_or_compile_recompiles_on_new_generation() {
        let space = space();
        let mut cache = ObsPlanCache::new(spec());

        cache.get_or_compile(&space, WorldGenerationId(1)).unwrap();
        assert_eq!(cache.cached_generation(), Some(WorldGenerationId(1)));

        cache.get_or_compile(&space, WorldGenerationId(2)).unwrap();
        assert_eq!(cache.cached_generation(), Some(WorldGenerationId(2)));
    }
}
