//! Integration tests for P4 propagators through the full LockstepWorld engine.
//!
//! Tests composition patterns: emission->diffusion->flow pipelines,
//! resource depletion/regrowth, wave stability, noise determinism,
//! and morphological mask computation.

use murk_core::{BoundaryBehavior, FieldDef, FieldId, FieldMutability, FieldReader, FieldType};
use murk_engine::{BackoffConfig, LockstepWorld, WorldConfig};
use murk_propagators::{
    AgentEmission, EmissionMode, FlowField, IdentityCopy, MorphOp, MorphologicalOp,
    NoiseInjection, NoiseType, RegrowthModel, ResourceField, ScalarDiffusion, WavePropagation,
};
use murk_space::{EdgeBehavior, Square4};

// ---------- Field IDs ----------

const PRESENCE: FieldId = FieldId(0);
const EMISSION: FieldId = FieldId(1);
const HEAT: FieldId = FieldId(2);
const FLOW: FieldId = FieldId(3);

// ---------- Helpers ----------

fn scalar_field(name: &str) -> FieldDef {
    FieldDef {
        name: name.to_string(),
        field_type: FieldType::Scalar,
        mutability: FieldMutability::PerTick,
        units: None,
        bounds: None,
        boundary_behavior: BoundaryBehavior::Clamp,
    }
}

fn vector2_field(name: &str) -> FieldDef {
    FieldDef {
        name: name.to_string(),
        field_type: FieldType::Vector { dims: 2 },
        mutability: FieldMutability::PerTick,
        units: None,
        bounds: None,
        boundary_behavior: BoundaryBehavior::Clamp,
    }
}

// ---------- Test 1: Emission -> Diffusion -> Flow pipeline ----------

/// Compose AgentEmission, ScalarDiffusion, and FlowField in a 3-stage
/// pheromone-trail pipeline. Verify all fields remain finite.
#[test]
fn emission_diffusion_flow_pipeline() {
    let config = WorldConfig {
        space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
        fields: vec![
            scalar_field("presence"),  // 0 = PRESENCE
            scalar_field("emission"),  // 1 = EMISSION
            scalar_field("heat"),      // 2 = HEAT
            vector2_field("flow"),     // 3 = FLOW
        ],
        propagators: vec![
            Box::new(IdentityCopy::new(PRESENCE)),
            Box::new(
                AgentEmission::builder()
                    .presence_field(PRESENCE)
                    .emission_field(EMISSION)
                    .intensity(10.0)
                    .mode(EmissionMode::Set)
                    .build()
                    .unwrap(),
            ),
            Box::new(
                ScalarDiffusion::builder()
                    .input_field(EMISSION)
                    .output_field(HEAT)
                    .coefficient(0.1)
                    .build()
                    .unwrap(),
            ),
            Box::new(
                FlowField::builder()
                    .potential_field(HEAT)
                    .flow_field(FLOW)
                    .normalize(false)
                    .build()
                    .unwrap(),
            ),
        ],
        dt: 0.1,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    let mut world = LockstepWorld::new(config).unwrap();

    for _ in 0..50 {
        world.step_sync(vec![]).unwrap();
    }

    let snap = world.snapshot();
    let heat = snap.read(HEAT).unwrap();
    assert!(heat.iter().all(|v| v.is_finite()), "heat has NaN/Inf");

    let flow = snap.read(FLOW).unwrap();
    assert!(flow.iter().all(|v| v.is_finite()), "flow has NaN/Inf");
}

// ---------- Test 2: Resource depletion and regrowth ----------

#[test]
fn resource_consumption_and_regrowth() {
    let config = WorldConfig {
        space: Box::new(Square4::new(5, 5, EdgeBehavior::Absorb).unwrap()),
        fields: vec![
            scalar_field("presence"),  // 0
            scalar_field("resource"),  // 1
        ],
        propagators: vec![
            Box::new(IdentityCopy::new(PRESENCE)),
            Box::new(
                ResourceField::builder()
                    .field(FieldId(1))
                    .presence_field(PRESENCE)
                    .consumption_rate(0.5)
                    .regrowth_rate(0.01)
                    .capacity(1.0)
                    .regrowth_model(RegrowthModel::Linear)
                    .build()
                    .unwrap(),
            ),
        ],
        dt: 0.1,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    let mut world = LockstepWorld::new(config).unwrap();

    for _ in 0..100 {
        world.step_sync(vec![]).unwrap();
    }

    let snap = world.snapshot();
    let resource = snap.read(FieldId(1)).unwrap();

    for &v in resource {
        assert!(v >= 0.0, "resource should be non-negative, got {v}");
        assert!(v <= 1.0, "resource should be <= capacity, got {v}");
    }
}

// ---------- Test 3: Wave stability ----------

#[test]
fn wave_stability_500_ticks() {
    let config = WorldConfig {
        space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
        fields: vec![
            scalar_field("displacement"),  // 0
            scalar_field("velocity"),      // 1
        ],
        propagators: vec![Box::new(
            WavePropagation::builder()
                .displacement_field(FieldId(0))
                .velocity_field(FieldId(1))
                .wave_speed(1.0)
                .damping(0.01)
                .build()
                .unwrap(),
        )],
        dt: 0.05,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    let mut world = LockstepWorld::new(config).unwrap();

    for tick in 1..=500u64 {
        world.step_sync(vec![]).unwrap();

        let snap = world.snapshot();
        let disp = snap.read(FieldId(0)).unwrap();
        let vel = snap.read(FieldId(1)).unwrap();

        for &v in disp.iter().chain(vel.iter()) {
            assert!(
                v.is_finite(),
                "NaN/Inf in wave fields at tick {tick}: {v}"
            );
        }
    }
}

// ---------- Test 4: Noise determinism ----------

#[test]
fn noise_determinism() {
    let make_config = |seed: u64| WorldConfig {
        space: Box::new(Square4::new(5, 5, EdgeBehavior::Absorb).unwrap()),
        fields: vec![scalar_field("noisy")],
        propagators: vec![Box::new(
            NoiseInjection::builder()
                .field(FieldId(0))
                .noise_type(NoiseType::Gaussian)
                .scale(0.5)
                .seed_offset(seed)
                .build()
                .unwrap(),
        )],
        dt: 0.1,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    let run = |seed: u64| -> Vec<f32> {
        let mut world = LockstepWorld::new(make_config(seed)).unwrap();
        for _ in 0..100 {
            world.step_sync(vec![]).unwrap();
        }
        let snap = world.snapshot();
        snap.read(FieldId(0)).unwrap().to_vec()
    };

    let a = run(99);
    let b = run(99);
    assert_eq!(a, b, "same seed -> bit-identical noise output");
}

// ---------- Test 5: Morphological dilate/erode ----------

#[test]
fn morphological_dilate_through_engine() {
    let config = WorldConfig {
        space: Box::new(Square4::new(5, 5, EdgeBehavior::Absorb).unwrap()),
        fields: vec![
            scalar_field("mask_in"),   // 0
            scalar_field("mask_out"),  // 1
        ],
        propagators: vec![
            Box::new(IdentityCopy::new(FieldId(0))),
            Box::new(
                MorphologicalOp::builder()
                    .input_field(FieldId(0))
                    .output_field(FieldId(1))
                    .op(MorphOp::Dilate)
                    .radius(1)
                    .threshold(0.5)
                    .build()
                    .unwrap(),
            ),
        ],
        dt: 0.1,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    let mut world = LockstepWorld::new(config).unwrap();

    world.step_sync(vec![]).unwrap();

    let snap = world.snapshot();
    let mask_out = snap.read(FieldId(1)).unwrap();
    assert!(
        mask_out.iter().all(|&v| v == 0.0),
        "dilate of all-zero should be all-zero"
    );
}
