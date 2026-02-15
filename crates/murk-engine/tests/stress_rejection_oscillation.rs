//! Stress Test #17: Rejection Oscillation Stability
//!
//! Verifies that the adaptive backoff mechanism stabilizes rejection rates
//! rather than oscillating under sustained overload. Simulates 50 agents
//! submitting commands at 2x the tick rate (120Hz submission into 60Hz ticks)
//! for 600 ticks, then measures the coefficient of variation of per-window
//! rejection rates.
//!
//! Pass criterion: CV < 0.3 across 10 one-second (60-tick) windows.

use murk_bench::reference_profile;
use murk_core::command::{Command, CommandPayload};
use murk_core::id::{ParameterKey, TickId};
use murk_engine::{BackoffConfig, LockstepWorld};
use murk_propagators::agent_movement::new_action_buffer;

const NUM_AGENTS: u64 = 50;
const TOTAL_TICKS: u64 = 600;
const WINDOW_SIZE: u64 = 60;
const NUM_WINDOWS: usize = (TOTAL_TICKS / WINDOW_SIZE) as usize; // 10
const COMMANDS_PER_TICK: u64 = NUM_AGENTS * 2; // 2x submission rate
const MAX_CV: f64 = 0.3;

/// Build a command for a given agent at a given tick.
///
/// Uses a tight TTL (`current_tick + 2`) so that commands which sit in
/// the queue across tick boundaries are likely to expire, creating a
/// steady stream of stale rejections that the adaptive backoff must
/// stabilize.
fn make_agent_cmd(agent_id: u64, current_tick: TickId, seq: u64) -> Command {
    Command {
        payload: CommandPayload::SetParameter {
            key: ParameterKey(0),
            value: 0.0,
        },
        expires_after_tick: TickId(current_tick.0 + 2),
        source_id: Some(agent_id),
        source_seq: Some(seq),
        priority_class: 1,
        arrival_seq: 0,
    }
}

/// Count rejected receipts from a `StepResult`.
///
/// A receipt is "rejected" if it was not applied. This covers both
/// submission-time rejections (QueueFull) and TTL expirations (Stale).
fn count_rejections(receipts: &[murk_core::command::Receipt]) -> u64 {
    receipts
        .iter()
        .filter(|r| r.applied_tick_id.is_none())
        .count() as u64
}

#[test]
#[ignore] // stress test — run with `cargo test --release -- --ignored`
fn stress_rejection_oscillation_stability() {
    // Build a reference profile (100x100 grid) with a constrained ingress
    // queue. The queue holds fewer commands than we submit per tick,
    // guaranteeing a baseline rejection rate from QueueFull.
    let action_buffer = new_action_buffer();
    let mut config = reference_profile(42, action_buffer);

    // Constrain the ingress queue to force rejections.
    // We submit 100 commands per tick; a queue of 64 means ~36 are
    // rejected per tick at the QueueFull boundary.
    config.max_ingress_queue = 64;

    // Configure adaptive backoff to be active during the test.
    config.backoff = BackoffConfig {
        initial_max_skew: 2,
        backoff_factor: 1.5,
        max_skew_cap: 10,
        decay_rate: 60,
        rejection_rate_threshold: 0.20,
    };

    let mut world = LockstepWorld::new(config).expect("failed to create world");

    // Per-window rejection counts.
    let mut window_rejections: Vec<u64> = Vec::with_capacity(NUM_WINDOWS);
    let mut current_window_rejections: u64 = 0;

    // Track per-agent sequence numbers.
    let mut agent_seqs = vec![0u64; NUM_AGENTS as usize];

    for tick in 0..TOTAL_TICKS {
        let current_tick = world.current_tick();

        // Generate two batches of 50 commands (simulating 120Hz into 60Hz).
        let mut commands = Vec::with_capacity(COMMANDS_PER_TICK as usize);

        // Batch 1: 50 agents, each sending 1 command.
        for agent_id in 0..NUM_AGENTS {
            let seq = agent_seqs[agent_id as usize];
            agent_seqs[agent_id as usize] += 1;
            commands.push(make_agent_cmd(agent_id, current_tick, seq));
        }

        // Batch 2: same 50 agents, second command (the "extra" 120Hz batch).
        for agent_id in 0..NUM_AGENTS {
            let seq = agent_seqs[agent_id as usize];
            agent_seqs[agent_id as usize] += 1;
            commands.push(make_agent_cmd(agent_id, current_tick, seq));
        }

        assert_eq!(commands.len(), COMMANDS_PER_TICK as usize);

        // Step the world. In lockstep mode this submits and executes atomically.
        let result = world.step_sync(commands).expect("step_sync failed");

        let rejections = count_rejections(&result.receipts);
        current_window_rejections += rejections;

        // End of window?
        if (tick + 1) % WINDOW_SIZE == 0 {
            window_rejections.push(current_window_rejections);
            current_window_rejections = 0;
        }
    }

    assert_eq!(
        window_rejections.len(),
        NUM_WINDOWS,
        "expected {NUM_WINDOWS} windows, got {}",
        window_rejections.len()
    );

    // Compute rejection rate per window.
    let commands_per_window = (COMMANDS_PER_TICK * WINDOW_SIZE) as f64;
    let rates: Vec<f64> = window_rejections
        .iter()
        .map(|&r| r as f64 / commands_per_window)
        .collect();

    // Compute mean rejection rate.
    let mean: f64 = rates.iter().sum::<f64>() / rates.len() as f64;

    // Compute standard deviation.
    let variance: f64 = rates.iter().map(|&r| (r - mean).powi(2)).sum::<f64>() / rates.len() as f64;
    let stddev = variance.sqrt();

    // Coefficient of variation.
    let cv = if mean > 0.0 { stddev / mean } else { 0.0 };

    // Diagnostic output.
    eprintln!("=== Stress Test #17: Rejection Oscillation Stability ===");
    eprintln!("Total ticks: {TOTAL_TICKS}");
    eprintln!(
        "Commands per tick: {COMMANDS_PER_TICK} ({}agents x 2 batches)",
        NUM_AGENTS
    );
    eprintln!("Window size: {WINDOW_SIZE} ticks ({NUM_WINDOWS} windows)");
    eprintln!();
    for (i, rate) in rates.iter().enumerate() {
        eprintln!(
            "  Window {:>2}: rejection_rate = {:.4} ({} / {})",
            i, rate, window_rejections[i], commands_per_window as u64
        );
    }
    eprintln!();
    eprintln!("Mean rejection rate: {mean:.4}");
    eprintln!("Stddev:              {stddev:.4}");
    eprintln!("CV (stddev/mean):    {cv:.4}");
    eprintln!("Threshold:           {MAX_CV:.4}");
    eprintln!(
        "Result:              {}",
        if cv < MAX_CV { "PASS" } else { "FAIL" }
    );

    // Assert: CV must be below threshold. A high CV indicates the adaptive
    // backoff is oscillating instead of stabilizing the rejection rate.
    assert!(
        cv < MAX_CV,
        "Rejection oscillation detected: CV = {cv:.4} >= {MAX_CV} \
         (mean = {mean:.4}, stddev = {stddev:.4}). \
         The adaptive backoff mechanism is oscillating instead of stabilizing."
    );

    // Sanity check: there should be *some* rejections, otherwise the test
    // isn't exercising the rejection path at all.
    let total_rejections: u64 = window_rejections.iter().sum();
    assert!(
        total_rejections > 0,
        "No rejections observed across {TOTAL_TICKS} ticks with {COMMANDS_PER_TICK} \
         commands/tick — the ingress queue constraint ({}) may be too large",
        64
    );

    // Sanity check: rejection rate should be non-trivial (at least 10%).
    assert!(
        mean > 0.10,
        "Mean rejection rate {mean:.4} is too low to meaningfully test oscillation stability"
    );
}
