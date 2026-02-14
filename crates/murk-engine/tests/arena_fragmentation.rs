//! Integration test: arena fragmentation profiling under sparse field churn.
//!
//! Verifies that long-running simulations with sparse field churn do not
//! cause unbounded memory growth. The test runs 1000 ticks, toggling a
//! sparse field's value on alternating ticks, and asserts that memory
//! at tick 1000 is no more than 3x the memory at tick 100.

use murk_core::command::{Command, CommandPayload};
use murk_core::id::{FieldId, TickId};
use murk_core::{BoundaryBehavior, FieldDef, FieldMutability, FieldSet, FieldType, PropagatorError};
use murk_engine::config::{BackoffConfig, WorldConfig};
use murk_engine::lockstep::LockstepWorld;
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use smallvec::smallvec;

// ── Custom propagator that writes a constant to all cells ────────────

/// A simple propagator that writes 1.0 to every cell of a single field.
/// Used as the minimum viable propagator for the fragmentation test so
/// that the pipeline is valid.
struct FillPropagator {
    name: String,
    output: FieldId,
    value: f32,
}

impl FillPropagator {
    fn new(name: &str, output: FieldId, value: f32) -> Self {
        Self {
            name: name.to_string(),
            output,
            value,
        }
    }
}

impl Propagator for FillPropagator {
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
        let out = ctx
            .writes()
            .write(self.output)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("field {:?} not writable", self.output),
            })?;
        out.fill(self.value);
        Ok(())
    }
}

/// Build a WorldConfig with one PerTick field (written by propagator)
/// and one Sparse field (churned via SetField commands).
fn sparse_churn_config() -> WorldConfig {
    let fields = vec![
        FieldDef {
            name: "energy".to_string(),
            field_type: FieldType::Scalar,
            mutability: FieldMutability::PerTick,
            units: None,
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        },
        FieldDef {
            name: "sparse_marker".to_string(),
            field_type: FieldType::Scalar,
            mutability: FieldMutability::Sparse,
            units: None,
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        },
    ];

    WorldConfig {
        space: Box::new(
            murk_space::Line1D::new(100, murk_space::EdgeBehavior::Absorb).unwrap(),
        ),
        fields,
        propagators: vec![Box::new(FillPropagator::new("fill_energy", FieldId(0), 1.0))],
        dt: 0.1,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    }
}

/// Create a SetField command targeting the sparse field at a given cell.
fn set_sparse_cmd(cell: i32, value: f32, expires: u64) -> Command {
    Command {
        payload: CommandPayload::SetField {
            coord: smallvec![cell],
            field_id: FieldId(1), // sparse_marker
            value,
        },
        expires_after_tick: TickId(expires),
        source_id: None,
        source_seq: None,
        priority_class: 1,
        arrival_seq: 0,
    }
}

/// Run 1000 ticks with sparse field churn and verify bounded memory growth.
///
/// On every tick, we submit SetField commands to the sparse field for
/// several cells, alternating between setting a non-zero value and
/// clearing (setting to 0.0). This simulates the worst-case allocation
/// churn for sparse fields.
///
/// Memory at tick 1000 must be no more than 3x the memory at tick 100.
#[test]
#[ignore] // Resource-intensive test
fn sparse_field_churn_bounded_memory() {
    let mut world = LockstepWorld::new(sparse_churn_config()).unwrap();

    let mut memory_at_100: Option<usize> = None;

    for tick in 1..=1000u64 {
        // Generate churn commands: toggle sparse field values.
        // On odd ticks, set cells 0..9 to a non-zero value.
        // On even ticks, set cells 0..9 to 0.0 (clear).
        let value = if tick % 2 == 1 { tick as f32 } else { 0.0 };
        let commands: Vec<Command> = (0..10)
            .map(|cell| set_sparse_cmd(cell, value, tick + 10))
            .collect();

        world.step_sync(commands).unwrap();

        // Record memory at tick 100 and at every 100-tick boundary.
        if tick == 100 {
            memory_at_100 = Some(world.last_metrics().memory_bytes);
        }
    }

    let mem_100 = memory_at_100.expect("should have recorded memory at tick 100");
    let mem_1000 = world.last_metrics().memory_bytes;

    // Guard against degenerate case where memory_at_100 is zero (would
    // make the ratio check meaningless).
    assert!(
        mem_100 > 0,
        "memory at tick 100 should be non-zero, got {mem_100}"
    );

    let ratio = mem_1000 as f64 / mem_100 as f64;
    assert!(
        ratio <= 3.0,
        "memory grew {ratio:.2}x from tick 100 ({mem_100} bytes) to tick 1000 \
         ({mem_1000} bytes) -- expected at most 3x for bounded growth"
    );
}
