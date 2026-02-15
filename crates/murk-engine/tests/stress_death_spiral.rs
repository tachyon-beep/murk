//! Stress test #15: Death Spiral Resistance
//!
//! Verifies that the RealtimeAsync engine sheds load under overload rather
//! than amplifying it (positive feedback / death spiral).
//!
//! **Workload:** Reference profile (10K cells, 100x100 Square4, 5 fields,
//! 3 propagators, 16 agents) running at 60 Hz.
//!
//! **Injection:**
//! - Ticks 0..100: 16 concurrent observe() callers (normal load)
//! - Ticks 100..600: 32 concurrent observe() callers (2x overload)
//!
//! **Pass criterion:**
//! `overrun_rate(tick 500..600) <= 1.5 * overrun_rate(tick 200..300)`
//!
//! **Fail criterion:**
//! `overrun_rate(tick 500..600) > 2.0 * overrun_rate(tick 200..300)` — death spiral
//!
//! Marked `#[ignore]` because this is a hardware-sensitive stress test that
//! should not run in normal CI.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use murk_core::FieldId;
use murk_engine::{AsyncConfig, BackoffConfig, RealtimeAsyncWorld, WorldConfig};
use murk_obs::spec::{ObsDtype, ObsRegion, ObsTransform};
use murk_obs::{ObsEntry, ObsPlan, ObsSpec};
use murk_propagators::agent_movement::new_action_buffer;
use murk_propagators::{AgentMovementPropagator, DiffusionPropagator, RewardPropagator};
use murk_space::{EdgeBehavior, RegionSpec, Square4};

/// Tick budget at 60 Hz in microseconds.
const TICK_BUDGET_US: u64 = 16_667;

/// Number of ticks for the entire test.
const TOTAL_TICKS: u64 = 600;

/// Tick rate in Hz.
const TICK_RATE_HZ: f64 = 60.0;

/// Duration of the test in seconds (TOTAL_TICKS / TICK_RATE_HZ).
const TEST_DURATION_SECS: f64 = TOTAL_TICKS as f64 / TICK_RATE_HZ;

/// Number of agents in the reference profile for this test.
const NUM_AGENTS: u16 = 16;

/// Normal-load concurrency (ticks 0..100).
const NORMAL_CONCURRENCY: usize = 16;

/// Overload concurrency (ticks 100..600).
const OVERLOAD_CONCURRENCY: usize = 32;

/// Tick at which overload begins.
const OVERLOAD_START_TICK: u64 = 100;

/// Deterministic agent placement that handles large agent counts without overflow.
///
/// Equivalent to `murk_bench::init_agent_positions` but uses wrapping arithmetic
/// to avoid panic in debug builds when `n * multiplier` overflows u64.
fn init_agent_positions_safe(cell_count: usize, n: u16, seed: u64) -> Vec<(u16, usize)> {
    if cell_count == 0 {
        return Vec::new();
    }
    let n = (n as usize).min(cell_count) as u16;
    let mut positions = Vec::with_capacity(n as usize);
    let mut occupied = std::collections::BTreeSet::new();

    for i in 0..n {
        let mut pos = (seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add((i as u64).wrapping_mul(1442695040888963407))
            % cell_count as u64) as usize;

        while occupied.contains(&pos) {
            pos = (pos + 1) % cell_count;
        }
        occupied.insert(pos);
        positions.push((i, pos));
    }
    positions
}

/// Build a WorldConfig matching the stress test spec: 100x100 grid, 16 agents, 60 Hz.
fn death_spiral_config(seed: u64) -> (WorldConfig, murk_propagators::ActionBuffer) {
    let cell_count = 100 * 100;
    let action_buffer = new_action_buffer();
    let initial_positions = init_agent_positions_safe(cell_count, NUM_AGENTS, seed);

    let config = WorldConfig {
        space: Box::new(Square4::new(100, 100, EdgeBehavior::Absorb).unwrap()),
        fields: murk_propagators::reference_fields(),
        propagators: vec![
            Box::new(DiffusionPropagator::new(0.1)),
            Box::new(AgentMovementPropagator::new(
                action_buffer.clone(),
                initial_positions,
            )),
            Box::new(RewardPropagator::new(1.0, -0.01)),
        ],
        dt: 0.1,
        seed,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: Some(TICK_RATE_HZ),
        backoff: BackoffConfig::default(),
    };

    (config, action_buffer)
}

/// Shared counters for measuring observation latency across worker threads.
struct LatencyCounters {
    /// Total observe() calls completed in the baseline window (ticks 200..300).
    baseline_total: AtomicU64,
    /// observe() calls exceeding the tick budget in the baseline window.
    baseline_overruns: AtomicU64,
    /// Total observe() calls completed in the late window (ticks 500..600).
    late_total: AtomicU64,
    /// observe() calls exceeding the tick budget in the late window.
    late_overruns: AtomicU64,
}

impl LatencyCounters {
    fn new() -> Self {
        Self {
            baseline_total: AtomicU64::new(0),
            baseline_overruns: AtomicU64::new(0),
            late_total: AtomicU64::new(0),
            late_overruns: AtomicU64::new(0),
        }
    }
}

/// Classification of the current time window based on elapsed ticks.
enum TimeWindow {
    /// Before baseline measurement (ticks 0..200).
    Early,
    /// Baseline measurement window (ticks 200..300).
    Baseline,
    /// Between windows (ticks 300..500).
    Middle,
    /// Late measurement window (ticks 500..600).
    Late,
    /// After the test (ticks >= 600).
    Done,
}

fn classify_tick(tick: u64) -> TimeWindow {
    match tick {
        0..200 => TimeWindow::Early,
        200..300 => TimeWindow::Baseline,
        300..500 => TimeWindow::Middle,
        500..600 => TimeWindow::Late,
        _ => TimeWindow::Done,
    }
}

#[test]
#[ignore]
fn stress_death_spiral_resistance() {
    // --- Setup ---
    let (config, _action_buffer) = death_spiral_config(42);
    let async_config = AsyncConfig {
        worker_count: Some(8),
        max_epoch_hold_ms: 200,
        cancel_grace_ms: 20,
    };

    let world = RealtimeAsyncWorld::new(config, async_config).unwrap();

    // Wait for the engine to produce at least one snapshot before starting.
    let deadline = Instant::now() + Duration::from_secs(5);
    while world.latest_snapshot().is_none() {
        if Instant::now() > deadline {
            panic!("no snapshot produced within 5s — engine failed to start");
        }
        thread::sleep(Duration::from_millis(10));
    }

    // Compile an observation plan against a single scalar field (heat).
    // This exercises the full egress pipeline: snapshot read, gather, transform.
    let space = world.space();
    let spec = ObsSpec {
        entries: vec![ObsEntry {
            field_id: FieldId(0), // heat
            region: ObsRegion::Fixed(RegionSpec::All),
            pool: None,
            transform: ObsTransform::Identity,
            dtype: ObsDtype::F32,
        }],
    };
    let plan_result = ObsPlan::compile(&spec, space).unwrap();
    let plan = Arc::new(plan_result.plan);
    let output_len = plan_result.output_len;
    let mask_len = plan_result.mask_len;

    // Wrap world in Arc for sharing across observer threads.
    let world = Arc::new(world);
    let counters = Arc::new(LatencyCounters::new());
    let stop_flag = Arc::new(AtomicBool::new(false));

    // Record the start time so we can estimate the current tick from wall time.
    let test_start = Instant::now();

    // --- Spawn observer threads ---
    // Phase 1: start NORMAL_CONCURRENCY observers immediately.
    // Phase 2: at OVERLOAD_START_TICK, start additional observers (total = OVERLOAD_CONCURRENCY).
    let mut observer_handles = Vec::new();

    for observer_id in 0..OVERLOAD_CONCURRENCY {
        let w = Arc::clone(&world);
        let p = Arc::clone(&plan);
        let c = Arc::clone(&counters);
        let sf = Arc::clone(&stop_flag);
        let start = test_start;
        let is_overload_observer = observer_id >= NORMAL_CONCURRENCY;

        let handle = thread::Builder::new()
            .name(format!("observer-{observer_id}"))
            .spawn(move || {
                // Overload observers wait until the overload phase.
                if is_overload_observer {
                    let overload_start_secs = OVERLOAD_START_TICK as f64 / TICK_RATE_HZ;
                    let wait_until = start + Duration::from_secs_f64(overload_start_secs);
                    while Instant::now() < wait_until {
                        if sf.load(Ordering::Relaxed) {
                            return;
                        }
                        thread::sleep(Duration::from_millis(10));
                    }
                }

                let mut output = vec![0.0f32; output_len];
                let mut mask = vec![0u8; mask_len];

                while !sf.load(Ordering::Relaxed) {
                    let call_start = Instant::now();
                    let result = w.observe(&p, &mut output, &mut mask);
                    let call_us = call_start.elapsed().as_micros() as u64;

                    if result.is_err() {
                        // World might be shutting down.
                        break;
                    }

                    // Estimate current tick from wall time.
                    let elapsed_secs = start.elapsed().as_secs_f64();
                    let estimated_tick = (elapsed_secs * TICK_RATE_HZ) as u64;

                    let is_overrun = call_us > TICK_BUDGET_US;

                    match classify_tick(estimated_tick) {
                        TimeWindow::Baseline => {
                            c.baseline_total.fetch_add(1, Ordering::Relaxed);
                            if is_overrun {
                                c.baseline_overruns.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        TimeWindow::Late => {
                            c.late_total.fetch_add(1, Ordering::Relaxed);
                            if is_overrun {
                                c.late_overruns.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        TimeWindow::Done => break,
                        _ => {}
                    }

                    // Small yield to avoid busy-spin when observations are fast.
                    // This simulates realistic RL training cadence where agents
                    // process observations between calls.
                    thread::yield_now();
                }
            })
            .unwrap();

        observer_handles.push(handle);
    }

    // --- Let the test run for its full duration ---
    let test_deadline = test_start + Duration::from_secs_f64(TEST_DURATION_SECS + 1.0);
    while Instant::now() < test_deadline {
        thread::sleep(Duration::from_millis(100));
    }

    // Signal all observers to stop.
    stop_flag.store(true, Ordering::Release);

    // Join all observer threads.
    for handle in observer_handles {
        let _ = handle.join();
    }

    // Shutdown the world.
    // We need to drop the Arc to get ownership; since observers are joined,
    // we should be the only holder.
    let mut world = match Arc::try_unwrap(world) {
        Ok(w) => w,
        Err(_) => panic!("observers should have been joined — Arc still shared"),
    };
    let report = world.shutdown();
    assert!(report.tick_joined, "tick thread should join cleanly");

    // --- Analyze results ---
    let baseline_total = counters.baseline_total.load(Ordering::Relaxed);
    let baseline_overruns = counters.baseline_overruns.load(Ordering::Relaxed);
    let late_total = counters.late_total.load(Ordering::Relaxed);
    let late_overruns = counters.late_overruns.load(Ordering::Relaxed);

    eprintln!("=== Death Spiral Resistance Results ===");
    eprintln!("Baseline window (tick 200..300): {baseline_overruns}/{baseline_total} overruns");
    eprintln!("Late window (tick 500..600): {late_overruns}/{late_total} overruns");

    // Guard against degenerate test runs with too few samples.
    assert!(
        baseline_total >= 10,
        "too few baseline observations ({baseline_total}); test environment may be too slow"
    );
    assert!(
        late_total >= 10,
        "too few late observations ({late_total}); test environment may be too slow"
    );

    // Compute overrun rates.
    let baseline_rate = if baseline_total > 0 {
        baseline_overruns as f64 / baseline_total as f64
    } else {
        0.0
    };
    let late_rate = if late_total > 0 {
        late_overruns as f64 / late_total as f64
    } else {
        0.0
    };

    eprintln!("Baseline overrun rate: {baseline_rate:.4}");
    eprintln!("Late overrun rate:     {late_rate:.4}");

    // If baseline rate is zero or very small, we can't meaningfully compute
    // a ratio. In that case, the system is handling the load fine — pass.
    if baseline_rate < 0.001 {
        eprintln!(
            "Baseline overrun rate is negligible ({baseline_rate:.6}); system is not overloaded."
        );
        eprintln!("Checking absolute late rate instead...");
        // Even under 2x load, if overrun rate stays below 10% absolute, that's fine.
        assert!(
            late_rate < 0.10,
            "late overrun rate {late_rate:.4} exceeds 10% absolute threshold \
             despite negligible baseline — possible death spiral"
        );
        eprintln!("PASS: late overrun rate {late_rate:.4} is within absolute threshold.");
        return;
    }

    let ratio = late_rate / baseline_rate;
    eprintln!("Overrun ratio (late/baseline): {ratio:.4}");

    // FAIL criterion: ratio > 2.0 indicates positive feedback (death spiral).
    assert!(
        ratio <= 2.0,
        "DEATH SPIRAL DETECTED: overrun ratio {ratio:.2} > 2.0 \
         (baseline={baseline_rate:.4}, late={late_rate:.4}). \
         System amplifies load instead of shedding it."
    );

    // PASS criterion: ratio <= 1.5 is healthy load shedding.
    if ratio <= 1.5 {
        eprintln!("PASS: overrun ratio {ratio:.4} <= 1.5 — healthy load shedding.");
    } else {
        eprintln!(
            "WARNING: overrun ratio {ratio:.4} is between 1.5 and 2.0 — \
             marginal. System sheds load but not aggressively."
        );
    }
}

/// Supplementary test: verify that the reference profile with 16 agents
/// validates and the world can be constructed in RealtimeAsync mode.
#[test]
fn death_spiral_config_validates() {
    let (config, _ab) = death_spiral_config(42);
    config.validate().unwrap();
}

#[test]
fn death_spiral_world_starts_and_stops() {
    let (config, _ab) = death_spiral_config(42);
    let async_config = AsyncConfig {
        worker_count: Some(4),
        ..AsyncConfig::default()
    };

    let mut world = RealtimeAsyncWorld::new(config, async_config).unwrap();

    // Wait for at least one snapshot.
    let deadline = Instant::now() + Duration::from_secs(3);
    while world.latest_snapshot().is_none() {
        if Instant::now() > deadline {
            panic!("no snapshot produced within 3s");
        }
        thread::sleep(Duration::from_millis(10));
    }

    let report = world.shutdown();
    assert!(report.tick_joined);
}
