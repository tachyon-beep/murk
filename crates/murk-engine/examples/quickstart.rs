//! Murk Quickstart — a complete, minimal simulation from scratch.
//!
//! Demonstrates:
//!   1. Creating a space (Square4 grid)
//!   2. Defining fields with mutability classes
//!   3. Implementing a propagator (discrete diffusion with a fixed source)
//!   4. Building a WorldConfig and LockstepWorld
//!   5. Stepping, reading snapshots, injecting commands, and resetting
//!
//! Run with:
//!   cargo run --example quickstart

use murk_core::{
    BoundaryBehavior, Command, CommandPayload, FieldDef, FieldId, FieldMutability, FieldReader,
    FieldSet, FieldType, PropagatorError, SnapshotAccess, TickId,
};
use murk_engine::{BackoffConfig, LockstepWorld, WorldConfig};
use murk_propagator::{Propagator, StepContext, WriteMode};
use murk_space::{EdgeBehavior, Square4, Space};
use smallvec::smallvec;

// ─── Field IDs ──────────────────────────────────────────────────

const HEAT: FieldId = FieldId(0);

// ─── Grid parameters ────────────────────────────────────────────

const ROWS: u32 = 8;
const COLS: u32 = 8;
const CELL_COUNT: usize = (ROWS * COLS) as usize;
const DT: f64 = 1.0;
const DIFFUSION: f64 = 0.08;

// Source position (center of grid).
const SOURCE_R: usize = 4;
const SOURCE_C: usize = 4;

// ─── Propagator: discrete Laplacian diffusion ───────────────────
//
// Reads the previous tick's heat field (Jacobi style), computes
// the 4-connected discrete Laplacian, and writes updated values.
// One cell is pinned as a constant-temperature heat source.

struct DiffusionPropagator;

impl Propagator for DiffusionPropagator {
    fn name(&self) -> &str {
        "diffusion"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        // Jacobi read: always sees the frozen tick-start values,
        // regardless of what other propagators write this tick.
        [HEAT].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(HEAT, WriteMode::Full)]
    }

    fn max_dt(&self) -> Option<f64> {
        // CFL constraint: 4 * D * dt < 1 → dt < 1/(4*D) = 3.125
        Some(1.0 / (4.0 * DIFFUSION))
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let prev_heat = ctx.reads_previous().read(HEAT).ok_or_else(|| {
            PropagatorError::ExecutionFailed {
                reason: "heat field not readable".into(),
            }
        })?;

        // Copy into a local buffer — we need random access while
        // holding the mutable write buffer (split-borrow limitation).
        let prev: Vec<f32> = prev_heat.to_vec();
        let dt = ctx.dt();

        let out = ctx.writes().write(HEAT).ok_or_else(|| {
            PropagatorError::ExecutionFailed {
                reason: "heat field not writable".into(),
            }
        })?;

        for r in 0..ROWS as usize {
            for c in 0..COLS as usize {
                let idx = r * COLS as usize + c;

                // Pin the heat source to a constant temperature.
                if r == SOURCE_R && c == SOURCE_C {
                    out[idx] = 10.0;
                    continue;
                }

                // 4-connected Laplacian with absorb boundary (edge = self).
                let n = if r > 0 { prev[(r - 1) * COLS as usize + c] } else { prev[idx] };
                let s = if r < ROWS as usize - 1 { prev[(r + 1) * COLS as usize + c] } else { prev[idx] };
                let w = if c > 0 { prev[r * COLS as usize + c - 1] } else { prev[idx] };
                let e = if c < COLS as usize - 1 { prev[r * COLS as usize + c + 1] } else { prev[idx] };

                let laplacian = n + s + e + w - 4.0 * prev[idx];
                let new_val = prev[idx] + (DIFFUSION * dt) as f32 * laplacian;
                out[idx] = new_val.max(0.0);
            }
        }

        Ok(())
    }
}

// ─── Main ───────────────────────────────────────────────────────

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Murk Quickstart ===\n");

    // 1. Create a space: 8x8 grid, 4-connected, absorb at edges.
    let space = Square4::new(ROWS, COLS, EdgeBehavior::Absorb)?;
    println!(
        "Space: {}x{} Square4, {} cells, {} neighbors/cell (interior)",
        ROWS, COLS, space.cell_count(), 4
    );

    // 2. Define fields.
    let fields = vec![
        FieldDef {
            name: "heat".into(),
            field_type: FieldType::Scalar,
            mutability: FieldMutability::PerTick,
            units: Some("kelvin".into()),
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        },
    ];
    println!("Fields: heat (PerTick)");

    // 3. Build config.
    let config = WorldConfig {
        space: Box::new(space),
        fields,
        propagators: vec![Box::new(DiffusionPropagator)],
        dt: DT,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    // 4. Create world.
    let mut world = LockstepWorld::new(config)?;
    println!("World created. Seed: {}\n", world.seed());

    // 5. Run 50 ticks of diffusion, printing progress.
    println!("Running diffusion (source at ({}, {}))...", SOURCE_R, SOURCE_C);
    for _ in 0..50 {
        let result = world.step_sync(vec![])?;
        let tick = result.snapshot.tick_id().0;

        if tick % 10 == 0 {
            let heat = result.snapshot.read(HEAT).unwrap();
            let mean: f32 = heat.iter().sum::<f32>() / CELL_COUNT as f32;
            let max: f32 = heat.iter().cloned().fold(0.0_f32, f32::max);
            println!(
                "  tick {:>3}: mean_heat={:.4}, max_heat={:.4}, time={}μs",
                tick, mean, max, result.metrics.total_us,
            );
        }
    }

    // 6. Inject a command: set a second heat spot at (1, 1).
    println!("\nInjecting SetField command at (1, 1)...");
    let cmd = Command {
        payload: CommandPayload::SetField {
            coord: smallvec![1, 1],
            field_id: HEAT,
            value: 10.0,
        },
        expires_after_tick: TickId(u64::MAX),
        source_id: None,
        source_seq: None,
        priority_class: 1,
        arrival_seq: 0,
    };
    let result = world.step_sync(vec![cmd])?;
    println!(
        "  tick {:>3}: command accepted={}, time={}μs",
        result.snapshot.tick_id().0,
        result.receipts.first().is_some_and(|r| r.accepted),
        result.metrics.total_us,
    );

    // 7. Run 20 more ticks to see the perturbation spread.
    for _ in 0..20 {
        world.step_sync(vec![])?;
    }

    // 8. Read the final heat map and display it.
    let snap = world.snapshot();
    let heat = snap.read(HEAT).unwrap();
    println!("\nFinal heat map (tick {}):", snap.tick_id().0);
    for r in 0..ROWS as usize {
        let row: Vec<String> = (0..COLS as usize)
            .map(|c| {
                let v = heat[r * COLS as usize + c];
                if v >= 5.0 {
                    " ## ".into()
                } else if v >= 1.0 {
                    format!("{:4.1}", v)
                } else if v >= 0.01 {
                    format!(" .{} ", (v * 10.0) as u8)
                } else {
                    "  . ".into()
                }
            })
            .collect();
        println!("  {}", row.join(""));
    }

    // 9. Reset and verify.
    world.reset(123)?;
    println!("\nReset to seed 123, tick: {}", world.current_tick().0);

    println!("Done.");
    Ok(())
}
