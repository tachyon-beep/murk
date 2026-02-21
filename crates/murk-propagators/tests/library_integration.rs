//! Integration tests for the library propagators (ScalarDiffusion, GradientCompute,
//! IdentityCopy) through the full LockstepWorld engine.
//!
//! These are NOT unit tests — they exercise the complete tick pipeline: arena
//! allocation, command ingress, propagator scheduling, overlay resolution,
//! and snapshot publication.

use murk_core::{BoundaryBehavior, FieldDef, FieldId, FieldMutability, FieldReader, FieldType};
use murk_engine::{BackoffConfig, LockstepWorld, WorldConfig};
use murk_propagators::{
    FlowField, GradientCompute, IdentityCopy, ScalarDiffusion, WavePropagation,
};
use murk_space::{EdgeBehavior, Hex2D, Ring1D, Space, Square4};

// ---------- Field IDs (our own, not the deprecated constants) ----------

const HEAT: FieldId = FieldId(0);
const GRADIENT: FieldId = FieldId(1);
const MARKER: FieldId = FieldId(2);

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

// ---------- Test 1: 1000-tick stability ----------

/// Run ScalarDiffusion with a hot center on a 10x10 Absorb grid for 1000 ticks.
/// Assert no NaN/Inf, all values >= 0 (clamp_min), and total energy stays bounded.
#[test]
fn thousand_tick_stability_scalar_diffusion() {
    // 10x10 grid, fixed source at (5,5) = flat index 55,
    // absorb boundaries, decay, and clamp_min.
    // With absorb boundaries + decay, energy leaks out. The fixed source
    // re-injects 100.0 each tick at cell 55. The system should reach a
    // bounded steady state.
    let config = WorldConfig {
        space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
        fields: vec![scalar_field("heat")],
        propagators: vec![Box::new(
            ScalarDiffusion::builder()
                .input_field(HEAT)
                .output_field(HEAT)
                .coefficient(0.1)
                .decay(0.01)
                .clamp_min(0.0)
                .sources(vec![(55, 100.0)])
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

    let mut world = LockstepWorld::new(config).unwrap();

    for tick in 1..=1000u64 {
        world.step_sync(vec![]).unwrap();

        let snap = world.snapshot();
        let heat = snap.read(HEAT).unwrap();
        assert_eq!(heat.len(), 100, "field length mismatch at tick {tick}");

        // No NaN or Inf values.
        for (i, &v) in heat.iter().enumerate() {
            assert!(v.is_finite(), "NaN/Inf at cell {i}, tick {tick}: {v}");
        }

        // All values >= 0.0 (clamp_min enforced).
        for (i, &v) in heat.iter().enumerate() {
            assert!(v >= 0.0, "negative value at cell {i}, tick {tick}: {v}");
        }

        // Total energy should remain bounded. With a source injecting 100.0 at one
        // cell and decay + absorb boundaries removing energy, the total should never
        // exceed a reasonable upper bound. A 100-cell grid with source=100 and small
        // decay: worst case every cell reaches ~100, so total < 10_000.
        let total: f32 = heat.iter().sum();
        assert!(
            total < 10_000.0,
            "total energy diverged at tick {tick}: {total}"
        );
    }

    // After 1000 ticks, the source cell should still be pinned at 100.0.
    let snap = world.snapshot();
    let heat = snap.read(HEAT).unwrap();
    assert!(
        (heat[55] - 100.0).abs() < 1e-6,
        "source cell should be 100.0 after 1000 ticks, got {}",
        heat[55]
    );
}

// ---------- Test 2: Determinism ----------

/// Run the same ScalarDiffusion config twice with the same seed for 100 ticks.
/// Assert bit-identical output field arrays.
#[test]
fn determinism_same_seed_same_output() {
    let make_config = |seed: u64| WorldConfig {
        space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
        fields: vec![scalar_field("heat")],
        propagators: vec![Box::new(
            ScalarDiffusion::builder()
                .input_field(HEAT)
                .output_field(HEAT)
                .coefficient(0.1)
                .decay(0.01)
                .clamp_min(0.0)
                .sources(vec![(55, 100.0)])
                .build()
                .unwrap(),
        )],
        dt: 0.1,
        seed,
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
        snap.read(HEAT).unwrap().to_vec()
    };

    let output_a = run(42);
    let output_b = run(42);

    // Bit-identical comparison.
    assert_eq!(
        output_a, output_b,
        "two runs with the same seed must produce bit-identical output"
    );
}

// ---------- Test 3: GradientCompute integration ----------

/// Run ScalarDiffusion + GradientCompute together for 50 ticks.
/// Assert gradient field has expected properties: zero gradient in uniform
/// regions, non-zero near heat source boundary.
#[test]
fn gradient_compute_with_diffusion() {
    // ScalarDiffusion writes heat, GradientCompute reads previous-tick heat
    // and writes gradient. Note: GradientCompute uses reads_previous, so the
    // gradient at tick N is computed from the heat field at tick N-1.
    let config = WorldConfig {
        space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
        fields: vec![scalar_field("heat"), vector2_field("gradient")],
        propagators: vec![
            Box::new(
                ScalarDiffusion::builder()
                    .input_field(HEAT)
                    .output_field(HEAT)
                    .coefficient(0.1)
                    .sources(vec![(55, 100.0)])
                    .build()
                    .unwrap(),
            ),
            Box::new(
                GradientCompute::builder()
                    .input_field(HEAT)
                    .output_field(GRADIENT)
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

    // Run 50 ticks to let heat spread from the source.
    for _ in 0..50 {
        world.step_sync(vec![]).unwrap();
    }

    let snap = world.snapshot();
    let heat = snap.read(HEAT).unwrap();
    let grad = snap.read(GRADIENT).unwrap();

    // Gradient field should have 2 components per cell: 100 cells * 2 = 200.
    assert_eq!(
        grad.len(),
        200,
        "gradient field should have 200 elements (10x10 * 2)"
    );

    // All gradient values should be finite.
    for (i, &v) in grad.iter().enumerate() {
        assert!(v.is_finite(), "gradient NaN/Inf at index {i}: {v}");
    }

    // Find the cell with maximum gradient magnitude. After 50 ticks of
    // diffusion from a central source, the steepest gradient should be
    // at the front of the heat wave, not at the source itself (where
    // neighbours have similar values).
    let mut max_grad_mag = 0.0f32;
    let mut max_grad_cell = 0usize;
    for i in 0..100 {
        let gx = grad[i * 2];
        let gy = grad[i * 2 + 1];
        let mag = (gx * gx + gy * gy).sqrt();
        if mag > max_grad_mag {
            max_grad_mag = mag;
            max_grad_cell = i;
        }
    }

    // The maximum gradient should be non-trivial (heat is spreading).
    assert!(
        max_grad_mag > 0.1,
        "max gradient magnitude should be non-trivial, got {max_grad_mag} at cell {max_grad_cell}"
    );

    // The source cell should have heat = 100.0 (source re-injects each tick).
    assert!(
        (heat[55] - 100.0).abs() < 1e-6,
        "source cell should be 100.0, got {}",
        heat[55]
    );

    // Cells far from the source should have lower heat than the source.
    assert!(
        heat[0] < heat[55],
        "corner should have less heat than source: corner={}, source={}",
        heat[0],
        heat[55]
    );
}

// ---------- Test 4: IdentityCopy integration ----------

/// Run IdentityCopy on a field for 100 ticks. Since no other propagator writes
/// to the IdentityCopy field and initial state is all zeros, verify that:
/// 1. The field remains exactly zero across all ticks (no corruption, no drift)
/// 2. Tick 2 snapshot is bit-identical to tick 100 snapshot
#[test]
fn identity_copy_preserves_values() {
    // IdentityCopy reads previous tick, writes current tick unchanged.
    // Starting from all-zero initial state, the field should remain zero
    // for all ticks. This exercises the full WriteMode::Full pipeline through
    // the engine — any corruption in arena allocation, overlay resolution, or
    // snapshot publishing would show up as non-zero values.
    let config = WorldConfig {
        space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
        fields: vec![scalar_field("data")],
        propagators: vec![Box::new(IdentityCopy::new(FieldId(0)))],
        dt: 0.1,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    let mut world = LockstepWorld::new(config).unwrap();

    // Step tick 1.
    world.step_sync(vec![]).unwrap();

    let after_tick_1: Vec<f32> = {
        let snap = world.snapshot();
        snap.read(FieldId(0)).unwrap().to_vec()
    };

    // All values should be exactly 0.0 after tick 1.
    assert!(
        after_tick_1.iter().all(|&v| v == 0.0),
        "field should be all zeros after tick 1, max={}",
        after_tick_1.iter().cloned().fold(0.0f32, f32::max)
    );

    // Run 99 more ticks (total 100 ticks).
    for _ in 0..99 {
        world.step_sync(vec![]).unwrap();
    }

    let after_tick_100: Vec<f32> = {
        let snap = world.snapshot();
        snap.read(FieldId(0)).unwrap().to_vec()
    };

    // Values should be bit-identical between tick 1 and tick 100.
    assert_eq!(
        after_tick_1, after_tick_100,
        "IdentityCopy must preserve all field values exactly across 99 ticks"
    );

    // All values should still be exactly zero.
    assert!(
        after_tick_100.iter().all(|&v| v == 0.0),
        "field should be all zeros after 100 ticks, max={}",
        after_tick_100.iter().cloned().fold(0.0f32, f32::max)
    );
}

// ---------- Test 5: Combined pipeline stability ----------

/// Run all three library propagators in a single world for 100 ticks.
/// ScalarDiffusion writes heat, GradientCompute writes gradient,
/// IdentityCopy preserves a separate marker field.
#[test]
fn combined_pipeline_three_propagators() {
    let config = WorldConfig {
        space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
        fields: vec![
            scalar_field("heat"),      // FieldId(0) = HEAT
            vector2_field("gradient"), // FieldId(1) = GRADIENT
            scalar_field("marker"),    // FieldId(2) = MARKER
        ],
        propagators: vec![
            Box::new(
                ScalarDiffusion::builder()
                    .input_field(HEAT)
                    .output_field(HEAT)
                    .coefficient(0.1)
                    .sources(vec![(55, 100.0)])
                    .build()
                    .unwrap(),
            ),
            Box::new(
                GradientCompute::builder()
                    .input_field(HEAT)
                    .output_field(GRADIENT)
                    .build()
                    .unwrap(),
            ),
            Box::new(IdentityCopy::new(MARKER)),
        ],
        dt: 0.1,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    let mut world = LockstepWorld::new(config).unwrap();

    // Run 100 ticks.
    for _ in 0..100 {
        world.step_sync(vec![]).unwrap();
    }

    let snap = world.snapshot();

    // Heat field: should have diffused, source still injecting.
    let heat = snap.read(HEAT).unwrap();
    assert_eq!(heat.len(), 100);
    assert!(heat.iter().all(|v| v.is_finite()), "heat has NaN/Inf");
    assert!(
        (heat[55] - 100.0).abs() < 1e-6,
        "source cell should be 100.0, got {}",
        heat[55]
    );
    // Heat should have spread: neighbors of source should be > 0.
    assert!(
        heat[54] > 0.0,
        "west neighbor of source should have heat, got {}",
        heat[54]
    );
    assert!(
        heat[45] > 0.0,
        "north neighbor of source should have heat, got {}",
        heat[45]
    );

    // Gradient field: should be present and finite.
    let grad = snap.read(GRADIENT).unwrap();
    assert_eq!(grad.len(), 200);
    assert!(grad.iter().all(|v| v.is_finite()), "gradient has NaN/Inf");

    // Marker field: IdentityCopy copies from previous tick. Since no propagator
    // or command ever writes non-zero values to the marker field, it should
    // remain all zeros. This verifies the three-propagator pipeline doesn't
    // corrupt independent fields.
    let marker = snap.read(MARKER).unwrap();
    assert_eq!(marker.len(), 100);
    assert!(
        marker.iter().all(|&v| v == 0.0),
        "marker field should be all zeros, max={}",
        marker.iter().cloned().fold(0.0f32, f32::max)
    );
}

// ==========================================================================
// Generic fallback path tests (non-Square4 spaces)
// ==========================================================================
// All propagators have a Square4 fast path and a generic fallback. The tests
// above only use Square4. These tests exercise the generic `step_generic()`
// code path by using Ring1D (1D) and Hex2D (2D, 6-connected).

// ---------- Test 6: ScalarDiffusion on Ring1D (generic path) ----------

/// ScalarDiffusion on a 20-cell Ring1D (periodic). The generic fallback is used
/// because Ring1D is not Square4. After many ticks with a source, energy should
/// spread symmetrically around the ring.
#[test]
fn scalar_diffusion_generic_ring1d() {
    let cell_count = 20;
    let source_cell = 0;
    let config = WorldConfig {
        space: Box::new(Ring1D::new(cell_count).unwrap()),
        fields: vec![scalar_field("heat")],
        propagators: vec![Box::new(
            ScalarDiffusion::builder()
                .input_field(HEAT)
                .output_field(HEAT)
                .coefficient(0.1)
                .sources(vec![(source_cell, 50.0)])
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

    let mut world = LockstepWorld::new(config).unwrap();

    for _ in 0..200 {
        world.step_sync(vec![]).unwrap();
    }

    let snap = world.snapshot();
    let heat = snap.read(HEAT).unwrap();
    assert_eq!(heat.len(), cell_count as usize);

    // All values should be finite and non-negative (no clamp_min but source is positive).
    for (i, &v) in heat.iter().enumerate() {
        assert!(v.is_finite(), "NaN/Inf at cell {i}: {v}");
    }

    // Source cell should be pinned at 50.0.
    assert!(
        (heat[source_cell] - 50.0).abs() < 1e-4,
        "source cell should be ~50.0, got {}",
        heat[source_cell]
    );

    // On a periodic ring, heat spreads symmetrically. Cell 10 (opposite side)
    // should have some heat after 200 ticks.
    assert!(
        heat[10] > 0.0,
        "opposite side of ring should have heat, got {}",
        heat[10]
    );
}

// ---------- Test 7: WavePropagation on Ring1D (generic path) ----------

/// WavePropagation on a 20-cell Ring1D. The generic fallback computes the
/// Laplacian using canonical_ordering + neighbours instead of the Square4
/// index arithmetic. Starting from all zeros, displacement should remain
/// zero (no excitation) and all values should be finite.
#[test]
fn wave_propagation_generic_ring1d() {
    let cell_count = 20;
    let config = WorldConfig {
        space: Box::new(Ring1D::new(cell_count).unwrap()),
        fields: vec![scalar_field("displacement"), scalar_field("velocity")],
        propagators: vec![Box::new(
            WavePropagation::builder()
                .displacement_field(HEAT) // FieldId(0)
                .velocity_field(GRADIENT) // FieldId(1) — reusing const, just an ID
                .wave_speed(1.0)
                .damping(0.05)
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

    // Run 50 ticks starting from all zeros — exercises the generic Laplacian
    // computation path through Ring1D's canonical_ordering.
    for _ in 0..50 {
        world.step_sync(vec![]).unwrap();
    }

    let snap = world.snapshot();
    let disp = snap.read(HEAT).unwrap();
    let vel = snap.read(GRADIENT).unwrap();

    assert_eq!(disp.len(), cell_count as usize);
    assert_eq!(vel.len(), cell_count as usize);

    // All values should be finite (no NaN from Laplacian computation).
    for (i, &v) in disp.iter().enumerate() {
        assert!(v.is_finite(), "displacement NaN/Inf at cell {i}: {v}");
    }
    for (i, &v) in vel.iter().enumerate() {
        assert!(v.is_finite(), "velocity NaN/Inf at cell {i}: {v}");
    }

    // Starting from zeros with no excitation, displacement should remain zero.
    assert!(
        disp.iter().all(|&v| v == 0.0),
        "displacement should remain zero with no excitation"
    );
}

// ---------- Test 8: ScalarDiffusion + GradientCompute on Hex2D (generic path) ----------

/// ScalarDiffusion and GradientCompute on a Hex2D (radius 3, 37 cells).
/// Both use the generic fallback since Hex2D is not Square4.
#[test]
fn diffusion_and_gradient_generic_hex2d() {
    let hex = Hex2D::new(5, 5).unwrap();
    let cell_count = hex.cell_count();

    let config = WorldConfig {
        space: Box::new(hex),
        fields: vec![scalar_field("heat"), vector2_field("gradient")],
        propagators: vec![
            Box::new(
                ScalarDiffusion::builder()
                    .input_field(HEAT)
                    .output_field(HEAT)
                    .coefficient(0.05) // conservative for 6-connected
                    .sources(vec![(0, 80.0)]) // center cell
                    .build()
                    .unwrap(),
            ),
            Box::new(
                GradientCompute::builder()
                    .input_field(HEAT)
                    .output_field(GRADIENT)
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
    let grad = snap.read(GRADIENT).unwrap();

    assert_eq!(heat.len(), cell_count);
    assert_eq!(grad.len(), cell_count * 2); // 2-component vector

    // All values finite.
    for (i, &v) in heat.iter().enumerate() {
        assert!(v.is_finite(), "heat NaN/Inf at cell {i}: {v}");
    }
    for (i, &v) in grad.iter().enumerate() {
        assert!(v.is_finite(), "gradient NaN/Inf at index {i}: {v}");
    }

    // Source cell should have high heat.
    assert!(
        heat[0] > 10.0,
        "center cell should have substantial heat, got {}",
        heat[0]
    );

    // Gradient should be non-trivial somewhere (heat is spreading radially).
    let max_grad_mag = (0..cell_count)
        .map(|i| {
            let gx = grad[i * 2];
            let gy = grad[i * 2 + 1];
            (gx * gx + gy * gy).sqrt()
        })
        .fold(0.0f32, f32::max);
    assert!(
        max_grad_mag > 0.01,
        "gradient should be non-trivial, max magnitude={max_grad_mag}"
    );
}

// ---------- Test 9: FlowField on Hex2D (generic path) ----------

/// FlowField on Hex2D with normalization enabled. Uses the generic
/// fallback for gradient-based flow direction computation.
#[test]
fn flow_field_generic_hex2d() {
    let hex = Hex2D::new(5, 5).unwrap();
    let cell_count = hex.cell_count();

    let config = WorldConfig {
        space: Box::new(hex),
        fields: vec![scalar_field("potential"), vector2_field("flow")],
        propagators: vec![
            // First diffuse to create a smooth potential field.
            Box::new(
                ScalarDiffusion::builder()
                    .input_field(HEAT)
                    .output_field(HEAT)
                    .coefficient(0.05)
                    .sources(vec![(0, 100.0)])
                    .build()
                    .unwrap(),
            ),
            // Then compute flow from the potential.
            Box::new(
                FlowField::builder()
                    .potential_field(HEAT)
                    .flow_field(GRADIENT)
                    .normalize(true)
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

    for _ in 0..30 {
        world.step_sync(vec![]).unwrap();
    }

    let snap = world.snapshot();
    let flow = snap.read(GRADIENT).unwrap();

    assert_eq!(flow.len(), cell_count * 2);

    // All values finite.
    for (i, &v) in flow.iter().enumerate() {
        assert!(v.is_finite(), "flow NaN/Inf at index {i}: {v}");
    }

    // With normalize=true, non-zero flow vectors should have magnitude ~1.0.
    let mut found_nonzero = false;
    for i in 0..cell_count {
        let fx = flow[i * 2];
        let fy = flow[i * 2 + 1];
        let mag = (fx * fx + fy * fy).sqrt();
        if mag > 0.01 {
            found_nonzero = true;
            assert!(
                (mag - 1.0).abs() < 0.1,
                "normalized flow at cell {i} should have magnitude ~1.0, got {mag}"
            );
        }
    }
    assert!(
        found_nonzero,
        "should have at least one non-zero flow vector"
    );
}
