//! End-to-end lockstep RL loop example.
//!
//! Demonstrates: build config → LockstepWorld → inject actions → step → read
//! observations → reset → repeat.

use murk_bench::reference_profile;
use murk_core::FieldReader;
use murk_engine::LockstepWorld;
use murk_propagators::agent_movement::{new_action_buffer, AgentAction, Direction};
use murk_propagators::fields::{HEAT, REWARD};

fn main() {
    println!("=== Murk Lockstep RL Example ===\n");

    let action_buffer = new_action_buffer();
    let config = reference_profile(42, action_buffer.clone());
    let mut world = LockstepWorld::new(config).unwrap();

    // --- Episode 1: random walk ---
    println!("Episode 1: 100 ticks with actions");
    let directions = [
        Direction::North,
        Direction::South,
        Direction::East,
        Direction::West,
        Direction::Stay,
    ];

    for tick in 0..100 {
        // Inject one action per agent per tick (cycling through directions)
        {
            let mut buf = action_buffer.lock().unwrap();
            for agent_id in 0..4u16 {
                buf.push(AgentAction {
                    agent_id,
                    direction: directions[(tick + agent_id as usize) % 5],
                });
            }
        }

        let result = world.step_sync(vec![]).unwrap();

        if tick % 25 == 0 || tick == 99 {
            let snap = &result.snapshot;
            let heat = snap.read(HEAT).unwrap();
            let reward = snap.read(REWARD).unwrap();

            let total_reward: f32 = reward.iter().sum();
            let max_heat: f32 = heat.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let mean_heat: f32 = heat.iter().sum::<f32>() / heat.len() as f32;

            println!(
                "  tick {:>3}: total_reward={:>8.3}, max_heat={:>8.3}, mean_heat={:>8.5}, time={:>6}μs",
                tick + 1,
                total_reward,
                max_heat,
                mean_heat,
                result.metrics.total_us,
            );
        }
    }

    // --- Reset and Episode 2 ---
    println!("\nResetting world...");
    world.reset(99).unwrap();

    println!("Episode 2: 50 ticks without actions (diffusion only)");
    for tick in 0..50 {
        let result = world.step_sync(vec![]).unwrap();

        if tick % 10 == 0 || tick == 49 {
            let snap = &result.snapshot;
            let heat = snap.read(HEAT).unwrap();
            let mean_heat: f32 = heat.iter().sum::<f32>() / heat.len() as f32;
            let reward = snap.read(REWARD).unwrap();
            let total_reward: f32 = reward.iter().sum();

            println!(
                "  tick {:>3}: mean_heat={:>8.5}, total_reward={:>8.3}, time={:>6}μs",
                tick + 1,
                mean_heat,
                total_reward,
                result.metrics.total_us,
            );
        }
    }

    println!("\nFinal tick: {}", world.current_tick().0);
    println!("Done.");
}
