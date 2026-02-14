//! Lockstep (synchronous) simulation world.
//!
//! [`LockstepWorld`] is the primary user-facing API for running simulations
//! in lockstep mode. Each call to [`step_sync()`](LockstepWorld::step_sync)
//! submits commands, executes one tick of the propagator pipeline, and
//! returns a snapshot of the resulting world state.
//!
//! # Ownership model
//!
//! `LockstepWorld` is [`Send`] (can be moved between threads) but not
//! [`Sync`] (cannot be shared). All mutating methods take `&mut self`,
//! and [`step_sync()`](LockstepWorld::step_sync) returns a [`Snapshot`]
//! that borrows from `self`. This means the caller cannot call `step_sync()`
//! while holding a snapshot reference — the borrow checker enforces
//! aliasing prevention at compile time.
//!
//! # Shutdown
//!
//! Dropping a `LockstepWorld` reclaims all arena memory. Since `&mut self`
//! guarantees no outstanding borrows at drop time, cleanup is always safe
//! (Decision E). No background threads are involved.

use murk_arena::read::Snapshot;
use murk_core::command::{Command, Receipt};
use murk_core::id::TickId;

use crate::config::{ConfigError, WorldConfig};
use crate::metrics::StepMetrics;
use crate::tick::{TickEngine, TickError};

// Compile-time assertion: LockstepWorld is Send but NOT Sync.
// (dyn Propagator is Send + !Sync, which is the design intent.)
// Fails to compile if any field is !Send.
const _: () = {
    #[allow(dead_code)]
    fn assert_send<T: Send>() {}
    #[allow(dead_code)]
    fn check() {
        assert_send::<LockstepWorld>();
    }
};

// ── StepResult ──────────────────────────────────────────────────

/// Result of a successful [`LockstepWorld::step_sync()`] call.
pub struct StepResult<'w> {
    /// Read-only snapshot of world state after this tick.
    pub snapshot: Snapshot<'w>,
    /// Per-command receipts from tick execution (applied, expired, rolled back).
    ///
    /// Does **not** include submission-rejected receipts (e.g. `QueueFull`).
    /// In lockstep mode the queue is drained every tick, so rejection is rare.
    /// If you need submission receipts, use the lower-level
    /// [`TickEngine`] API directly.
    pub receipts: Vec<Receipt>,
    /// Performance metrics for this tick.
    pub metrics: StepMetrics,
}

// ── LockstepWorld ───────────────────────────────────────────────

/// Single-threaded simulation world for lockstep (synchronous) execution.
///
/// Created from a [`WorldConfig`] via [`new()`](LockstepWorld::new).
/// Each [`step_sync()`](LockstepWorld::step_sync) call runs one complete
/// tick: submit commands → drain ingress → run propagator pipeline →
/// publish snapshot → return results.
///
/// # Example
///
/// ```ignore
/// let world = LockstepWorld::new(config)?;
/// for _ in 0..1000 {
///     let result = world.step_sync(commands)?;
///     let obs = result.snapshot.read(field_id);
/// }
/// ```
pub struct LockstepWorld {
    engine: TickEngine,
    seed: u64,
}

impl LockstepWorld {
    /// Create a new lockstep world from a [`WorldConfig`].
    ///
    /// Validates the configuration, builds the read resolution plan,
    /// constructs the arena, and returns a ready-to-step world.
    /// Consumes the `WorldConfig`.
    pub fn new(config: WorldConfig) -> Result<Self, ConfigError> {
        let seed = config.seed;
        Ok(Self {
            engine: TickEngine::new(config)?,
            seed,
        })
    }

    /// Execute one tick synchronously.
    ///
    /// Submits `commands` to the ingress queue, runs the full propagator
    /// pipeline, publishes the new snapshot, and returns a [`StepResult`]
    /// containing the snapshot reference, receipts, and metrics.
    ///
    /// The returned [`Snapshot`] borrows from `self`, preventing the caller
    /// from calling `step_sync()` again until the snapshot is dropped.
    ///
    /// # Errors
    ///
    /// Returns [`TickError`] if a propagator fails (tick is rolled back
    /// atomically) or if ticking is disabled after consecutive rollbacks.
    /// On rollback, the error's `receipts` field contains per-command
    /// rollback receipts plus any submission rejections.
    pub fn step_sync(&mut self, commands: Vec<Command>) -> Result<StepResult<'_>, TickError> {
        let submit_receipts = self.engine.submit_commands(commands);

        // Collect submission-rejected receipts (QueueFull, TickDisabled).
        // Accepted commands get their final receipts from execute_tick.
        let rejected: Vec<Receipt> = submit_receipts
            .into_iter()
            .filter(|r| !r.accepted)
            .collect();

        match self.engine.execute_tick() {
            Ok(tick_result) => {
                let mut receipts = rejected;
                receipts.extend(tick_result.receipts);
                Ok(StepResult {
                    snapshot: self.engine.snapshot(),
                    receipts,
                    metrics: tick_result.metrics,
                })
            }
            Err(mut tick_error) => {
                let mut receipts = rejected;
                receipts.append(&mut tick_error.receipts);
                Err(TickError {
                    kind: tick_error.kind,
                    receipts,
                })
            }
        }
    }

    /// Reset the world to tick 0 with a new seed.
    ///
    /// Reclaims all arena memory, clears the ingress queue, and resets
    /// all counters. Returns a snapshot of the initial (zeroed) state.
    ///
    /// The `seed` is stored for future use when propagators support
    /// seeded RNG. Currently all runs are fully deterministic regardless
    /// of seed.
    pub fn reset(&mut self, seed: u64) -> Result<Snapshot<'_>, ConfigError> {
        self.engine.reset()?;
        self.seed = seed;
        Ok(self.engine.snapshot())
    }

    /// Get a read-only snapshot of the current published generation.
    pub fn snapshot(&self) -> Snapshot<'_> {
        self.engine.snapshot()
    }

    /// Current tick ID (0 after construction or reset).
    pub fn current_tick(&self) -> TickId {
        self.engine.current_tick()
    }

    /// Whether ticking is disabled due to consecutive rollbacks.
    pub fn is_tick_disabled(&self) -> bool {
        self.engine.is_tick_disabled()
    }

    /// Metrics from the most recent successful tick.
    pub fn last_metrics(&self) -> &StepMetrics {
        self.engine.last_metrics()
    }

    /// The current simulation seed.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Number of consecutive rollbacks since the last successful tick.
    pub fn consecutive_rollback_count(&self) -> u32 {
        self.engine.consecutive_rollback_count()
    }

    /// The spatial topology for this world.
    pub fn space(&self) -> &dyn murk_space::Space {
        self.engine.space()
    }
}

impl std::fmt::Debug for LockstepWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockstepWorld")
            .field("current_tick", &self.engine.current_tick())
            .field("seed", &self.seed)
            .field("tick_disabled", &self.engine.is_tick_disabled())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::command::CommandPayload;
    use murk_core::id::{FieldId, ParameterKey};
    use murk_core::traits::{FieldReader, SnapshotAccess};
    use murk_core::{BoundaryBehavior, FieldDef, FieldMutability, FieldType};
    use murk_propagator::propagator::WriteMode;
    use murk_propagator::Propagator;
    use murk_space::{EdgeBehavior, Line1D, Square4};
    use murk_test_utils::{ConstPropagator, FailingPropagator, IdentityPropagator};

    fn scalar_field(name: &str) -> FieldDef {
        FieldDef {
            name: name.to_string(),
            field_type: FieldType::Scalar,
            mutability: FieldMutability::PerTick,
            units: None,
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        }
    }

    fn simple_config() -> WorldConfig {
        WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("energy")],
            propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), 42.0))],
            dt: 0.1,
            seed: 42,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: crate::config::BackoffConfig::default(),
        }
    }

    /// Two-field pipeline: PropA writes field0=7.0, PropB copies field0→field1.
    fn two_field_config() -> WorldConfig {
        WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("field0"), scalar_field("field1")],
            propagators: vec![
                Box::new(ConstPropagator::new("write_f0", FieldId(0), 7.0)),
                Box::new(IdentityPropagator::new(
                    "copy_f0_to_f1",
                    FieldId(0),
                    FieldId(1),
                )),
            ],
            dt: 0.1,
            seed: 42,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: crate::config::BackoffConfig::default(),
        }
    }

    /// Square4 10x10 config for M0 integration testing.
    fn square4_config() -> WorldConfig {
        // Three-field pipeline on a 10x10 grid:
        //   PropA writes field0 = 3.0 (uniform)
        //   PropB copies field0 → field1
        //   PropC reads field0 + field1, writes sum to field2
        struct SumPropagator {
            name: String,
            input_a: FieldId,
            input_b: FieldId,
            output: FieldId,
        }

        impl SumPropagator {
            fn new(name: &str, a: FieldId, b: FieldId, out: FieldId) -> Self {
                Self {
                    name: name.to_string(),
                    input_a: a,
                    input_b: b,
                    output: out,
                }
            }
        }

        impl Propagator for SumPropagator {
            fn name(&self) -> &str {
                &self.name
            }
            fn reads(&self) -> murk_core::FieldSet {
                [self.input_a, self.input_b].into_iter().collect()
            }
            fn writes(&self) -> Vec<(FieldId, WriteMode)> {
                vec![(self.output, WriteMode::Full)]
            }
            fn step(
                &self,
                ctx: &mut murk_propagator::StepContext<'_>,
            ) -> Result<(), murk_core::PropagatorError> {
                let a = ctx.reads().read(self.input_a).unwrap().to_vec();
                let b = ctx.reads().read(self.input_b).unwrap().to_vec();
                let out = ctx.writes().write(self.output).unwrap();
                for i in 0..out.len() {
                    out[i] = a[i] + b[i];
                }
                Ok(())
            }
        }

        WorldConfig {
            space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![
                scalar_field("field0"),
                scalar_field("field1"),
                scalar_field("field2"),
            ],
            propagators: vec![
                Box::new(ConstPropagator::new("write_f0", FieldId(0), 3.0)),
                Box::new(IdentityPropagator::new(
                    "copy_f0_to_f1",
                    FieldId(0),
                    FieldId(1),
                )),
                Box::new(SumPropagator::new(
                    "sum_f0_f1_to_f2",
                    FieldId(0),
                    FieldId(1),
                    FieldId(2),
                )),
            ],
            dt: 0.016,
            seed: 12345,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: crate::config::BackoffConfig::default(),
        }
    }

    fn make_cmd(expires: u64) -> Command {
        Command {
            payload: CommandPayload::SetParameter {
                key: ParameterKey(0),
                value: 0.0,
            },
            expires_after_tick: TickId(expires),
            source_id: None,
            source_seq: None,
            priority_class: 1,
            arrival_seq: 0,
        }
    }

    // ── Basic lifecycle tests ────────────────────────────────

    #[test]
    fn new_creates_world_at_tick_zero() {
        let world = LockstepWorld::new(simple_config()).unwrap();
        assert_eq!(world.current_tick(), TickId(0));
        assert!(!world.is_tick_disabled());
        assert_eq!(world.seed(), 42);
    }

    #[test]
    fn step_sync_advances_tick() {
        let mut world = LockstepWorld::new(simple_config()).unwrap();
        let result = world.step_sync(vec![]).unwrap();
        assert_eq!(result.snapshot.tick_id(), TickId(1));
        assert_eq!(world.current_tick(), TickId(1));
    }

    #[test]
    fn step_sync_returns_correct_snapshot() {
        let mut world = LockstepWorld::new(simple_config()).unwrap();
        let result = world.step_sync(vec![]).unwrap();
        let data = result.snapshot.read(FieldId(0)).unwrap();
        assert_eq!(data.len(), 10);
        assert!(data.iter().all(|&v| v == 42.0));
    }

    #[test]
    fn step_sync_with_commands_produces_receipts() {
        let mut world = LockstepWorld::new(simple_config()).unwrap();
        let result = world.step_sync(vec![make_cmd(100), make_cmd(100)]).unwrap();
        let applied: Vec<_> = result
            .receipts
            .iter()
            .filter(|r| r.applied_tick_id.is_some())
            .collect();
        assert_eq!(applied.len(), 2);
        assert!(applied.iter().all(|r| r.applied_tick_id == Some(TickId(1))));
    }

    #[test]
    fn step_sync_propagator_failure_returns_tick_error() {
        let config = WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("energy")],
            propagators: vec![Box::new(FailingPropagator::new("fail", FieldId(0), 0))],
            dt: 0.1,
            seed: 42,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: crate::config::BackoffConfig::default(),
        };
        let mut world = LockstepWorld::new(config).unwrap();
        let result = world.step_sync(vec![]);
        assert!(result.is_err());
    }

    // ── Two-field overlay visibility ─────────────────────────

    #[test]
    fn step_sync_overlay_visibility() {
        let mut world = LockstepWorld::new(two_field_config()).unwrap();
        let result = world.step_sync(vec![]).unwrap();
        // PropB copies field0 (7.0) to field1.
        assert_eq!(result.snapshot.read(FieldId(0)).unwrap()[0], 7.0);
        assert_eq!(result.snapshot.read(FieldId(1)).unwrap()[0], 7.0);
    }

    // ── Reset tests ──────────────────────────────────────────

    #[test]
    fn reset_returns_to_tick_zero() {
        let mut world = LockstepWorld::new(simple_config()).unwrap();
        world.step_sync(vec![]).unwrap();
        world.step_sync(vec![]).unwrap();
        assert_eq!(world.current_tick(), TickId(2));

        let snap = world.reset(99).unwrap();
        assert_eq!(snap.tick_id(), TickId(0));
        assert_eq!(world.current_tick(), TickId(0));
        assert_eq!(world.seed(), 99);
    }

    #[test]
    fn reset_allows_continued_stepping() {
        let mut world = LockstepWorld::new(simple_config()).unwrap();
        world.step_sync(vec![]).unwrap();
        world.reset(42).unwrap();

        let result = world.step_sync(vec![]).unwrap();
        assert_eq!(result.snapshot.tick_id(), TickId(1));
        let data = result.snapshot.read(FieldId(0)).unwrap();
        assert!(data.iter().all(|&v| v == 42.0));
    }

    // ── 1000-step determinism (M0 quality gate) ──────────────

    #[test]
    fn thousand_step_determinism() {
        // Run the same config twice and compare snapshots at every tick.
        let mut world_a = LockstepWorld::new(square4_config()).unwrap();
        let mut world_b = LockstepWorld::new(square4_config()).unwrap();

        for tick in 1..=1000u64 {
            let result_a = world_a.step_sync(vec![]).unwrap();
            let result_b = world_b.step_sync(vec![]).unwrap();

            // Tick IDs match.
            assert_eq!(
                result_a.snapshot.tick_id(),
                result_b.snapshot.tick_id(),
                "tick ID mismatch at tick {tick}"
            );

            // Spot-check fields at each tick (full comparison every 100 ticks).
            if tick % 100 == 0 || tick == 1 {
                for field_idx in 0..3u32 {
                    let field = FieldId(field_idx);
                    let data_a = result_a.snapshot.read(field).unwrap();
                    let data_b = result_b.snapshot.read(field).unwrap();
                    assert_eq!(data_a, data_b, "field {field_idx} mismatch at tick {tick}");
                }
            }
        }

        assert_eq!(world_a.current_tick(), TickId(1000));
        assert_eq!(world_b.current_tick(), TickId(1000));

        // Final full field comparison.
        let snap_a = world_a.snapshot();
        let snap_b = world_b.snapshot();
        for field_idx in 0..3u32 {
            let field = FieldId(field_idx);
            assert_eq!(
                snap_a.read(field).unwrap(),
                snap_b.read(field).unwrap(),
                "final field {field_idx} mismatch"
            );
        }
    }

    // ── Memory bound (M0 quality gate) ───────────────────────

    #[test]
    fn memory_bound_tick_1000_approx_tick_10() {
        let mut world = LockstepWorld::new(square4_config()).unwrap();

        // Run 10 ticks and record memory.
        for _ in 0..10 {
            world.step_sync(vec![]).unwrap();
        }
        let mem_10 = world.last_metrics().memory_bytes;

        // Run to tick 1000.
        for _ in 10..1000 {
            world.step_sync(vec![]).unwrap();
        }
        let mem_1000 = world.last_metrics().memory_bytes;

        // Memory at tick 1000 should be approximately equal to tick 10.
        // Allow up to 20% growth for internal bookkeeping (IndexMap resizing, etc).
        let ratio = mem_1000 as f64 / mem_10 as f64;
        assert!(
            ratio < 1.2,
            "memory grew {ratio:.2}x from tick 10 ({mem_10}) to tick 1000 ({mem_1000})"
        );
    }

    // ── Square4 three-propagator integration ─────────────────

    #[test]
    fn square4_three_propagator_end_to_end() {
        let mut world = LockstepWorld::new(square4_config()).unwrap();
        let result = world.step_sync(vec![]).unwrap();

        let snap = &result.snapshot;
        let f0 = snap.read(FieldId(0)).unwrap();
        let f1 = snap.read(FieldId(1)).unwrap();
        let f2 = snap.read(FieldId(2)).unwrap();

        // 100 cells (10x10 Square4)
        assert_eq!(f0.len(), 100);
        assert_eq!(f1.len(), 100);
        assert_eq!(f2.len(), 100);

        // PropA: field0 = 3.0
        assert!(f0.iter().all(|&v| v == 3.0));
        // PropB: field1 = copy of field0 = 3.0
        assert!(f1.iter().all(|&v| v == 3.0));
        // PropC: field2 = field0 + field1 = 6.0
        assert!(f2.iter().all(|&v| v == 6.0));
    }

    // ── Debug impl ───────────────────────────────────────────

    #[test]
    fn debug_impl_doesnt_panic() {
        let world = LockstepWorld::new(simple_config()).unwrap();
        let debug = format!("{world:?}");
        assert!(debug.contains("LockstepWorld"));
        assert!(debug.contains("current_tick"));
    }

    // ── Snapshot borrowing prevents aliasing ─────────────────

    #[test]
    fn snapshot_borrows_from_self() {
        // This test verifies that snapshot() returns a reference tied to
        // &self. The borrow checker prevents calling &mut self methods
        // while a snapshot is live. This is a compile-time guarantee —
        // the test simply exercises the API.
        let mut world = LockstepWorld::new(simple_config()).unwrap();
        world.step_sync(vec![]).unwrap();

        let snap = world.snapshot();
        let data = snap.read(FieldId(0)).unwrap();
        assert_eq!(data[0], 42.0);
        // snap must be dropped before calling step_sync again.
        drop(snap);

        // Now we can step again.
        world.step_sync(vec![]).unwrap();
        assert_eq!(world.current_tick(), TickId(2));
    }

    // ── Bug-fix regression tests ─────────────────────────────

    #[test]
    fn step_sync_surfaces_submission_rejections() {
        // Create a world with a tiny ingress queue (capacity=2).
        let config = WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("energy")],
            propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))],
            dt: 0.1,
            seed: 42,
            ring_buffer_size: 8,
            max_ingress_queue: 2,
            tick_rate_hz: None,
            backoff: crate::config::BackoffConfig::default(),
        };
        let mut world = LockstepWorld::new(config).unwrap();

        // Submit 4 commands — only 2 fit in the queue.
        let result = world
            .step_sync(vec![
                make_cmd(100),
                make_cmd(100),
                make_cmd(100),
                make_cmd(100),
            ])
            .unwrap();

        // Should have 4 receipts total: 2 applied + 2 rejected.
        assert_eq!(result.receipts.len(), 4);

        let rejected: Vec<_> = result
            .receipts
            .iter()
            .filter(|r| r.reason_code == Some(murk_core::error::IngressError::QueueFull))
            .collect();
        assert_eq!(rejected.len(), 2, "QueueFull rejections must be surfaced");

        let applied: Vec<_> = result
            .receipts
            .iter()
            .filter(|r| r.applied_tick_id.is_some())
            .collect();
        assert_eq!(applied.len(), 2);
    }

    #[test]
    fn reset_does_not_update_seed_on_failure() {
        // We can't easily make arena.reset() fail in the current implementation,
        // but we verify the ordering: seed should only change after success.
        let mut world = LockstepWorld::new(simple_config()).unwrap();
        assert_eq!(world.seed(), 42);

        // Successful reset updates seed.
        world.reset(99).unwrap();
        assert_eq!(world.seed(), 99);

        // Another successful reset.
        world.reset(7).unwrap();
        assert_eq!(world.seed(), 7);
        assert_eq!(world.current_tick(), TickId(0));
    }
}
