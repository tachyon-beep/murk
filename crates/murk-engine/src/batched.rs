//! Batched simulation engine for vectorized RL training.
//!
//! [`BatchedEngine`] owns N [`LockstepWorld`]s and steps them all in a
//! single call, eliminating per-world FFI overhead. Observation extraction
//! uses [`ObsPlan::execute_batch()`] to fill a contiguous output buffer
//! across all worlds.
//!
//! # Design
//!
//! The hot path is `step_and_observe`: step all worlds sequentially, then
//! extract observations in batch. The GIL is released once at the Python
//! layer, covering the entire operation. This reduces 2N GIL cycles (the
//! current `MurkVecEnv` approach) to exactly 1.
//!
//! Parallelism (rayon) is deferred to v2. The GIL elimination alone is
//! the dominant win; adding `par_iter_mut` later is a 3-line change.

use murk_core::command::Command;
use murk_core::error::ObsError;
use murk_core::id::TickId;
use murk_core::traits::SnapshotAccess;
use murk_obs::metadata::ObsMetadata;
use murk_obs::plan::ObsPlan;
use murk_obs::spec::ObsSpec;

use crate::config::{ConfigError, WorldConfig};
use crate::lockstep::LockstepWorld;
use crate::metrics::StepMetrics;
use crate::tick::TickError;

// ── Error type ──────────────────────────────────────────────────

/// Error from a batched operation, annotated with the failing world index.
#[derive(Debug, PartialEq)]
pub enum BatchError {
    /// A world's `step_sync()` failed.
    Step {
        /// Index of the world that failed (0-based).
        world_index: usize,
        /// The underlying tick error.
        error: TickError,
    },
    /// Observation extraction failed.
    Observe(ObsError),
    /// Configuration error during construction or reset.
    Config(ConfigError),
    /// World index out of bounds.
    InvalidIndex {
        /// The requested index.
        world_index: usize,
        /// Total number of worlds.
        num_worlds: usize,
    },
    /// No observation plan was compiled (called observe without obs_spec).
    NoObsPlan,
    /// Batch-level argument validation failed.
    InvalidArgument {
        /// Human-readable description of what's wrong.
        reason: String,
    },
}

impl std::fmt::Display for BatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BatchError::Step { world_index, error } => {
                write!(f, "world {world_index}: step failed: {error:?}")
            }
            BatchError::Observe(e) => write!(f, "observe failed: {e:?}"),
            BatchError::Config(e) => write!(f, "config error: {e:?}"),
            BatchError::InvalidIndex {
                world_index,
                num_worlds,
            } => write!(
                f,
                "world index {world_index} out of range (num_worlds={num_worlds})"
            ),
            BatchError::NoObsPlan => write!(f, "no observation plan compiled"),
            BatchError::InvalidArgument { reason } => {
                write!(f, "invalid argument: {reason}")
            }
        }
    }
}

impl std::error::Error for BatchError {}

// ── Result type ─────────────────────────────────────────────────

/// Result of stepping a batch of worlds.
pub struct BatchResult {
    /// Per-world tick IDs after stepping.
    pub tick_ids: Vec<TickId>,
    /// Per-world step metrics.
    pub metrics: Vec<StepMetrics>,
}

// ── BatchedEngine ───────────────────────────────────────────────

/// Batched simulation engine owning N lockstep worlds.
///
/// Created from N [`WorldConfig`]s with an optional [`ObsSpec`].
/// All worlds must share the same space topology (validated at
/// construction).
///
/// The primary interface is [`step_and_observe()`](Self::step_and_observe):
/// step all worlds, then extract observations into a contiguous buffer
/// using [`ObsPlan::execute_batch()`].
pub struct BatchedEngine {
    worlds: Vec<LockstepWorld>,
    obs_plan: Option<ObsPlan>,
    obs_output_len: usize,
    obs_mask_len: usize,
}

impl BatchedEngine {
    /// Create a batched engine from N world configs.
    ///
    /// If `obs_spec` is provided, compiles an [`ObsPlan`] from the first
    /// world's space. All worlds must have the same `cell_count`
    /// (defensive check).
    ///
    /// # Errors
    ///
    /// Returns [`BatchError::Config`] if any world fails to construct,
    /// or [`BatchError::Observe`] if the obs plan fails to compile.
    pub fn new(configs: Vec<WorldConfig>, obs_spec: Option<&ObsSpec>) -> Result<Self, BatchError> {
        if configs.is_empty() {
            return Err(BatchError::InvalidArgument {
                reason: "BatchedEngine requires at least one world config".into(),
            });
        }

        let mut worlds = Vec::with_capacity(configs.len());
        for config in configs {
            let world = LockstepWorld::new(config).map_err(BatchError::Config)?;
            worlds.push(world);
        }

        // Validate all worlds share the same space topology.
        // topology_eq checks TypeId, dimensions, and behavioral parameters
        // (e.g. EdgeBehavior) so that spaces like Line1D(10, Absorb) and
        // Line1D(10, Wrap) are correctly rejected.
        let ref_space = worlds[0].space();
        for (i, world) in worlds.iter().enumerate().skip(1) {
            if !ref_space.topology_eq(world.space()) {
                return Err(BatchError::InvalidArgument {
                    reason: format!(
                        "world 0 and world {i} have incompatible space topologies; \
                         all worlds in a batch must use the same topology"
                    ),
                });
            }
        }

        // Compile obs plan if spec provided.
        let (obs_plan, obs_output_len, obs_mask_len) = match obs_spec {
            Some(spec) => {
                let result =
                    ObsPlan::compile(spec, worlds[0].space()).map_err(BatchError::Observe)?;

                // Validate all worlds have matching field schemas for observed fields.
                // ObsPlan::compile only takes a Space (not a snapshot), so field
                // existence isn't checked until execute(). Catching mismatches here
                // prevents late observation failures after worlds have been stepped.
                let ref_snap = worlds[0].snapshot();
                for entry in &spec.entries {
                    let fid = entry.field_id;
                    let ref_len = ref_snap.read_field(fid).map(|d| d.len());
                    for (i, world) in worlds.iter().enumerate().skip(1) {
                        let snap = world.snapshot();
                        let other_len = snap.read_field(fid).map(|d| d.len());
                        if other_len != ref_len {
                            return Err(BatchError::InvalidArgument {
                                reason: format!(
                                    "world {i} field {fid:?}: {} elements, \
                                     world 0 has {} elements; \
                                     all worlds must share the same field schema",
                                    other_len
                                        .map(|n| n.to_string())
                                        .unwrap_or_else(|| "missing".into()),
                                    ref_len
                                        .map(|n| n.to_string())
                                        .unwrap_or_else(|| "missing".into()),
                                ),
                            });
                        }
                    }
                }

                (Some(result.plan), result.output_len, result.mask_len)
            }
            None => (None, 0, 0),
        };

        Ok(BatchedEngine {
            worlds,
            obs_plan,
            obs_output_len,
            obs_mask_len,
        })
    }

    /// Step all worlds and extract observations in one call.
    ///
    /// `commands` must have exactly `num_worlds()` entries.
    /// `output` must have at least `num_worlds() * obs_output_len()` elements.
    /// `mask` must have at least `num_worlds() * obs_mask_len()` bytes.
    ///
    /// Returns per-world tick IDs and metrics.
    pub fn step_and_observe(
        &mut self,
        commands: &[Vec<Command>],
        output: &mut [f32],
        mask: &mut [u8],
    ) -> Result<BatchResult, BatchError> {
        // Pre-flight: validate observation preconditions before mutating
        // world state. Without this, a late observe failure (no obs plan,
        // buffer too small) would leave worlds stepped but observations
        // unextracted — making the error non-atomic.
        self.validate_observe_buffers(output, mask)?;

        let result = self.step_all(commands)?;

        // Observe phase: borrow worlds immutably for snapshot collection.
        self.observe_all_inner(output, mask)?;

        Ok(result)
    }

    /// Step all worlds without observation extraction.
    pub fn step_all(&mut self, commands: &[Vec<Command>]) -> Result<BatchResult, BatchError> {
        let n = self.worlds.len();
        if commands.len() != n {
            return Err(BatchError::InvalidArgument {
                reason: format!("commands has {} entries, expected {n}", commands.len()),
            });
        }

        let mut tick_ids = Vec::with_capacity(n);
        let mut metrics = Vec::with_capacity(n);

        for (idx, world) in self.worlds.iter_mut().enumerate() {
            let result = world
                .step_sync(commands[idx].clone())
                .map_err(|e| BatchError::Step {
                    world_index: idx,
                    error: e,
                })?;
            tick_ids.push(result.snapshot.tick_id());
            metrics.push(result.metrics);
        }

        Ok(BatchResult { tick_ids, metrics })
    }

    /// Extract observations from all worlds without stepping.
    ///
    /// Used after `reset_all()` to get initial observations.
    pub fn observe_all(
        &self,
        output: &mut [f32],
        mask: &mut [u8],
    ) -> Result<Vec<ObsMetadata>, BatchError> {
        self.observe_all_inner(output, mask)
    }

    /// Internal observation extraction shared by step_and_observe and observe_all.
    fn observe_all_inner(
        &self,
        output: &mut [f32],
        mask: &mut [u8],
    ) -> Result<Vec<ObsMetadata>, BatchError> {
        let plan = self.obs_plan.as_ref().ok_or(BatchError::NoObsPlan)?;

        let snapshots: Vec<_> = self.worlds.iter().map(|w| w.snapshot()).collect();
        let snap_refs: Vec<&dyn SnapshotAccess> =
            snapshots.iter().map(|s| s as &dyn SnapshotAccess).collect();

        plan.execute_batch(&snap_refs, None, output, mask)
            .map_err(BatchError::Observe)
    }

    /// Validate that observation preconditions are met (plan exists, buffers
    /// large enough) without performing any mutation. Called by
    /// `step_and_observe` before `step_all` to guarantee atomicity.
    fn validate_observe_buffers(&self, output: &[f32], mask: &[u8]) -> Result<(), BatchError> {
        let plan = self.obs_plan.as_ref().ok_or(BatchError::NoObsPlan)?;
        if plan.is_standard() {
            return Err(BatchError::InvalidArgument {
                reason: "obs spec uses agent-relative regions (AgentDisk/AgentRect), \
                         which are unsupported in batched step_and_observe"
                    .into(),
            });
        }
        let n = self.worlds.len();
        let expected_out = n * self.obs_output_len;
        let expected_mask = n * self.obs_mask_len;
        if output.len() < expected_out {
            return Err(BatchError::InvalidArgument {
                reason: format!("output buffer too small: {} < {expected_out}", output.len()),
            });
        }
        if mask.len() < expected_mask {
            return Err(BatchError::InvalidArgument {
                reason: format!("mask buffer too small: {} < {expected_mask}", mask.len()),
            });
        }
        Ok(())
    }

    /// Reset a single world by index.
    pub fn reset_world(&mut self, idx: usize, seed: u64) -> Result<(), BatchError> {
        let n = self.worlds.len();
        let world = self.worlds.get_mut(idx).ok_or(BatchError::InvalidIndex {
            world_index: idx,
            num_worlds: n,
        })?;
        world.reset(seed).map_err(BatchError::Config)?;
        Ok(())
    }

    /// Reset all worlds with per-world seeds.
    pub fn reset_all(&mut self, seeds: &[u64]) -> Result<(), BatchError> {
        let n = self.worlds.len();
        if seeds.len() != n {
            return Err(BatchError::InvalidArgument {
                reason: format!("seeds has {} entries, expected {n}", seeds.len()),
            });
        }
        for (idx, world) in self.worlds.iter_mut().enumerate() {
            world.reset(seeds[idx]).map_err(BatchError::Config)?;
        }
        Ok(())
    }

    /// Number of worlds in the batch.
    pub fn num_worlds(&self) -> usize {
        self.worlds.len()
    }

    /// Per-world observation output length (f32 elements).
    pub fn obs_output_len(&self) -> usize {
        self.obs_output_len
    }

    /// Per-world observation mask length (bytes).
    pub fn obs_mask_len(&self) -> usize {
        self.obs_mask_len
    }

    /// Current tick ID of a specific world.
    pub fn world_tick(&self, idx: usize) -> Option<TickId> {
        self.worlds.get(idx).map(|w| w.current_tick())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::id::FieldId;
    use murk_core::traits::FieldReader;
    use murk_obs::spec::{ObsDtype, ObsEntry, ObsRegion, ObsTransform};
    use murk_space::{EdgeBehavior, Line1D, RegionSpec, Square4};
    use murk_test_utils::ConstPropagator;

    use crate::config::BackoffConfig;

    fn scalar_field(name: &str) -> murk_core::FieldDef {
        murk_core::FieldDef {
            name: name.to_string(),
            field_type: murk_core::FieldType::Scalar,
            mutability: murk_core::FieldMutability::PerTick,
            units: None,
            bounds: None,
            boundary_behavior: murk_core::BoundaryBehavior::Clamp,
        }
    }

    fn make_config(seed: u64, value: f32) -> WorldConfig {
        WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("energy")],
            propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), value))],
            dt: 0.1,
            seed,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: BackoffConfig::default(),
        }
    }

    fn make_grid_config(seed: u64, value: f32) -> WorldConfig {
        WorldConfig {
            space: Box::new(Square4::new(4, 4, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("energy")],
            propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), value))],
            dt: 0.1,
            seed,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: BackoffConfig::default(),
        }
    }

    fn obs_spec_all_field0() -> ObsSpec {
        ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        }
    }

    // ── Construction tests ────────────────────────────────────

    #[test]
    fn new_single_world() {
        let configs = vec![make_config(42, 1.0)];
        let engine = BatchedEngine::new(configs, None).unwrap();
        assert_eq!(engine.num_worlds(), 1);
        assert_eq!(engine.obs_output_len(), 0);
        assert_eq!(engine.obs_mask_len(), 0);
    }

    #[test]
    fn new_four_worlds() {
        let configs: Vec<_> = (0..4).map(|i| make_config(i, 1.0)).collect();
        let engine = BatchedEngine::new(configs, None).unwrap();
        assert_eq!(engine.num_worlds(), 4);
    }

    #[test]
    fn new_zero_worlds_is_error() {
        let result = BatchedEngine::new(vec![], None);
        assert!(result.is_err());
    }

    #[test]
    fn new_with_obs_spec() {
        let configs = vec![make_config(42, 1.0)];
        let spec = obs_spec_all_field0();
        let engine = BatchedEngine::new(configs, Some(&spec)).unwrap();
        assert_eq!(engine.obs_output_len(), 10); // Line1D(10) → 10 cells
        assert_eq!(engine.obs_mask_len(), 10);
    }

    // ── Determinism test ──────────────────────────────────────

    #[test]
    fn batch_matches_independent_worlds() {
        let spec = obs_spec_all_field0();

        // Batched: 2 worlds
        let configs = vec![make_config(42, 42.0), make_config(99, 42.0)];
        let mut batched = BatchedEngine::new(configs, Some(&spec)).unwrap();
        let n = batched.num_worlds();
        let out_len = n * batched.obs_output_len();
        let mask_len = n * batched.obs_mask_len();
        let mut batch_output = vec![0.0f32; out_len];
        let mut batch_mask = vec![0u8; mask_len];

        let commands = vec![vec![], vec![]];
        batched
            .step_and_observe(&commands, &mut batch_output, &mut batch_mask)
            .unwrap();

        // Independent: 2 separate worlds
        let mut w0 = LockstepWorld::new(make_config(42, 42.0)).unwrap();
        let mut w1 = LockstepWorld::new(make_config(99, 42.0)).unwrap();
        let r0 = w0.step_sync(vec![]).unwrap();
        let r1 = w1.step_sync(vec![]).unwrap();

        let d0 = r0.snapshot.read(FieldId(0)).unwrap();
        let d1 = r1.snapshot.read(FieldId(0)).unwrap();

        // Batch output should be [world0_obs | world1_obs]
        assert_eq!(&batch_output[..10], d0);
        assert_eq!(&batch_output[10..20], d1);
    }

    // ── Observation correctness ───────────────────────────────

    #[test]
    fn observation_filled_with_const_value() {
        let spec = obs_spec_all_field0();
        let configs = vec![
            make_config(1, 42.0),
            make_config(2, 42.0),
            make_config(3, 42.0),
        ];
        let mut engine = BatchedEngine::new(configs, Some(&spec)).unwrap();

        let commands = vec![vec![], vec![], vec![]];
        let n = engine.num_worlds();
        let mut output = vec![0.0f32; n * engine.obs_output_len()];
        let mut mask = vec![0u8; n * engine.obs_mask_len()];
        engine
            .step_and_observe(&commands, &mut output, &mut mask)
            .unwrap();

        // All cells should be 42.0 for all worlds.
        assert!(output.iter().all(|&v| v == 42.0));
        assert!(mask.iter().all(|&m| m == 1));
    }

    // ── Reset tests ───────────────────────────────────────────

    #[test]
    fn reset_single_world_preserves_others() {
        let configs: Vec<_> = (0..4).map(|i| make_config(i, 1.0)).collect();
        let mut engine = BatchedEngine::new(configs, None).unwrap();

        // Step all once.
        let commands = vec![vec![]; 4];
        engine.step_all(&commands).unwrap();
        assert_eq!(engine.world_tick(0), Some(TickId(1)));
        assert_eq!(engine.world_tick(3), Some(TickId(1)));

        // Reset only world 0.
        engine.reset_world(0, 999).unwrap();
        assert_eq!(engine.world_tick(0), Some(TickId(0)));
        assert_eq!(engine.world_tick(1), Some(TickId(1)));
        assert_eq!(engine.world_tick(2), Some(TickId(1)));
        assert_eq!(engine.world_tick(3), Some(TickId(1)));
    }

    #[test]
    fn reset_all_resets_to_tick_zero() {
        let configs: Vec<_> = (0..3).map(|i| make_config(i, 1.0)).collect();
        let mut engine = BatchedEngine::new(configs, None).unwrap();

        // Step all twice.
        let commands = vec![vec![]; 3];
        engine.step_all(&commands).unwrap();
        engine.step_all(&commands).unwrap();

        engine.reset_all(&[10, 20, 30]).unwrap();
        for i in 0..3 {
            assert_eq!(engine.world_tick(i), Some(TickId(0)));
        }
    }

    // ── Error isolation ───────────────────────────────────────

    #[test]
    fn invalid_world_index_returns_error() {
        let configs = vec![make_config(0, 1.0)];
        let mut engine = BatchedEngine::new(configs, None).unwrap();

        let result = engine.reset_world(5, 0);
        assert!(matches!(result, Err(BatchError::InvalidIndex { .. })));
    }

    #[test]
    fn wrong_command_count_returns_error() {
        let configs = vec![make_config(0, 1.0), make_config(1, 1.0)];
        let mut engine = BatchedEngine::new(configs, None).unwrap();

        let result = engine.step_all(&[vec![]]); // 1 entry for 2 worlds
        assert!(result.is_err());
    }

    #[test]
    fn observe_without_plan_returns_error() {
        let configs = vec![make_config(0, 1.0)];
        let engine = BatchedEngine::new(configs, None).unwrap();

        let mut output = vec![0.0f32; 10];
        let mut mask = vec![0u8; 10];
        let result = engine.observe_all(&mut output, &mut mask);
        assert!(matches!(result, Err(BatchError::NoObsPlan)));
    }

    // ── Observe after reset ───────────────────────────────────

    #[test]
    fn observe_all_after_reset() {
        let spec = obs_spec_all_field0();
        let configs = vec![make_config(1, 42.0), make_config(2, 42.0)];
        let mut engine = BatchedEngine::new(configs, Some(&spec)).unwrap();

        // Step once to populate data.
        let commands = vec![vec![], vec![]];
        let n = engine.num_worlds();
        let mut output = vec![0.0f32; n * engine.obs_output_len()];
        let mut mask = vec![0u8; n * engine.obs_mask_len()];
        engine
            .step_and_observe(&commands, &mut output, &mut mask)
            .unwrap();

        // Reset all and observe (initial state is zeroed).
        engine.reset_all(&[10, 20]).unwrap();
        let meta = engine.observe_all(&mut output, &mut mask).unwrap();
        assert_eq!(meta.len(), 2);
        assert_eq!(meta[0].tick_id, TickId(0));
        assert_eq!(meta[1].tick_id, TickId(0));
    }

    // ── Topology validation ──────────────────────────────────

    #[test]
    fn mixed_space_types_rejected() {
        use murk_space::Ring1D;

        // Line1D(10) and Ring1D(10): same ndim, same cell_count, different type.
        let line_config = WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("energy")],
            propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))],
            dt: 0.1,
            seed: 1,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: BackoffConfig::default(),
        };
        let ring_config = WorldConfig {
            space: Box::new(Ring1D::new(10).unwrap()),
            fields: vec![scalar_field("energy")],
            propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))],
            dt: 0.1,
            seed: 2,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: BackoffConfig::default(),
        };

        let result = BatchedEngine::new(vec![line_config, ring_config], None);
        match result {
            Err(e) => {
                let msg = format!("{e}");
                assert!(msg.contains("incompatible space topologies"), "got: {msg}");
            }
            Ok(_) => panic!("expected error for mixed space types"),
        }
    }

    #[test]
    fn mixed_edge_behaviors_rejected() {
        // Line1D(10, Absorb) and Line1D(10, Wrap): same TypeId, ndim, cell_count,
        // but different edge behavior — must be rejected.
        let absorb_config = WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("energy")],
            propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))],
            dt: 0.1,
            seed: 1,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: BackoffConfig::default(),
        };
        let wrap_config = WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Wrap).unwrap()),
            fields: vec![scalar_field("energy")],
            propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))],
            dt: 0.1,
            seed: 2,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: BackoffConfig::default(),
        };

        let result = BatchedEngine::new(vec![absorb_config, wrap_config], None);
        assert!(result.is_err(), "expected error for mixed edge behaviors");
    }

    // ── Atomic step_and_observe ──────────────────────────────

    #[test]
    fn step_and_observe_no_plan_does_not_step() {
        // Without an obs plan, step_and_observe should fail *before*
        // advancing any world state.
        let configs = vec![make_config(0, 1.0), make_config(1, 1.0)];
        let mut engine = BatchedEngine::new(configs, None).unwrap();

        let commands = vec![vec![], vec![]];
        let mut output = vec![0.0f32; 20];
        let mut mask = vec![0u8; 20];
        let result = engine.step_and_observe(&commands, &mut output, &mut mask);
        assert!(matches!(result, Err(BatchError::NoObsPlan)));

        // Worlds must still be at tick 0 — no mutation occurred.
        assert_eq!(engine.world_tick(0), Some(TickId(0)));
        assert_eq!(engine.world_tick(1), Some(TickId(0)));
    }

    #[test]
    fn step_and_observe_small_buffer_does_not_step() {
        // Buffer too small should fail before advancing world state.
        let spec = obs_spec_all_field0();
        let configs = vec![make_config(0, 1.0), make_config(1, 1.0)];
        let mut engine = BatchedEngine::new(configs, Some(&spec)).unwrap();

        let commands = vec![vec![], vec![]];
        let mut output = vec![0.0f32; 5]; // need 20, only 5
        let mut mask = vec![0u8; 20];
        let result = engine.step_and_observe(&commands, &mut output, &mut mask);
        assert!(result.is_err());

        // Worlds must still be at tick 0.
        assert_eq!(engine.world_tick(0), Some(TickId(0)));
        assert_eq!(engine.world_tick(1), Some(TickId(0)));
    }

    #[test]
    fn step_and_observe_agent_relative_plan_does_not_step() {
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentRect {
                    half_extent: smallvec::smallvec![1, 1],
                },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let configs = vec![make_grid_config(0, 1.0), make_grid_config(1, 1.0)];
        let mut engine = BatchedEngine::new(configs, Some(&spec)).unwrap();
        let n = engine.num_worlds();
        let mut output = vec![0.0f32; n * engine.obs_output_len()];
        let mut mask = vec![0u8; n * engine.obs_mask_len()];

        let result = engine.step_and_observe(&[vec![], vec![]], &mut output, &mut mask);
        match result {
            Err(BatchError::InvalidArgument { reason }) => {
                assert!(
                    reason.contains("AgentDisk/AgentRect"),
                    "unexpected reason: {reason}"
                );
            }
            _ => panic!("expected InvalidArgument for agent-relative plan"),
        }

        assert_eq!(engine.world_tick(0), Some(TickId(0)));
        assert_eq!(engine.world_tick(1), Some(TickId(0)));
    }

    // ── Field schema validation ─────────────────────────────

    #[test]
    fn mismatched_field_schemas_rejected() {
        // World 0 has 2 fields, world 1 has only 1. Obs spec references
        // FieldId(1) which is missing in world 1. Construction must fail.
        let spec = ObsSpec {
            entries: vec![
                ObsEntry {
                    field_id: FieldId(0),
                    region: ObsRegion::Fixed(RegionSpec::All),
                    pool: None,
                    transform: ObsTransform::Identity,
                    dtype: ObsDtype::F32,
                },
                ObsEntry {
                    field_id: FieldId(1),
                    region: ObsRegion::Fixed(RegionSpec::All),
                    pool: None,
                    transform: ObsTransform::Identity,
                    dtype: ObsDtype::F32,
                },
            ],
        };

        // World 0: has 2 fields (FieldId(0) and FieldId(1))
        let config_two_fields = WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("energy"), scalar_field("temp")],
            propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))],
            dt: 0.1,
            seed: 1,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: BackoffConfig::default(),
        };

        // World 1: has only 1 field (FieldId(0)), missing FieldId(1)
        let config_one_field = WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("energy")],
            propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))],
            dt: 0.1,
            seed: 2,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: BackoffConfig::default(),
        };

        let result = BatchedEngine::new(vec![config_two_fields, config_one_field], Some(&spec));
        match result {
            Err(e) => {
                let msg = format!("{e}");
                assert!(
                    msg.contains("field") && msg.contains("missing"),
                    "error should mention missing field, got: {msg}"
                );
            }
            Ok(_) => panic!("expected error for mismatched field schemas"),
        }
    }
}
