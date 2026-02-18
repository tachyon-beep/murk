//! Benchmark profiles and utilities for the Murk simulation framework.
//!
//! Provides pre-built [`WorldConfig`] profiles for benchmarking and examples:
//!
//! - [`reference_profile`]: 100x100 grid (10K cells) with full propagator pipeline
//! - [`stress_profile`]: 316x316 grid (~100K cells) for stress testing
//! - [`init_agent_positions`]: deterministic agent placement via seed

#![forbid(unsafe_code)]
#![deny(rustdoc::broken_intra_doc_links)]

use murk_engine::{BackoffConfig, WorldConfig};
#[allow(deprecated)]
use murk_propagators::fields::{HEAT, HEAT_GRADIENT, VELOCITY};
use murk_propagators::{
    ActionBuffer, AgentMovementPropagator, GradientCompute, RewardPropagator, ScalarDiffusion,
};
use murk_space::{EdgeBehavior, Square4};

/// Build a reference benchmark profile: 100x100 grid (10K cells).
///
/// Pipeline: ScalarDiffusion(heat, D=0.1) + ScalarDiffusion(velocity, D=0.1)
///   + GradientCompute(heat→gradient) → AgentMovement → Reward(bonus=1.0, cost=-0.01).
/// dt=0.1 (within CFL limit of 1/(4*0.1) = 2.5).
#[allow(deprecated)] // reference_fields() — will be removed when bench defines its own fields
pub fn reference_profile(seed: u64, action_buffer: ActionBuffer) -> WorldConfig {
    let cell_count = 100 * 100;
    let initial_positions = init_agent_positions(cell_count, 4, seed);

    WorldConfig {
        space: Box::new(Square4::new(100, 100, EdgeBehavior::Absorb).unwrap()),
        fields: murk_propagators::reference_fields(),
        propagators: vec![
            Box::new(
                ScalarDiffusion::builder()
                    .input_field(HEAT)
                    .output_field(HEAT)
                    .coefficient(0.1)
                    .build()
                    .unwrap(),
            ),
            Box::new(
                ScalarDiffusion::builder()
                    .input_field(VELOCITY)
                    .output_field(VELOCITY)
                    .coefficient(0.1)
                    .build()
                    .unwrap(),
            ),
            Box::new(
                GradientCompute::builder()
                    .input_field(HEAT)
                    .output_field(HEAT_GRADIENT)
                    .build()
                    .unwrap(),
            ),
            Box::new(AgentMovementPropagator::new(
                action_buffer,
                initial_positions,
            )),
            Box::new(RewardPropagator::new(1.0, -0.01)),
        ],
        dt: 0.1,
        seed,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    }
}

/// Build a stress benchmark profile: 316x316 grid (~100K cells).
///
/// Same pipeline as [`reference_profile`] but at 10x the cell count.
#[allow(deprecated)] // reference_fields() — will be removed when bench defines its own fields
pub fn stress_profile(seed: u64, action_buffer: ActionBuffer) -> WorldConfig {
    let cell_count = 316 * 316;
    let initial_positions = init_agent_positions(cell_count, 4, seed);

    WorldConfig {
        space: Box::new(Square4::new(316, 316, EdgeBehavior::Absorb).unwrap()),
        fields: murk_propagators::reference_fields(),
        propagators: vec![
            Box::new(
                ScalarDiffusion::builder()
                    .input_field(HEAT)
                    .output_field(HEAT)
                    .coefficient(0.1)
                    .build()
                    .unwrap(),
            ),
            Box::new(
                ScalarDiffusion::builder()
                    .input_field(VELOCITY)
                    .output_field(VELOCITY)
                    .coefficient(0.1)
                    .build()
                    .unwrap(),
            ),
            Box::new(
                GradientCompute::builder()
                    .input_field(HEAT)
                    .output_field(HEAT_GRADIENT)
                    .build()
                    .unwrap(),
            ),
            Box::new(AgentMovementPropagator::new(
                action_buffer,
                initial_positions,
            )),
            Box::new(RewardPropagator::new(1.0, -0.01)),
        ],
        dt: 0.1,
        seed,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    }
}

/// Generate deterministic initial agent positions.
///
/// Places `n` agents at evenly-spaced positions in the grid using a
/// simple hash of the seed. Returns `(agent_id, flat_index)` pairs.
///
/// If `cell_count` is 0 or `n` exceeds `cell_count`, the result is
/// clamped to at most `cell_count` agents (no panic, no infinite loop).
pub fn init_agent_positions(cell_count: usize, n: u16, seed: u64) -> Vec<(u16, usize)> {
    if cell_count == 0 {
        return Vec::new();
    }

    // Cannot place more agents than cells
    let n = (n as usize).min(cell_count) as u16;

    let mut positions = Vec::with_capacity(n as usize);
    let mut occupied = std::collections::HashSet::new();

    for i in 0..n {
        // Simple deterministic placement: spread agents across the grid
        let mut pos = ((seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(i as u64 * 1442695040888963407))
            % cell_count as u64) as usize;

        // Linear probe to avoid collisions (guaranteed to terminate: n <= cell_count)
        while occupied.contains(&pos) {
            pos = (pos + 1) % cell_count;
        }
        occupied.insert(pos);
        positions.push((i, pos));
    }

    positions
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_propagators::agent_movement::new_action_buffer;

    #[test]
    fn reference_profile_validates() {
        let ab = new_action_buffer();
        let config = reference_profile(42, ab);
        config.validate().unwrap();
    }

    #[test]
    fn stress_profile_validates() {
        let ab = new_action_buffer();
        let config = stress_profile(42, ab);
        config.validate().unwrap();
    }

    #[test]
    fn init_agent_positions_no_collisions() {
        let positions = init_agent_positions(100, 10, 42);
        assert_eq!(positions.len(), 10);

        let indices: Vec<usize> = positions.iter().map(|&(_, idx)| idx).collect();
        let unique: std::collections::HashSet<usize> = indices.iter().copied().collect();
        assert_eq!(unique.len(), 10, "all positions should be unique");

        for &(_, idx) in &positions {
            assert!(idx < 100, "position {idx} out of bounds");
        }
    }

    #[test]
    fn init_agent_positions_deterministic() {
        let a = init_agent_positions(1000, 5, 42);
        let b = init_agent_positions(1000, 5, 42);
        assert_eq!(a, b);
    }

    #[test]
    fn init_agent_positions_zero_cells_returns_empty() {
        let positions = init_agent_positions(0, 5, 42);
        assert!(positions.is_empty());
    }

    #[test]
    fn init_agent_positions_more_agents_than_cells_clamps() {
        let positions = init_agent_positions(3, 10, 42);
        assert_eq!(positions.len(), 3);

        let indices: Vec<usize> = positions.iter().map(|&(_, idx)| idx).collect();
        let unique: std::collections::HashSet<usize> = indices.iter().copied().collect();
        assert_eq!(unique.len(), 3, "all positions should be unique");
    }

    #[test]
    fn init_agent_positions_exactly_fills_grid() {
        let positions = init_agent_positions(4, 4, 99);
        assert_eq!(positions.len(), 4);

        let indices: Vec<usize> = positions.iter().map(|&(_, idx)| idx).collect();
        let unique: std::collections::HashSet<usize> = indices.iter().copied().collect();
        assert_eq!(unique.len(), 4);
    }
}
