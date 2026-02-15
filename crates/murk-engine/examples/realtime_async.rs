//! Murk RealtimeAsyncWorld — background-threaded simulation with concurrent observation.
//!
//! Demonstrates:
//!   1. Creating a RealtimeAsyncWorld with a background tick thread
//!   2. Submitting commands to the tick thread and receiving receipts
//!   3. Observing simulation state while ticks happen in the background
//!   4. Watching the tick_id advance across observations
//!   5. Graceful shutdown with the 4-state shutdown state machine
//!
//! # Lockstep vs. RealtimeAsync
//!
//! In **Lockstep** mode (`LockstepWorld`), the caller drives each tick
//! explicitly via `step_sync()`. The simulation advances exactly one
//! tick per call, making it fully deterministic and easy to test.
//!
//! In **RealtimeAsync** mode (`RealtimeAsyncWorld`), a dedicated background
//! thread advances the simulation at a configured tick rate (e.g. 30 Hz).
//! The caller submits commands asynchronously and reads observations
//! concurrently via an egress worker pool. This is the primary mode for
//! RL training, where the environment must tick independently of the
//! agent's inference latency.
//!
//! Run with:
//!   cargo run --example realtime_async

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use murk_core::{
    BoundaryBehavior, Command, CommandPayload, FieldDef, FieldId, FieldMutability, FieldSet,
    FieldType, PropagatorError, SnapshotAccess, TickId,
};
use murk_engine::{AsyncConfig, BackoffConfig, RealtimeAsyncWorld, WorldConfig};
use murk_obs::{ObsDtype, ObsEntry, ObsPlan, ObsRegion, ObsSpec, ObsTransform};
use murk_propagator::{Propagator, StepContext, WriteMode};
use murk_space::{EdgeBehavior, RegionSpec, Space, Square4};
use smallvec::smallvec;

// ─── Field IDs ──────────────────────────────────────────────────

const HEAT: FieldId = FieldId(0);

// ─── Grid parameters ────────────────────────────────────────────

const ROWS: u32 = 4;
const COLS: u32 = 4;
const DT: f64 = 1.0;
const DIFFUSION: f64 = 0.08;

// Source position (center-ish of grid).
const SOURCE_R: usize = 2;
const SOURCE_C: usize = 2;

// ─── Propagator: discrete Laplacian diffusion ───────────────────
//
// Same Jacobi-style diffusion as quickstart.rs, on a smaller 4x4 grid.
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
        [HEAT].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(HEAT, WriteMode::Full)]
    }

    fn max_dt(&self) -> Option<f64> {
        Some(1.0 / (4.0 * DIFFUSION))
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let prev_heat = ctx.reads_previous().read(HEAT).ok_or_else(|| {
            PropagatorError::ExecutionFailed {
                reason: "heat field not readable".into(),
            }
        })?;
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

                if r == SOURCE_R && c == SOURCE_C {
                    out[idx] = 10.0;
                    continue;
                }

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
    println!("=== Murk RealtimeAsync Example ===\n");

    // 1. Create a space: 4x4 grid, 4-connected, absorb at edges.
    let space = Square4::new(ROWS, COLS, EdgeBehavior::Absorb)?;
    println!(
        "Space: {}x{} Square4, {} cells",
        ROWS, COLS, space.cell_count()
    );

    // 2. Define fields.
    let fields = vec![FieldDef {
        name: "heat".into(),
        field_type: FieldType::Scalar,
        mutability: FieldMutability::PerTick,
        units: Some("kelvin".into()),
        bounds: None,
        boundary_behavior: BoundaryBehavior::Clamp,
    }];

    // 3. Build WorldConfig.
    //
    // KEY DIFFERENCE from lockstep: tick_rate_hz is set.
    // This tells the tick thread how fast to advance the simulation.
    // In lockstep mode, tick_rate_hz is None (caller drives ticks).
    let config = WorldConfig {
        space: Box::new(space),
        fields,
        propagators: vec![Box::new(DiffusionPropagator)],
        dt: DT,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: Some(30.0), // 30 Hz — tick thread sleeps ~33ms between ticks
        backoff: BackoffConfig::default(),
    };

    // 4. Create RealtimeAsyncWorld.
    //
    // This spawns:
    //   - 1 tick thread (runs at tick_rate_hz)
    //   - N egress worker threads (serve observation requests)
    //
    // In lockstep mode, you'd use LockstepWorld::new(config) instead,
    // and drive ticks manually with world.step_sync(commands).
    let async_config = AsyncConfig {
        worker_count: Some(2), // 2 egress workers (small example)
        ..AsyncConfig::default()
    };
    let mut world = RealtimeAsyncWorld::new(config, async_config)?;
    println!("RealtimeAsyncWorld created — tick thread running at 30 Hz");
    println!("  (In lockstep mode, you'd call step_sync() to advance each tick manually)\n");

    // 5. Wait for the first snapshot to appear.
    //
    // The tick thread starts immediately but needs one tick cycle to
    // produce the initial snapshot. We poll latest_snapshot().
    print!("Waiting for first snapshot...");
    loop {
        if world.latest_snapshot().is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    let snap = world.latest_snapshot().unwrap();
    println!(" tick_id={}", snap.tick_id().0);

    // 6. Compile an observation plan.
    //
    // ObsPlan is compiled once and reused for all observations.
    // It maps the ObsSpec (what to observe) + Space (spatial topology)
    // into a pre-computed gather plan with branch-free execution.
    let spec = ObsSpec {
        entries: vec![ObsEntry {
            field_id: HEAT,
            region: ObsRegion::Fixed(RegionSpec::All),
            pool: None,
            transform: ObsTransform::Identity,
            dtype: ObsDtype::F32,
        }],
    };
    let plan_result = ObsPlan::compile(&spec, world.space())?;
    let plan = Arc::new(plan_result.plan);
    let mut output = vec![0.0f32; plan_result.output_len];
    let mut mask = vec![0u8; plan_result.mask_len];

    println!("ObsPlan compiled: {} output elements, {} mask bytes\n", plan_result.output_len, plan_result.mask_len);

    // 7. Observe multiple times to watch the tick_id advance.
    //
    // Unlike lockstep mode where tick_id advances only when you call
    // step_sync(), here the tick thread advances independently. Each
    // observe() call reads the latest snapshot, which may be several
    // ticks ahead of the previous observation.
    println!("Observing simulation state (tick thread is running in background):");
    for i in 0..5 {
        thread::sleep(Duration::from_millis(100)); // Let ~3 ticks accumulate at 30 Hz

        let metadata = world.observe(&plan, &mut output, &mut mask)?;
        let mean_heat: f32 = output.iter().sum::<f32>() / output.len() as f32;
        let max_heat: f32 = output.iter().cloned().fold(0.0_f32, f32::max);

        println!(
            "  observation {}: tick_id={:>3}, coverage={:.1}%, mean_heat={:.4}, max_heat={:.4}",
            i + 1,
            metadata.tick_id.0,
            metadata.coverage * 100.0,
            mean_heat,
            max_heat,
        );
    }

    // 8. Submit commands while the simulation is running.
    //
    // submit_commands() is non-blocking: it sends the command batch
    // to the tick thread via a bounded channel, then blocks briefly
    // for the receipt (which arrives after the next tick processes it).
    //
    // In lockstep mode, commands are passed directly to step_sync().
    println!("\nSubmitting SetField command at (0, 0) — second heat source...");
    let cmd = Command {
        payload: CommandPayload::SetField {
            coord: smallvec![0, 0],
            field_id: HEAT,
            value: 10.0,
        },
        expires_after_tick: TickId(u64::MAX),
        source_id: None,
        source_seq: None,
        priority_class: 1,
        arrival_seq: 0,
    };
    let receipts = world.submit_commands(vec![cmd])?;
    println!(
        "  Receipt: accepted={}, applied at next tick",
        receipts[0].accepted,
    );

    // 9. Observe a few more times to see the command take effect.
    println!("\nObserving after command injection:");
    for i in 0..3 {
        thread::sleep(Duration::from_millis(100));

        let metadata = world.observe(&plan, &mut output, &mut mask)?;
        let mean_heat: f32 = output.iter().sum::<f32>() / output.len() as f32;
        let max_heat: f32 = output.iter().cloned().fold(0.0_f32, f32::max);

        println!(
            "  observation {}: tick_id={:>3}, mean_heat={:.4}, max_heat={:.4}",
            i + 1,
            metadata.tick_id.0,
            mean_heat,
            max_heat,
        );
    }

    // 10. Read the latest snapshot directly (no egress dispatch).
    //
    // latest_snapshot() reads the ring buffer without going through
    // the egress worker pool. Useful for quick state checks.
    if let Some(snap) = world.latest_snapshot() {
        let heat = snap.read_field(HEAT).unwrap();
        println!("\nDirect snapshot read at tick {}:", snap.tick_id().0);
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
    }

    // 11. Graceful shutdown.
    //
    // The shutdown state machine runs through 4 phases:
    //   Running -> Draining (signal tick thread to stop, <=33ms)
    //   Draining -> Quiescing (cancel workers, drop channels, <=200ms)
    //   Quiescing -> Dropped (join all threads, <=10ms)
    //
    // If you don't call shutdown() explicitly, Drop handles it.
    println!("\nShutting down...");
    let report = world.shutdown();
    println!("ShutdownReport:");
    println!("  total_ms:       {}", report.total_ms);
    println!("  drain_ms:       {}", report.drain_ms);
    println!("  quiesce_ms:     {}", report.quiesce_ms);
    println!("  tick_joined:    {}", report.tick_joined);
    println!("  workers_joined: {}", report.workers_joined);

    println!("\nDone.");
    Ok(())
}
