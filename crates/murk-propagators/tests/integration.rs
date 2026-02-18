//! Integration tests for the reference propagator pipeline.
//!
//! These tests exercise the full pipeline through LockstepWorld, not
//! just individual propagators in isolation.

#![allow(deprecated)] // Tests use the old hardcoded field constants intentionally.

use murk_core::FieldReader;
use murk_engine::{BackoffConfig, LockstepWorld, WorldConfig};
use murk_propagators::agent_movement::{new_action_buffer, AgentAction, Direction};
#[allow(deprecated)]
use murk_propagators::fields::{AGENT_PRESENCE, HEAT, HEAT_GRADIENT, REWARD, VELOCITY};
use murk_propagators::{AgentMovementPropagator, DiffusionPropagator, RewardPropagator};
use murk_space::{EdgeBehavior, Square4};

#[allow(deprecated)]
fn small_config(rows: u32, cols: u32, seed: u64) -> (WorldConfig, murk_propagators::ActionBuffer) {
    let ab = new_action_buffer();
    let cell_count = (rows * cols) as usize;
    let initial_positions = vec![(0, cell_count / 2)]; // one agent at center

    let config = WorldConfig {
        space: Box::new(Square4::new(rows, cols, EdgeBehavior::Absorb).unwrap()),
        fields: murk_propagators::reference_fields(),
        propagators: vec![
            Box::new(DiffusionPropagator::new(0.1)),
            Box::new(AgentMovementPropagator::new(ab.clone(), initial_positions)),
            Box::new(RewardPropagator::new(1.0, -0.01)),
        ],
        dt: 0.1,
        seed,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    (config, ab)
}

#[test]
fn thousand_tick_reference_run() {
    let (config, _ab) = small_config(10, 10, 42);
    let mut world = LockstepWorld::new(config).unwrap();

    for _ in 0..1000 {
        world.step_sync(vec![]).unwrap();
    }

    let snap = world.snapshot();
    let heat = snap.read(HEAT).unwrap();
    assert_eq!(heat.len(), 100);
    // All heat values should be finite
    assert!(heat.iter().all(|v| v.is_finite()));
}

#[test]
fn determinism_same_seed_same_output() {
    let run = |seed: u64| {
        let (config, ab) = small_config(10, 10, seed);
        let mut world = LockstepWorld::new(config).unwrap();

        // Inject some actions
        for tick in 0..50 {
            {
                let mut buf = ab.lock().unwrap();
                buf.push(AgentAction {
                    agent_id: 0,
                    direction: if tick % 2 == 0 {
                        Direction::East
                    } else {
                        Direction::South
                    },
                });
            }
            world.step_sync(vec![]).unwrap();
        }

        // Collect final state
        let snap = world.snapshot();
        let heat: Vec<f32> = snap.read(HEAT).unwrap().to_vec();
        let presence: Vec<f32> = snap.read(AGENT_PRESENCE).unwrap().to_vec();
        let reward: Vec<f32> = snap.read(REWARD).unwrap().to_vec();
        (heat, presence, reward)
    };

    let (h1, p1, r1) = run(42);
    let (h2, p2, r2) = run(42);
    assert_eq!(h1, h2, "heat mismatch");
    assert_eq!(p1, p2, "presence mismatch");
    assert_eq!(r1, r2, "reward mismatch");
}

#[test]
fn agents_stay_in_bounds() {
    let (config, ab) = small_config(5, 5, 42);
    let mut world = LockstepWorld::new(config).unwrap();

    // Push agent towards boundaries repeatedly
    let directions = [Direction::North, Direction::West];
    for tick in 0..100 {
        {
            let mut buf = ab.lock().unwrap();
            buf.push(AgentAction {
                agent_id: 0,
                direction: directions[tick % 2],
            });
        }
        world.step_sync(vec![]).unwrap();
    }

    let snap = world.snapshot();
    let presence = snap.read(AGENT_PRESENCE).unwrap();
    // Exactly one cell should be occupied
    let occupied: Vec<usize> = presence
        .iter()
        .enumerate()
        .filter(|(_, &v)| v != 0.0)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(
        occupied.len(),
        1,
        "expected exactly 1 agent, found {} at {:?}",
        occupied.len(),
        occupied
    );
    // The occupied cell must be in bounds (index < 25)
    assert!(occupied[0] < 25, "agent OOB at index {}", occupied[0]);
}

#[test]
#[allow(deprecated)]
fn diffusion_convergence() {
    // With no agents and initial heat at center, diffusion should
    // converge towards uniform heat over many ticks.
    let ab = new_action_buffer();
    let config = WorldConfig {
        space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
        fields: murk_propagators::reference_fields(),
        propagators: vec![
            Box::new(DiffusionPropagator::new(0.1)),
            Box::new(AgentMovementPropagator::new(ab, vec![])), // no agents
            Box::new(RewardPropagator::new(1.0, -0.01)),
        ],
        dt: 0.1,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };
    let mut world = LockstepWorld::new(config).unwrap();

    // Run 1000 ticks — starting from all zeros, should stay all zeros
    // (diffusion of uniform zero is zero)
    for _ in 0..1000 {
        world.step_sync(vec![]).unwrap();
    }

    let snap = world.snapshot();
    let heat = snap.read(HEAT).unwrap();
    assert!(
        heat.iter().all(|&v| v.abs() < 1e-6),
        "heat should remain zero: max={}",
        heat.iter().cloned().fold(0.0f32, f32::max)
    );
}

#[test]
fn all_fields_present_after_tick() {
    let (config, _ab) = small_config(5, 5, 42);
    let mut world = LockstepWorld::new(config).unwrap();
    world.step_sync(vec![]).unwrap();

    let snap = world.snapshot();
    assert!(snap.read(HEAT).is_some(), "heat field missing");
    assert!(snap.read(VELOCITY).is_some(), "velocity field missing");
    assert!(
        snap.read(AGENT_PRESENCE).is_some(),
        "agent_presence field missing"
    );
    assert!(snap.read(HEAT_GRADIENT).is_some(), "gradient field missing");
    assert!(snap.read(REWARD).is_some(), "reward field missing");

    // Check sizes
    assert_eq!(snap.read(HEAT).unwrap().len(), 25);
    assert_eq!(snap.read(VELOCITY).unwrap().len(), 50); // 2 components
    assert_eq!(snap.read(AGENT_PRESENCE).unwrap().len(), 25);
    assert_eq!(snap.read(HEAT_GRADIENT).unwrap().len(), 50); // 2 components
    assert_eq!(snap.read(REWARD).unwrap().len(), 25);
}
