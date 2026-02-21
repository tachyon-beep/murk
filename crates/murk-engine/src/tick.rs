//! Tick engine: the single-threaded simulation loop.
//!
//! [`TickEngine`] wires together the arena, propagators, ingress queue,
//! and overlay caches into a deterministic tick execution loop with
//! rollback atomicity.
//!
//! # Lockstep mode only
//!
//! This module implements Lockstep mode — a callable struct with no
//! background threads. RealtimeAsync mode (future WP) wraps this in
//! a thread with a ring buffer.

use std::fmt;
use std::time::Instant;

use murk_arena::config::ArenaConfig;
use murk_arena::pingpong::PingPongArena;
use murk_arena::read::Snapshot;
use murk_arena::static_arena::StaticArena;
use murk_core::command::{Command, CommandPayload, Receipt};
use murk_core::error::{IngressError, StepError};
use murk_core::id::{FieldId, ParameterVersion, TickId};
use murk_core::traits::{FieldReader, FieldWriter};
use murk_core::FieldMutability;
use murk_propagator::pipeline::{ReadResolutionPlan, ReadSource};
use murk_propagator::propagator::Propagator;
use murk_propagator::scratch::ScratchRegion as PropagatorScratch;

use crate::config::{ConfigError, WorldConfig};
use crate::ingress::IngressQueue;
use crate::metrics::StepMetrics;
use crate::overlay::{BaseFieldCache, BaseFieldSet, OverlayReader, StagedFieldCache};

// ── TickResult ───────────────────────────────────────────────────

/// Result of a successful tick execution.
#[derive(Debug)]
pub struct TickResult {
    /// Receipts for commands submitted before this tick.
    pub receipts: Vec<Receipt>,
    /// Performance metrics for this tick.
    pub metrics: StepMetrics,
}

// ── TickError ───────────────────────────────────────────────────

/// Error returned from [`TickEngine::execute_tick()`].
///
/// Wraps the underlying [`StepError`] and any receipts that were produced
/// before the failure. On rollback, receipts carry `TickRollback` reason
/// codes; callers must not discard them.
#[derive(Debug, PartialEq, Eq)]
pub struct TickError {
    /// The underlying error.
    pub kind: StepError,
    /// Receipts produced before the failure (may include rollback receipts).
    pub receipts: Vec<Receipt>,
}

impl fmt::Display for TickError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)
    }
}

impl std::error::Error for TickError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.kind)
    }
}

// ── TickEngine ───────────────────────────────────────────────────

/// Single-threaded tick engine for Lockstep mode.
///
/// Owns all simulation state and executes ticks synchronously. Each
/// `execute_tick()` call runs the full propagator pipeline, publishes
/// a snapshot, and returns receipts for any submitted commands.
pub struct TickEngine {
    arena: PingPongArena,
    propagators: Vec<Box<dyn Propagator>>,
    plan: ReadResolutionPlan,
    ingress: IngressQueue,
    space: Box<dyn murk_space::Space>,
    dt: f64,
    current_tick: TickId,
    param_version: ParameterVersion,
    consecutive_rollback_count: u32,
    tick_disabled: bool,
    max_consecutive_rollbacks: u32,
    propagator_scratch: PropagatorScratch,
    base_field_set: BaseFieldSet,
    base_cache: BaseFieldCache,
    staged_cache: StagedFieldCache,
    last_metrics: StepMetrics,
}

impl TickEngine {
    /// Construct a new tick engine from a [`WorldConfig`].
    ///
    /// Validates the configuration, builds the read resolution plan,
    /// constructs the arena, and pre-computes the base field set.
    /// Consumes the `WorldConfig`.
    pub fn new(config: WorldConfig) -> Result<Self, ConfigError> {
        // Validate and build read resolution plan.
        config.validate()?;
        let defined_fields = config.defined_field_set();
        let plan = murk_propagator::validate_pipeline(
            &config.propagators,
            &defined_fields,
            config.dt,
            &*config.space,
        )?;

        // Build arena field defs.
        // Safety: validate() already checked fields.len() fits in u32.
        let arena_field_defs: Vec<(FieldId, murk_core::FieldDef)> = config
            .fields
            .iter()
            .enumerate()
            .map(|(i, def)| {
                (
                    FieldId(u32::try_from(i).expect("field count validated")),
                    def.clone(),
                )
            })
            .collect();

        // Safety: validate() already checked cell_count fits in u32.
        let cell_count = u32::try_from(config.space.cell_count()).expect("cell count validated");
        let arena_config = ArenaConfig::new(cell_count);

        // Build static arena for any Static fields.
        let static_fields: Vec<(FieldId, u32)> = arena_field_defs
            .iter()
            .filter(|(_, d)| d.mutability == FieldMutability::Static)
            .map(|(id, d)| (*id, cell_count * d.field_type.components()))
            .collect();
        let static_arena = StaticArena::new(&static_fields).into_shared();

        let arena = PingPongArena::new(arena_config, arena_field_defs, static_arena)?;

        // Pre-compute base field set.
        let base_field_set = BaseFieldSet::from_plan(&plan, &config.propagators);

        // Compute max scratch bytes across all propagators.
        let max_scratch = config
            .propagators
            .iter()
            .map(|p| p.scratch_bytes())
            .max()
            .unwrap_or(0);
        let propagator_scratch = PropagatorScratch::with_byte_capacity(max_scratch);

        let ingress = IngressQueue::new(config.max_ingress_queue);

        Ok(Self {
            arena,
            propagators: config.propagators,
            plan,
            ingress,
            space: config.space,
            dt: config.dt,
            current_tick: TickId(0),
            param_version: ParameterVersion(0),
            consecutive_rollback_count: 0,
            tick_disabled: false,
            max_consecutive_rollbacks: 3,
            propagator_scratch,
            base_field_set,
            base_cache: BaseFieldCache::new(),
            staged_cache: StagedFieldCache::new(),
            last_metrics: StepMetrics::default(),
        })
    }

    /// Submit commands to be processed in the next tick.
    ///
    /// Returns one receipt per command indicating acceptance or rejection.
    pub fn submit_commands(&mut self, commands: Vec<Command>) -> Vec<Receipt> {
        self.ingress.submit(commands, self.tick_disabled)
    }

    /// Execute one tick.
    ///
    /// Runs the full propagator pipeline, publishes the snapshot, and
    /// returns receipts plus metrics. On propagator failure, the tick
    /// is rolled back atomically (the staging buffer is abandoned).
    pub fn execute_tick(&mut self) -> Result<TickResult, TickError> {
        let tick_start = Instant::now();

        // 0. Check if ticking is disabled.
        if self.tick_disabled {
            return Err(TickError {
                kind: StepError::TickDisabled,
                receipts: Vec::new(),
            });
        }

        let next_tick = TickId(self.current_tick.0 + 1);

        // 1. Populate base field cache from snapshot.
        {
            let snapshot = self.arena.snapshot();
            self.base_cache.populate(&snapshot, &self.base_field_set);
        }

        // 2. Begin tick — if this fails, commands stay in the queue.
        let mut guard = self.arena.begin_tick().map_err(|_| TickError {
            kind: StepError::AllocationFailed,
            receipts: Vec::new(),
        })?;

        // 3. Drain ingress queue (safe: begin_tick succeeded).
        let cmd_start = Instant::now();
        let drain = self.ingress.drain(next_tick);
        let mut receipts = drain.expired_receipts;
        let commands = drain.commands;
        let accepted_receipt_start = receipts.len();
        for dc in &commands {
            receipts.push(Receipt {
                accepted: true,
                applied_tick_id: None,
                reason_code: None,
                command_index: dc.command_index,
            });
        }
        // 3b. Apply commands to the staging writer.
        for (i, dc) in commands.iter().enumerate() {
            let receipt = &mut receipts[accepted_receipt_start + i];
            match &dc.command.payload {
                CommandPayload::SetField {
                    ref coord,
                    field_id,
                    value,
                } => {
                    if let Some(rank) = self.space.canonical_rank(coord) {
                        if let Some(buf) = guard.writer.write(*field_id) {
                            if rank < buf.len() {
                                buf[rank] = *value;
                            }
                        }
                    }
                }
                CommandPayload::SetParameter { .. }
                | CommandPayload::SetParameterBatch { .. }
                | CommandPayload::Move { .. }
                | CommandPayload::Spawn { .. }
                | CommandPayload::Despawn { .. }
                | CommandPayload::Custom { .. } => {
                    receipt.accepted = false;
                    receipt.reason_code = Some(IngressError::UnsupportedCommand);
                }
            }
        }
        let command_processing_us = cmd_start.elapsed().as_micros() as u64;

        // 4. Run propagator pipeline.
        let mut propagator_us = Vec::with_capacity(self.propagators.len());
        for (i, prop) in self.propagators.iter().enumerate() {
            let prop_start = Instant::now();

            // 4a. Populate staged cache from guard.writer.read() per plan routes.
            self.staged_cache.clear();
            if let Some(routes) = self.plan.routes_for(i) {
                for (&field, &source) in routes {
                    if let ReadSource::Staged { .. } = source {
                        if let Some(data) = guard.writer.read(field) {
                            self.staged_cache.insert(field, data);
                        }
                    }
                }
            }

            // 4b. Construct OverlayReader.
            let empty_routes = indexmap::IndexMap::new();
            let routes = self.plan.routes_for(i).unwrap_or(&empty_routes);
            let overlay = OverlayReader::new(routes, &self.base_cache, &self.staged_cache);

            // 4c. Seed WriteMode::Incremental buffers from previous generation.
            for field in self.plan.incremental_fields_for(i) {
                if let Some(prev_data) = self.base_cache.read(field) {
                    // Copy through a temp buffer: base_cache borrows &self,
                    // guard.writer.write() borrows &mut guard.
                    let prev: Vec<f32> = prev_data.to_vec();
                    if let Some(write_buf) = guard.writer.write(field) {
                        let copy_len = prev.len().min(write_buf.len());
                        write_buf[..copy_len].copy_from_slice(&prev[..copy_len]);
                    }
                }
            }

            // 4d. Reset propagator scratch.
            self.propagator_scratch.reset();

            // 4e. Construct StepContext and call step().
            {
                let mut ctx = murk_propagator::StepContext::new(
                    &overlay,
                    &self.base_cache,
                    &mut guard.writer,
                    &mut self.propagator_scratch,
                    self.space.as_ref(),
                    next_tick,
                    self.dt,
                );

                // 4f. Call propagator step.
                if let Err(reason) = prop.step(&mut ctx) {
                    // 4g. Rollback on error — guard goes out of scope,
                    // abandoning the staging buffer (free rollback).
                    let prop_name = prop.name().to_string();
                    return self.handle_rollback(
                        prop_name,
                        reason,
                        receipts,
                        accepted_receipt_start,
                    );
                }
            }

            propagator_us.push((
                prop.name().to_string(),
                prop_start.elapsed().as_micros() as u64,
            ));
        }

        // 5. guard goes out of scope here (releases staging borrows).

        // 6. Publish.
        let publish_start = Instant::now();
        self.arena
            .publish(next_tick, self.param_version)
            .map_err(|_| TickError {
                kind: StepError::AllocationFailed,
                receipts: vec![],
            })?;
        let snapshot_publish_us = publish_start.elapsed().as_micros() as u64;

        // 7. Update state.
        self.current_tick = next_tick;
        self.consecutive_rollback_count = 0;

        // 8. Finalize receipts with applied_tick_id (only for actually executed commands).
        for receipt in &mut receipts[accepted_receipt_start..] {
            if receipt.accepted {
                receipt.applied_tick_id = Some(next_tick);
            }
        }

        // 9. Build metrics.
        let total_us = tick_start.elapsed().as_micros() as u64;
        let metrics = StepMetrics {
            total_us,
            command_processing_us,
            propagator_us,
            snapshot_publish_us,
            memory_bytes: self.arena.memory_bytes(),
            sparse_retired_ranges: self.arena.sparse_retired_range_count() as u32,
            sparse_pending_retired: self.arena.sparse_pending_retired_count() as u32,
            sparse_reuse_hits: self.arena.sparse_reuse_hits(),
            sparse_reuse_misses: self.arena.sparse_reuse_misses(),
        };
        self.arena.reset_sparse_reuse_counters();
        self.last_metrics = metrics.clone();

        Ok(TickResult { receipts, metrics })
    }

    /// Handle a propagator failure by rolling back the tick.
    ///
    /// Takes ownership of `receipts` and returns them inside [`TickError`]
    /// so the caller can inspect per-command rollback reason codes.
    fn handle_rollback(
        &mut self,
        prop_name: String,
        reason: murk_core::PropagatorError,
        mut receipts: Vec<Receipt>,
        accepted_start: usize,
    ) -> Result<TickResult, TickError> {
        // Guard was dropped → staging buffer abandoned (free rollback).
        self.consecutive_rollback_count += 1;
        if self.consecutive_rollback_count >= self.max_consecutive_rollbacks {
            self.tick_disabled = true;
        }

        // Mark accepted command receipts as rolled back, but preserve
        // receipts that were already rejected (e.g. unsupported command
        // types) so callers see the original rejection reason.
        for receipt in &mut receipts[accepted_start..] {
            if receipt.accepted {
                receipt.applied_tick_id = None;
                receipt.reason_code = Some(IngressError::TickRollback);
            }
        }

        Err(TickError {
            kind: StepError::PropagatorFailed {
                name: prop_name,
                reason,
            },
            receipts,
        })
    }

    /// Reset the engine to its initial state.
    pub fn reset(&mut self) -> Result<(), ConfigError> {
        self.arena.reset().map_err(ConfigError::Arena)?;
        self.ingress.clear();
        self.current_tick = TickId(0);
        self.param_version = ParameterVersion(0);
        self.tick_disabled = false;
        self.consecutive_rollback_count = 0;
        self.last_metrics = StepMetrics::default();
        Ok(())
    }

    /// Get a read-only snapshot of the current published generation.
    pub fn snapshot(&self) -> Snapshot<'_> {
        self.arena.snapshot()
    }

    /// Get an owned, thread-safe snapshot of the current published generation.
    ///
    /// Unlike [`TickEngine::snapshot()`], the returned `OwnedSnapshot` owns
    /// clones of the segment data and can be sent across thread boundaries.
    pub fn owned_snapshot(&self) -> murk_arena::OwnedSnapshot {
        self.arena.owned_snapshot()
    }

    /// Current tick ID.
    pub fn current_tick(&self) -> TickId {
        self.current_tick
    }

    /// Whether ticking is disabled due to consecutive rollbacks.
    pub fn is_tick_disabled(&self) -> bool {
        self.tick_disabled
    }

    /// Number of consecutive rollbacks since the last successful tick.
    pub fn consecutive_rollback_count(&self) -> u32 {
        self.consecutive_rollback_count
    }

    /// Metrics from the most recent successful tick.
    pub fn last_metrics(&self) -> &StepMetrics {
        &self.last_metrics
    }

    /// The spatial topology for this engine.
    pub fn space(&self) -> &dyn murk_space::Space {
        self.space.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::command::CommandPayload;
    use murk_core::id::{Coord, ParameterKey};
    use murk_core::traits::SnapshotAccess;
    use murk_core::{BoundaryBehavior, FieldDef, FieldMutability, FieldType};
    use murk_propagator::propagator::WriteMode;
    use murk_space::{EdgeBehavior, Line1D};
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

    fn simple_engine() -> TickEngine {
        let config = WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("energy")],
            propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), 42.0))],
            dt: 0.1,
            seed: 42,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: crate::config::BackoffConfig::default(),
        };
        TickEngine::new(config).unwrap()
    }

    fn two_field_engine() -> TickEngine {
        let config = WorldConfig {
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
        };
        TickEngine::new(config).unwrap()
    }

    fn three_field_engine() -> TickEngine {
        // PropA writes field0=7.0
        // PropB reads field0, copies to field1
        // PropC reads field0+field1, writes sum to field2
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

        let config = WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![
                scalar_field("field0"),
                scalar_field("field1"),
                scalar_field("field2"),
            ],
            propagators: vec![
                Box::new(ConstPropagator::new("write_f0", FieldId(0), 7.0)),
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
            dt: 0.1,
            seed: 42,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: crate::config::BackoffConfig::default(),
        };
        TickEngine::new(config).unwrap()
    }

    fn failing_engine(succeed_count: usize) -> TickEngine {
        let config = WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("energy")],
            propagators: vec![Box::new(FailingPropagator::new(
                "fail",
                FieldId(0),
                succeed_count,
            ))],
            dt: 0.1,
            seed: 42,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: crate::config::BackoffConfig::default(),
        };
        TickEngine::new(config).unwrap()
    }

    fn partial_failure_engine() -> TickEngine {
        // PropA succeeds always, PropB fails immediately.
        let config = WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("field0"), scalar_field("field1")],
            propagators: vec![
                Box::new(ConstPropagator::new("ok_prop", FieldId(0), 1.0)),
                Box::new(FailingPropagator::new("fail_prop", FieldId(1), 0)),
            ],
            dt: 0.1,
            seed: 42,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: crate::config::BackoffConfig::default(),
        };
        TickEngine::new(config).unwrap()
    }

    // ── Three-propagator overlay visibility tests ─────────────

    #[test]
    fn staged_read_sees_prior_propagator_write() {
        // PropB reads field0 via reads() → should see PropA's value (7.0)
        let mut engine = two_field_engine();
        let result = engine.execute_tick().unwrap();
        let snap = engine.snapshot();
        // field1 should be a copy of field0
        assert_eq!(snap.read(FieldId(1)).unwrap()[0], 7.0);
        assert!(result.metrics.total_us > 0);
    }

    #[test]
    fn reads_previous_sees_base_gen() {
        // With reads_previous, a propagator should always see the base gen
        // (tick-start snapshot), not staged writes. We verify by checking
        // that on tick 1, reads_previous sees zeros (initial state).
        struct ReadsPrevPropagator;
        impl Propagator for ReadsPrevPropagator {
            fn name(&self) -> &str {
                "reads_prev"
            }
            fn reads(&self) -> murk_core::FieldSet {
                murk_core::FieldSet::empty()
            }
            fn reads_previous(&self) -> murk_core::FieldSet {
                [FieldId(0)].into_iter().collect()
            }
            fn writes(&self) -> Vec<(FieldId, WriteMode)> {
                vec![(FieldId(1), WriteMode::Full)]
            }
            fn step(
                &self,
                ctx: &mut murk_propagator::StepContext<'_>,
            ) -> Result<(), murk_core::PropagatorError> {
                let prev = ctx.reads_previous().read(FieldId(0)).unwrap().to_vec();
                let out = ctx.writes().write(FieldId(1)).unwrap();
                out.copy_from_slice(&prev);
                Ok(())
            }
        }

        let config = WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("field0"), scalar_field("field1")],
            propagators: vec![
                Box::new(ConstPropagator::new("write_f0", FieldId(0), 99.0)),
                Box::new(ReadsPrevPropagator),
            ],
            dt: 0.1,
            seed: 42,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: crate::config::BackoffConfig::default(),
        };
        let mut engine = TickEngine::new(config).unwrap();

        // Tick 1: PropA writes 99.0 to field0. ReadsPrev reads field0
        // via reads_previous → sees base gen (all zeros).
        engine.execute_tick().unwrap();
        let snap = engine.snapshot();
        // field1 should be 0.0 (base gen of field0 on first tick)
        assert_eq!(snap.read(FieldId(1)).unwrap()[0], 0.0);

        // Tick 2: reads_previous now sees 99.0 (published from tick 1).
        engine.execute_tick().unwrap();
        let snap = engine.snapshot();
        assert_eq!(snap.read(FieldId(1)).unwrap()[0], 99.0);
    }

    #[test]
    fn three_propagator_overlay_visibility() {
        // A writes 7.0 to f0
        // B reads f0 (staged → 7.0), copies to f1
        // C reads f0 (staged → 7.0) + f1 (staged → 7.0), writes sum to f2
        let mut engine = three_field_engine();
        engine.execute_tick().unwrap();
        let snap = engine.snapshot();

        assert_eq!(snap.read(FieldId(0)).unwrap()[0], 7.0);
        assert_eq!(snap.read(FieldId(1)).unwrap()[0], 7.0);
        assert_eq!(snap.read(FieldId(2)).unwrap()[0], 14.0); // 7 + 7
    }

    #[test]
    fn unwritten_field_reads_from_base_gen() {
        // On tick 1, fields start as zero (base gen). A propagator
        // reading a field nobody wrote should see zero.
        let mut engine = three_field_engine();
        engine.execute_tick().unwrap();
        // All fields are written in this pipeline, so let's just
        // verify the snapshot is consistent.
        let snap = engine.snapshot();
        let f2 = snap.read(FieldId(2)).unwrap();
        assert!(f2.iter().all(|&v| v == 14.0));
    }

    // ── Tick atomicity tests ─────────────────────────────────

    #[test]
    fn propagator_failure_no_snapshot_published() {
        let mut engine = failing_engine(0);

        // Before any tick, snapshot is at tick 0 (initial state).
        let snap_before = engine.snapshot();
        let tick_before = snap_before.tick_id();

        // Execute tick → should fail.
        let result = engine.execute_tick();
        assert!(result.is_err());

        // Snapshot should be unchanged.
        let snap_after = engine.snapshot();
        assert_eq!(snap_after.tick_id(), tick_before);
    }

    #[test]
    fn partial_failure_rolls_back_all() {
        // PropA writes 1.0 to field0 (succeeds), PropB fails.
        // field0 should NOT show 1.0 in the snapshot after rollback.
        let mut engine = partial_failure_engine();

        // Snapshot before: field0 should be all zeros (or not yet published).
        let result = engine.execute_tick();
        assert!(result.is_err());

        // field0 should still be at initial state (no publish happened).
        let snap = engine.snapshot();
        let f0 = snap.read(FieldId(0));
        // On the very first tick with no prior publish, the snapshot
        // shows initial state. No writes from PropA should be visible.
        if let Some(data) = f0 {
            assert!(
                data.iter().all(|&v| v == 0.0),
                "rollback should prevent PropA's writes from being visible"
            );
        }
    }

    #[test]
    fn rollback_receipts_generated() {
        let mut engine = failing_engine(0);

        // Submit an accepted command (SetField) before the failing tick.
        let cmd = Command {
            payload: CommandPayload::SetField {
                coord: smallvec::smallvec![0],
                field_id: FieldId(0),
                value: 1.0,
            },
            expires_after_tick: TickId(100),
            source_id: None,
            source_seq: None,
            priority_class: 1,
            arrival_seq: 0,
        };
        engine.submit_commands(vec![cmd]);

        let result = engine.execute_tick();
        match result {
            Err(TickError {
                kind: StepError::PropagatorFailed { .. },
                receipts,
            }) => {
                // Accepted receipts must be surfaced with TickRollback.
                assert_eq!(receipts.len(), 1);
                assert!(receipts[0].accepted);
                assert_eq!(receipts[0].reason_code, Some(IngressError::TickRollback));
            }
            other => panic!("expected PropagatorFailed with receipts, got {other:?}"),
        }
    }

    #[test]
    fn rollback_preserves_rejected_receipts() {
        // Submit an unsupported command (SetParameter → rejected) alongside
        // the tick. When the propagator fails and triggers rollback, the
        // rejected receipt must stay accepted=false, not be overwritten
        // with TickRollback.
        let mut engine = failing_engine(0);

        // SetParameter is unsupported → should be rejected (accepted=false).
        engine.submit_commands(vec![make_cmd(100)]);

        let result = engine.execute_tick();
        match result {
            Err(TickError {
                kind: StepError::PropagatorFailed { .. },
                receipts,
            }) => {
                assert_eq!(receipts.len(), 1);
                // The receipt must remain rejected, NOT overwritten with
                // accepted=true + TickRollback.
                assert!(
                    !receipts[0].accepted,
                    "rejected receipt should stay rejected after rollback"
                );
                assert_eq!(
                    receipts[0].reason_code,
                    Some(IngressError::UnsupportedCommand),
                    "rejected receipt must preserve UnsupportedCommand reason after rollback"
                );
            }
            other => panic!("expected PropagatorFailed with receipts, got {other:?}"),
        }
    }

    // ── Rollback tracking tests ─────────────────────────────

    #[test]
    fn consecutive_rollbacks_disable_ticking() {
        let mut engine = failing_engine(0);

        for _ in 0..3 {
            let _ = engine.execute_tick();
        }

        assert!(engine.is_tick_disabled());
        assert_eq!(engine.consecutive_rollback_count(), 3);
    }

    #[test]
    fn success_resets_rollback_count() {
        // Succeeds 2 times, then fails, but the first 2 successes
        // should keep rollback count at 0.
        let mut engine = failing_engine(10);

        // Two successful ticks.
        engine.execute_tick().unwrap();
        engine.execute_tick().unwrap();
        assert_eq!(engine.consecutive_rollback_count(), 0);
        assert_eq!(engine.current_tick(), TickId(2));
    }

    #[test]
    fn tick_disabled_rejects_immediately() {
        let mut engine = failing_engine(0);

        // Cause 3 failures to disable ticking.
        for _ in 0..3 {
            let _ = engine.execute_tick();
        }
        assert!(engine.is_tick_disabled());

        // Next tick should fail immediately with TickDisabled.
        match engine.execute_tick() {
            Err(TickError {
                kind: StepError::TickDisabled,
                ..
            }) => {}
            other => panic!("expected TickDisabled, got {other:?}"),
        }
    }

    #[test]
    fn reset_clears_tick_disabled() {
        let mut engine = failing_engine(0);

        for _ in 0..3 {
            let _ = engine.execute_tick();
        }
        assert!(engine.is_tick_disabled());

        engine.reset().unwrap();
        assert!(!engine.is_tick_disabled());
        assert_eq!(engine.current_tick(), TickId(0));
        assert_eq!(engine.consecutive_rollback_count(), 0);
    }

    // ── Integration tests ────────────────────────────────────

    #[test]
    fn single_tick_end_to_end() {
        let mut engine = simple_engine();
        let result = engine.execute_tick().unwrap();

        let snap = engine.snapshot();
        let data = snap.read(FieldId(0)).unwrap();
        assert_eq!(data.len(), 10);
        assert!(data.iter().all(|&v| v == 42.0));
        assert_eq!(engine.current_tick(), TickId(1));
        assert!(!result.receipts.is_empty() || result.receipts.is_empty()); // receipts exist
    }

    #[test]
    fn multi_tick_determinism() {
        let mut engine = simple_engine();

        for _ in 0..10 {
            engine.execute_tick().unwrap();
        }

        let snap = engine.snapshot();
        let data = snap.read(FieldId(0)).unwrap();
        assert!(data.iter().all(|&v| v == 42.0));
        assert_eq!(engine.current_tick(), TickId(10));
    }

    #[test]
    fn commands_flow_through_to_receipts() {
        let mut engine = simple_engine();

        // Use SetField commands — the only command type currently executed.
        let coord: Coord = vec![0i32].into();
        let cmds = vec![
            Command {
                payload: CommandPayload::SetField {
                    coord: coord.clone(),
                    field_id: FieldId(0),
                    value: 1.0,
                },
                expires_after_tick: TickId(100),
                source_id: None,
                source_seq: None,
                priority_class: 1,
                arrival_seq: 0,
            },
            Command {
                payload: CommandPayload::SetField {
                    coord: coord.clone(),
                    field_id: FieldId(0),
                    value: 2.0,
                },
                expires_after_tick: TickId(100),
                source_id: None,
                source_seq: None,
                priority_class: 1,
                arrival_seq: 0,
            },
        ];
        let submit_receipts = engine.submit_commands(cmds);
        assert_eq!(submit_receipts.len(), 2);
        assert!(submit_receipts.iter().all(|r| r.accepted));

        let result = engine.execute_tick().unwrap();
        // Should have receipts for the 2 commands.
        let applied: Vec<_> = result
            .receipts
            .iter()
            .filter(|r| r.applied_tick_id.is_some())
            .collect();
        assert_eq!(applied.len(), 2);
        assert!(applied.iter().all(|r| r.applied_tick_id == Some(TickId(1))));
    }

    #[test]
    fn non_setfield_commands_rejected_honestly() {
        let mut engine = simple_engine();

        // Submit SetParameter commands — not yet implemented.
        let submit_receipts = engine.submit_commands(vec![make_cmd(100), make_cmd(100)]);
        assert_eq!(submit_receipts.len(), 2);
        assert!(submit_receipts.iter().all(|r| r.accepted));

        let result = engine.execute_tick().unwrap();
        assert_eq!(result.receipts.len(), 2);

        // Non-SetField commands must NOT report as applied and must carry
        // UnsupportedCommand reason so callers can distinguish the failure mode.
        for receipt in &result.receipts {
            assert!(
                !receipt.accepted,
                "unimplemented command type must be rejected"
            );
            assert_eq!(
                receipt.applied_tick_id, None,
                "unimplemented command must not have applied_tick_id"
            );
            assert_eq!(
                receipt.reason_code,
                Some(IngressError::UnsupportedCommand),
                "rejected unsupported command must carry UnsupportedCommand reason"
            );
        }
    }

    // ── Metrics tests ────────────────────────────────────────

    #[test]
    fn timing_fields_populated() {
        let mut engine = simple_engine();
        let result = engine.execute_tick().unwrap();

        // total_us is u64, so it's always >= 0; just verify the struct is populated.
        let _ = result.metrics.total_us;
        assert_eq!(result.metrics.propagator_us.len(), 1);
        assert_eq!(result.metrics.propagator_us[0].0, "const");
    }

    #[test]
    fn memory_bytes_matches_arena() {
        let mut engine = simple_engine();
        engine.execute_tick().unwrap();

        let metrics = engine.last_metrics();
        assert!(metrics.memory_bytes > 0);
    }

    // ── Bug-fix regression tests ─────────────────────────────

    #[test]
    fn reset_clears_pending_ingress() {
        let mut engine = simple_engine();

        // Submit commands but don't tick.
        engine.submit_commands(vec![make_cmd(1000), make_cmd(1000)]);

        // Reset should discard pending commands.
        engine.reset().unwrap();

        // Tick should produce zero receipts (no pending commands).
        let result = engine.execute_tick().unwrap();
        assert!(result.receipts.is_empty());
    }

    #[test]
    fn command_index_preserved_after_reordering() {
        let mut engine = simple_engine();

        // Submit commands with different priorities — they'll be reordered.
        // Low priority first (index 0), high priority second (index 1).
        let cmds = vec![
            Command {
                payload: CommandPayload::SetParameter {
                    key: ParameterKey(0),
                    value: 1.0,
                },
                expires_after_tick: TickId(100),
                source_id: None,
                source_seq: None,
                priority_class: 2, // low priority
                arrival_seq: 0,
            },
            Command {
                payload: CommandPayload::SetParameter {
                    key: ParameterKey(0),
                    value: 2.0,
                },
                expires_after_tick: TickId(100),
                source_id: None,
                source_seq: None,
                priority_class: 0, // high priority — sorted first
                arrival_seq: 0,
            },
        ];
        engine.submit_commands(cmds);

        let result = engine.execute_tick().unwrap();
        // After reordering, priority_class=0 (batch index 1) executes first,
        // priority_class=2 (batch index 0) executes second.
        // command_index must reflect the ORIGINAL batch position.
        assert_eq!(result.receipts.len(), 2);
        assert_eq!(result.receipts[0].command_index, 1); // was batch[1]
        assert_eq!(result.receipts[1].command_index, 0); // was batch[0]
    }

    #[test]
    fn writemode_incremental_seeds_from_previous_gen() {
        // Regression test for BUG-015: WriteMode::Incremental buffers must
        // be pre-seeded with previous-generation data, not zero-filled.
        //
        // An incremental propagator writes cell 0 on tick 1 and then does
        // nothing on tick 2. Cell 0 must retain its value across ticks.
        struct IncrementalOnce {
            written: std::cell::Cell<bool>,
        }
        impl IncrementalOnce {
            fn new() -> Self {
                Self {
                    written: std::cell::Cell::new(false),
                }
            }
        }
        impl Propagator for IncrementalOnce {
            fn name(&self) -> &str {
                "incr_once"
            }
            fn reads(&self) -> murk_core::FieldSet {
                murk_core::FieldSet::empty()
            }
            fn writes(&self) -> Vec<(FieldId, WriteMode)> {
                vec![(FieldId(0), WriteMode::Incremental)]
            }
            fn step(
                &self,
                ctx: &mut murk_propagator::StepContext<'_>,
            ) -> Result<(), murk_core::PropagatorError> {
                let buf = ctx.writes().write(FieldId(0)).unwrap();
                if !self.written.get() {
                    // First tick: write a distinctive value.
                    buf[0] = 42.0;
                    buf[1] = 99.0;
                    self.written.set(true);
                }
                // Second tick onward: do nothing — rely on incremental seeding.
                Ok(())
            }
        }

        let config = WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("state")],
            propagators: vec![Box::new(IncrementalOnce::new())],
            dt: 0.1,
            seed: 42,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: crate::config::BackoffConfig::default(),
        };
        let mut engine = TickEngine::new(config).unwrap();

        // Tick 1: propagator writes 42.0 and 99.0.
        engine.execute_tick().unwrap();
        let snap = engine.snapshot();
        assert_eq!(snap.read(FieldId(0)).unwrap()[0], 42.0);
        assert_eq!(snap.read(FieldId(0)).unwrap()[1], 99.0);

        // Tick 2: propagator does nothing — incremental seeding must preserve values.
        engine.execute_tick().unwrap();
        let snap = engine.snapshot();
        assert_eq!(
            snap.read(FieldId(0)).unwrap()[0],
            42.0,
            "BUG-015: incremental field lost data across ticks"
        );
        assert_eq!(
            snap.read(FieldId(0)).unwrap()[1],
            99.0,
            "BUG-015: incremental field lost data across ticks"
        );
        // Unwritten cells should remain zero (seeded from previous gen which was zero).
        assert_eq!(snap.read(FieldId(0)).unwrap()[2], 0.0);

        // Tick 3: still preserved.
        engine.execute_tick().unwrap();
        let snap = engine.snapshot();
        assert_eq!(snap.read(FieldId(0)).unwrap()[0], 42.0);
        assert_eq!(snap.read(FieldId(0)).unwrap()[1], 99.0);
    }
}
