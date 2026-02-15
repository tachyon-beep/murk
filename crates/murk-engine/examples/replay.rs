//! Murk Replay — recording and replaying a deterministic simulation.
//!
//! Demonstrates:
//!   1. Running a diffusion simulation on a small grid
//!   2. Recording each tick to a replay file using ReplayWriter
//!   3. Replaying the recording and verifying snapshot hashes
//!   4. Running the simulation a second time to prove determinism
//!
//! Run with:
//!   cargo run --example replay

use murk_core::{
    BoundaryBehavior, FieldDef, FieldId, FieldMutability, FieldReader, FieldSet, FieldType,
    PropagatorError, SnapshotAccess,
};
use murk_engine::{BackoffConfig, LockstepWorld, WorldConfig};
use murk_propagator::{Propagator, StepContext, WriteMode};
use murk_replay::{snapshot_hash, BuildMetadata, InitDescriptor, ReplayReader, ReplayWriter};
use murk_space::{EdgeBehavior, Square4};

// ---- Field IDs --------------------------------------------------------

const HEAT: FieldId = FieldId(0);

// ---- Grid parameters --------------------------------------------------

const ROWS: u32 = 8;
const COLS: u32 = 8;
const DT: f64 = 1.0;
const DIFFUSION: f64 = 0.08;
const NUM_TICKS: u64 = 30;

// Source position (center of grid).
const SOURCE_R: usize = 4;
const SOURCE_C: usize = 4;

// ---- Propagator: discrete Laplacian diffusion --------------------------

struct DiffusionPropagator;

impl Propagator for DiffusionPropagator {
    fn name(&self) -> &str {
        "diffusion"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        [HEAT].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(HEAT, WriteMode::Full)]
    }

    fn max_dt(&self) -> Option<f64> {
        Some(1.0 / (4.0 * DIFFUSION))
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let prev_heat =
            ctx.reads_previous()
                .read(HEAT)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: "heat field not readable".into(),
                })?;

        let prev: Vec<f32> = prev_heat.to_vec();
        let dt = ctx.dt();

        let out = ctx
            .writes()
            .write(HEAT)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: "heat field not writable".into(),
            })?;

        for r in 0..ROWS as usize {
            for c in 0..COLS as usize {
                let idx = r * COLS as usize + c;

                if r == SOURCE_R && c == SOURCE_C {
                    out[idx] = 10.0;
                    continue;
                }

                let n = if r > 0 {
                    prev[(r - 1) * COLS as usize + c]
                } else {
                    prev[idx]
                };
                let s = if r < ROWS as usize - 1 {
                    prev[(r + 1) * COLS as usize + c]
                } else {
                    prev[idx]
                };
                let w = if c > 0 {
                    prev[r * COLS as usize + c - 1]
                } else {
                    prev[idx]
                };
                let e = if c < COLS as usize - 1 {
                    prev[r * COLS as usize + c + 1]
                } else {
                    prev[idx]
                };

                let laplacian = n + s + e + w - 4.0 * prev[idx];
                let new_val = prev[idx] + (DIFFUSION * dt) as f32 * laplacian;
                out[idx] = new_val.max(0.0);
            }
        }

        Ok(())
    }
}

// ---- Helpers -----------------------------------------------------------

/// Build a WorldConfig for the diffusion simulation.
fn make_config() -> WorldConfig {
    let space = Square4::new(ROWS, COLS, EdgeBehavior::Absorb).expect("failed to create space");

    WorldConfig {
        space: Box::new(space),
        fields: vec![FieldDef {
            name: "heat".into(),
            field_type: FieldType::Scalar,
            mutability: FieldMutability::PerTick,
            units: Some("kelvin".into()),
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        }],
        propagators: vec![Box::new(DiffusionPropagator)],
        dt: DT,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    }
}

/// Replay header metadata (same for all runs in this example).
fn build_metadata() -> BuildMetadata {
    BuildMetadata {
        toolchain: "stable".into(),
        target_triple: "x86_64-unknown-linux-gnu".into(),
        murk_version: env!("CARGO_PKG_VERSION").into(),
        compile_flags: "".into(),
    }
}

/// Replay init descriptor matching our simulation parameters.
fn init_descriptor(cell_count: usize) -> InitDescriptor {
    InitDescriptor {
        seed: 42,
        config_hash: 0,
        field_count: 1,
        cell_count: cell_count as u64,
        space_descriptor: vec![],
    }
}

// ---- Main --------------------------------------------------------------

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Murk Replay Demo ===\n");

    // ----------------------------------------------------------------
    // Phase 1: Run a simulation and record every tick to a replay.
    // ----------------------------------------------------------------

    println!("--- Phase 1: Record ---\n");

    let config = make_config();
    let cell_count = config.space.cell_count();
    let meta = build_metadata();
    let init = init_descriptor(cell_count);

    let mut world = LockstepWorld::new(config)?;
    let mut replay_buf: Vec<u8> = Vec::new();
    let mut writer = ReplayWriter::new(&mut replay_buf, &meta, &init)?;

    println!(
        "Recording {} ticks of diffusion on {}x{} grid ({} cells)...\n",
        NUM_TICKS, ROWS, COLS, cell_count
    );

    for _ in 0..NUM_TICKS {
        let result = world.step_sync(vec![])?;
        let tick = result.snapshot.tick_id().0;

        // Record this tick's snapshot into the replay.
        // write_frame computes the snapshot hash internally.
        writer.write_frame(tick, &[], &result.snapshot as &dyn SnapshotAccess)?;

        if tick % 10 == 0 || tick == 1 {
            let heat = result.snapshot.read(HEAT).unwrap();
            let max: f32 = heat.iter().cloned().fold(0.0_f32, f32::max);
            let hash = snapshot_hash(&result.snapshot as &dyn SnapshotAccess, 1);
            println!(
                "  tick {:>3}: max_heat={:.4}, hash={:#018x}",
                tick, max, hash
            );
        }
    }

    let frames_written = writer.frames_written();
    drop(writer);

    println!(
        "\nRecorded {} frames ({} bytes)\n",
        frames_written,
        replay_buf.len()
    );

    // ----------------------------------------------------------------
    // Phase 2: Replay the recording and verify hashes.
    // ----------------------------------------------------------------

    println!("--- Phase 2: Replay & Verify ---\n");

    let mut reader = ReplayReader::open(replay_buf.as_slice())?;
    println!("Replay header:");
    println!("  toolchain:    {}", reader.metadata().toolchain);
    println!("  murk_version: {}", reader.metadata().murk_version);
    println!("  seed:         {}", reader.init_descriptor().seed);
    println!("  field_count:  {}", reader.init_descriptor().field_count);
    println!("  cell_count:   {}", reader.init_descriptor().cell_count);
    println!();

    // Run a fresh simulation alongside the replay to compare hashes.
    let mut verify_world = LockstepWorld::new(make_config())?;
    let mut mismatches = 0u64;

    while let Some(frame) = reader.next_frame()? {
        let result = verify_world.step_sync(vec![])?;
        let live_hash = snapshot_hash(&result.snapshot as &dyn SnapshotAccess, 1);

        if live_hash != frame.snapshot_hash {
            println!(
                "  MISMATCH at tick {}: recorded={:#018x}, live={:#018x}",
                frame.tick_id, frame.snapshot_hash, live_hash
            );
            mismatches += 1;
        } else if frame.tick_id % 10 == 0 || frame.tick_id == 1 {
            println!("  tick {:>3}: hash={:#018x}  OK", frame.tick_id, live_hash);
        }
    }

    if mismatches == 0 {
        println!(
            "\nAll {} frames verified -- hashes match!\n",
            frames_written
        );
    } else {
        println!(
            "\nWARNING: {} mismatches out of {} frames!\n",
            mismatches, frames_written
        );
    }

    // ----------------------------------------------------------------
    // Phase 3: Prove determinism by running the simulation a third
    //          time and comparing tick-by-tick hashes.
    // ----------------------------------------------------------------

    println!("--- Phase 3: Determinism Proof ---\n");

    let mut world_a = LockstepWorld::new(make_config())?;
    let mut world_b = LockstepWorld::new(make_config())?;
    let mut deterministic = true;

    for _ in 0..NUM_TICKS {
        let result_a = world_a.step_sync(vec![])?;
        let result_b = world_b.step_sync(vec![])?;

        let hash_a = snapshot_hash(&result_a.snapshot as &dyn SnapshotAccess, 1);
        let hash_b = snapshot_hash(&result_b.snapshot as &dyn SnapshotAccess, 1);

        let tick = result_a.snapshot.tick_id().0;

        if hash_a != hash_b {
            println!(
                "  DIVERGED at tick {}: A={:#018x}, B={:#018x}",
                tick, hash_a, hash_b
            );
            deterministic = false;
        } else if tick % 10 == 0 || tick == 1 {
            println!(
                "  tick {:>3}: A={:#018x}  B={:#018x}  ==",
                tick, hash_a, hash_b
            );
        }
    }

    if deterministic {
        println!(
            "\nDeterminism confirmed: {} ticks produced identical hashes across two independent runs.",
            NUM_TICKS
        );
    } else {
        println!("\nDeterminism BROKEN -- see divergences above.");
    }

    println!("\nDone.");
    Ok(())
}
