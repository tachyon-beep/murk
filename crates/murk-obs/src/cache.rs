//! Plan cache with space-topology-based invalidation.
//!
//! [`ObsPlanCache`] wraps an [`ObsSpec`] and lazily compiles an
//! [`ObsPlan`] on first use. Subsequent calls to [`ObsPlanCache::get_or_compile`]
//! return the cached plan as long as the same space instance (by
//! [`SpaceInstanceId`] and cell count)
//! is provided; otherwise the plan is recompiled automatically.
//!
//! The cache does **not** key on [`WorldGenerationId`](murk_core::WorldGenerationId)
//! because that counter increments on every tick, which would defeat
//! caching. Observation plans depend only on space topology (cell count,
//! canonical ordering), not on per-tick state.

use murk_core::error::ObsError;
use murk_core::{SnapshotAccess, SpaceInstanceId, TickId};
use murk_space::Space;

use crate::metadata::ObsMetadata;
use crate::spec::ObsSpec;
use crate::ObsPlan;

/// Cached observation plan with space-topology-based invalidation.
///
/// Holds an [`ObsSpec`] and an optional compiled [`ObsPlan`]. On each
/// call to [`execute`](Self::execute), checks whether the cached plan
/// was compiled for the same space (by [`SpaceInstanceId`] and cell count).
/// On mismatch, the plan is recompiled transparently.
///
/// # Example
///
/// ```ignore
/// let mut cache = ObsPlanCache::new(spec);
/// // First call compiles the plan:
/// let meta = cache.execute(&space, &snapshot, None, &mut output, &mut mask)?;
/// // Subsequent calls reuse it (same space):
/// let meta = cache.execute(&space, &snapshot, None, &mut output, &mut mask)?;
/// ```
///
/// # Invalidation
///
/// The plan is recompiled when:
/// - No plan has been compiled yet.
/// - A different space instance is passed (different [`SpaceInstanceId`]).
/// - The same space object's `cell_count()` has changed (topology mutation).
/// - [`invalidate`](Self::invalidate) is called explicitly.
///
/// The plan is **not** recompiled when:
/// - The snapshot's `WorldGenerationId` changes (that is per-tick churn,
///   not a topology change).
#[derive(Debug)]
pub struct ObsPlanCache {
    spec: ObsSpec,
    cached: Option<CachedPlan>,
}

/// Fingerprint of a `&dyn Space` for cache invalidation.
///
/// Uses the space's [`SpaceInstanceId`] (monotonic counter, no ABA risk)
/// plus `cell_count` as a mutation guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SpaceFingerprint {
    instance_id: SpaceInstanceId,
    cell_count: usize,
}

impl SpaceFingerprint {
    fn of(space: &dyn Space) -> Self {
        Self {
            instance_id: space.instance_id(),
            cell_count: space.cell_count(),
        }
    }
}

/// Internal: a compiled plan with its space fingerprint and layout info.
#[derive(Debug)]
struct CachedPlan {
    plan: ObsPlan,
    fingerprint: SpaceFingerprint,
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
    /// Returns the cached plan if one exists and was compiled for the
    /// same space (by [`SpaceInstanceId`] and cell count). Otherwise
    /// recompiles from the stored [`ObsSpec`].
    pub fn get_or_compile(
        &mut self,
        space: &dyn Space,
    ) -> Result<&ObsPlan, ObsError> {
        let fingerprint = SpaceFingerprint::of(space);

        let needs_recompile = match &self.cached {
            None => true,
            Some(cached) => cached.fingerprint != fingerprint,
        };

        if needs_recompile {
            let result = ObsPlan::compile(&self.spec, space)?;
            self.cached = Some(CachedPlan {
                plan: result.plan,
                fingerprint,
                output_len: result.output_len,
                mask_len: result.mask_len,
                entry_shapes: result.entry_shapes,
            });
        }

        Ok(&self.cached.as_ref().unwrap().plan)
    }

    /// Execute the observation plan against a snapshot, recompiling if
    /// the space has changed.
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
        let plan = self.get_or_compile(space)?;
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
        assert_eq!(cache.output_len(), Some(9));
    }

    #[test]
    fn same_space_reuses_plan_across_generations() {
        let space = space();
        // Different WorldGenerationId values — cache should NOT recompile.
        let snap_gen1 = snap(1, 10);
        let snap_gen2 = snap(2, 20);
        let snap_gen3 = snap(3, 30);
        let mut cache = ObsPlanCache::new(spec());

        let mut output = vec![0.0f32; 9];
        let mut mask = vec![0u8; 9];
        cache.execute(&space, &snap_gen1, None, &mut output, &mut mask).unwrap();
        assert!(cache.is_compiled());

        // Same space, different generation — no recompile.
        cache.execute(&space, &snap_gen2, None, &mut output, &mut mask).unwrap();
        assert!(cache.is_compiled());

        // Third generation — still no recompile.
        cache.execute(&space, &snap_gen3, None, &mut output, &mut mask).unwrap();
        assert!(cache.is_compiled());
    }

    #[test]
    fn different_space_triggers_recompile() {
        let space_a = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let space_b = Square4::new(4, 4, EdgeBehavior::Absorb).unwrap();
        let mut cache = ObsPlanCache::new(spec());

        // Compile with 3x3 space (9 cells).
        cache.get_or_compile(&space_a).unwrap();
        assert!(cache.is_compiled());
        assert_eq!(cache.output_len(), Some(9));

        // Different space object with different topology → recompile.
        cache.get_or_compile(&space_b).unwrap();
        assert!(cache.is_compiled());
        assert_eq!(cache.output_len(), Some(16));
    }

    #[test]
    fn different_space_same_dimensions_triggers_recompile() {
        // Two distinct space objects with the same dimensions.
        // Different instance IDs → recompile, even though topology is identical.
        let space_a = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let space_b = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let mut cache = ObsPlanCache::new(spec());

        let fp_a = SpaceFingerprint::of(&space_a);
        let fp_b = SpaceFingerprint::of(&space_b);
        // Distinct objects have different instance IDs (monotonic counter).
        assert_ne!(fp_a.instance_id, fp_b.instance_id);

        cache.get_or_compile(&space_a).unwrap();
        assert!(cache.is_compiled());

        // Different instance ID → recompile (conservative but safe).
        cache.get_or_compile(&space_b).unwrap();
        assert!(cache.is_compiled());
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
    fn get_or_compile_returns_unbound_plan() {
        let space = space();
        let mut cache = ObsPlanCache::new(spec());

        let plan = cache.get_or_compile(&space).unwrap();
        // Cache uses compile() not compile_bound(), so no generation binding.
        assert_eq!(plan.compiled_generation(), None);
    }

    #[test]
    fn get_or_compile_reuses_for_same_space() {
        let space = space();
        let mut cache = ObsPlanCache::new(spec());

        cache.get_or_compile(&space).unwrap();
        assert!(cache.is_compiled());

        // Same space reference → reuse.
        cache.get_or_compile(&space).unwrap();
        assert!(cache.is_compiled());
    }

    // ── SpaceFingerprint tests ───────────────────────────────

    #[test]
    fn fingerprint_same_object_is_equal() {
        let space = space();
        let fp1 = SpaceFingerprint::of(&space);
        let fp2 = SpaceFingerprint::of(&space);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn fingerprint_different_objects_differ() {
        // Monotonic counter guarantees distinct IDs even for identical topology.
        let a = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let b = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let fp_a = SpaceFingerprint::of(&a);
        let fp_b = SpaceFingerprint::of(&b);
        assert_ne!(fp_a, fp_b);
    }

    #[test]
    fn fingerprint_different_sizes_differ() {
        let small = Square4::new(2, 2, EdgeBehavior::Absorb).unwrap();
        let big = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let fp_s = SpaceFingerprint::of(&small);
        let fp_b = SpaceFingerprint::of(&big);
        assert_ne!(fp_s, fp_b);
    }

    #[test]
    fn fingerprint_clone_preserves_id() {
        // Cloning a space preserves instance_id (same topology, safe to reuse plan).
        let a = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let b = a.clone();
        let fp_a = SpaceFingerprint::of(&a);
        let fp_b = SpaceFingerprint::of(&b);
        assert_eq!(fp_a, fp_b);
    }
}
