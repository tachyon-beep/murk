//! Integration test: NaN detection and tick rollback.
//!
//! Verifies that a propagator returning `PropagatorError::NanDetected`
//! causes the tick engine to roll back the tick atomically. The world
//! state must not advance, and the error must surface through the
//! `TickError` → `StepError::PropagatorFailed` chain.

use std::sync::atomic::{AtomicUsize, Ordering};

use murk_core::error::StepError;
use murk_core::id::{FieldId, TickId};
use murk_core::traits::SnapshotAccess;
use murk_core::{
    BoundaryBehavior, FieldDef, FieldMutability, FieldSet, FieldType, PropagatorError,
};
use murk_engine::config::{BackoffConfig, WorldConfig};
use murk_engine::lockstep::LockstepWorld;
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use murk_space::{EdgeBehavior, Line1D};

// ── NaN-producing propagator ─────────────────────────────────────────

/// A propagator that succeeds for a configurable number of ticks, then
/// returns `PropagatorError::NanDetected` on the next call.
///
/// Uses an atomic counter so it satisfies `Send` (required by the
/// `Propagator` trait bound).
struct NanOnTickPropagator {
    name: String,
    output: FieldId,
    /// Number of successful ticks before NaN is reported.
    succeed_count: usize,
    call_count: AtomicUsize,
}

impl NanOnTickPropagator {
    fn new(name: &str, output: FieldId, succeed_count: usize) -> Self {
        Self {
            name: name.to_string(),
            output,
            succeed_count,
            call_count: AtomicUsize::new(0),
        }
    }
}

impl Propagator for NanOnTickPropagator {
    fn name(&self) -> &str {
        &self.name
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.output, WriteMode::Full)]
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let n = self.call_count.fetch_add(1, Ordering::Relaxed);

        // Fill the output buffer with a valid value on success ticks.
        let out =
            ctx.writes()
                .write(self.output)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: format!("field {:?} not writable", self.output),
                })?;
        out.fill((n + 1) as f32);

        if n >= self.succeed_count {
            // Report NaN at cell index 0 in our output field.
            return Err(PropagatorError::NanDetected {
                field_id: self.output,
                cell_index: Some(0),
            });
        }

        Ok(())
    }
}

// ── Helper: build a WorldConfig with the NaN propagator ──────────────

fn nan_config(succeed_count: usize) -> WorldConfig {
    WorldConfig {
        space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
        fields: vec![FieldDef {
            name: "value".to_string(),
            field_type: FieldType::Scalar,
            mutability: FieldMutability::PerTick,
            units: None,
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        }],
        propagators: vec![Box::new(NanOnTickPropagator::new(
            "nan_prop",
            FieldId(0),
            succeed_count,
        ))],
        dt: 0.1,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    }
}

// ── Tests ────────────────────────────────────────────────────────────

#[test]
fn nan_detected_causes_tick_error() {
    // Propagator succeeds 4 times, fails on the 5th call (tick 5).
    let mut world = LockstepWorld::new(nan_config(4)).unwrap();

    // Ticks 1-4 should succeed.
    for tick in 1..=4u64 {
        let result = world.step_sync(vec![]);
        assert!(
            result.is_ok(),
            "tick {tick} should succeed, got: {:?}",
            result.err()
        );
        assert_eq!(world.current_tick(), TickId(tick));
    }

    // Tick 5 should fail with PropagatorFailed(NanDetected).
    let result = world.step_sync(vec![]);
    assert!(result.is_err(), "tick 5 should fail with NanDetected");
}

#[test]
fn nan_error_contains_propagator_failed_with_nan_detected() {
    let mut world = LockstepWorld::new(nan_config(4)).unwrap();

    // Advance through the 4 successful ticks.
    for _ in 1..=4 {
        world.step_sync(vec![]).unwrap();
    }

    // Tick 5 should fail.
    let result = world.step_sync(vec![]);
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("tick 5 should have failed with NanDetected"),
    };

    match &err.kind {
        StepError::PropagatorFailed { name, reason } => {
            assert_eq!(name, "nan_prop");
            match reason {
                PropagatorError::NanDetected {
                    field_id,
                    cell_index,
                } => {
                    assert_eq!(*field_id, FieldId(0));
                    assert_eq!(*cell_index, Some(0));
                }
                other => panic!("expected NanDetected reason, got: {other:?}"),
            }
        }
        other => panic!("expected PropagatorFailed, got: {other:?}"),
    }
}

#[test]
fn nan_rollback_does_not_advance_tick() {
    let mut world = LockstepWorld::new(nan_config(4)).unwrap();

    // Advance 4 ticks.
    for _ in 1..=4 {
        world.step_sync(vec![]).unwrap();
    }
    assert_eq!(world.current_tick(), TickId(4));

    // Tick 5 fails -- tick should NOT advance.
    let _ = world.step_sync(vec![]);
    assert_eq!(
        world.current_tick(),
        TickId(4),
        "tick should not advance after NaN rollback"
    );
}

#[test]
fn nan_rollback_preserves_snapshot() {
    let mut world = LockstepWorld::new(nan_config(4)).unwrap();

    // Run 4 ticks.
    for _ in 1..=4 {
        world.step_sync(vec![]).unwrap();
    }

    // Capture the snapshot state at tick 4.
    let tick_before = world.snapshot().tick_id();

    // Tick 5 fails with NaN.
    let _ = world.step_sync(vec![]);

    // Snapshot should be unchanged (still at tick 4).
    let snap_after = world.snapshot();
    assert_eq!(
        snap_after.tick_id(),
        tick_before,
        "snapshot tick should not change after NaN rollback"
    );
}

#[test]
fn nan_increments_rollback_counter() {
    let mut world = LockstepWorld::new(nan_config(4)).unwrap();

    // Run 4 successful ticks.
    for _ in 1..=4 {
        world.step_sync(vec![]).unwrap();
    }
    assert_eq!(world.consecutive_rollback_count(), 0);

    // Tick 5 fails -- rollback counter should increment.
    let _ = world.step_sync(vec![]);
    assert_eq!(
        world.consecutive_rollback_count(),
        1,
        "rollback counter should be 1 after first NaN failure"
    );
}

#[test]
fn nan_on_first_tick_rolls_back_to_zero() {
    // Propagator fails immediately on tick 1.
    let mut world = LockstepWorld::new(nan_config(0)).unwrap();
    assert_eq!(world.current_tick(), TickId(0));

    let result = world.step_sync(vec![]);
    assert!(result.is_err());

    // Should still be at tick 0.
    assert_eq!(
        world.current_tick(),
        TickId(0),
        "tick should remain at 0 after first-tick NaN rollback"
    );
}

#[test]
fn three_nan_failures_disable_ticking() {
    // Propagator fails immediately on every tick.
    let mut world = LockstepWorld::new(nan_config(0)).unwrap();

    for _ in 0..3 {
        let _ = world.step_sync(vec![]);
    }

    assert!(
        world.is_tick_disabled(),
        "ticking should be disabled after 3 consecutive NaN rollbacks"
    );
    assert_eq!(world.consecutive_rollback_count(), 3);

    // Next attempt should return TickDisabled.
    let result = world.step_sync(vec![]);
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("expected TickDisabled after 3 rollbacks"),
    };
    match &err.kind {
        StepError::TickDisabled => {}
        other => panic!("expected TickDisabled after 3 rollbacks, got: {other:?}"),
    }
}
