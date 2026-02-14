//! Stress test #16: Mass Plan Invalidation Recovery
//!
//! Verifies that the observation system can recover from a bulk plan
//! invalidation event (200 compiled ObsPlans simultaneously invalidated
//! by a topology change) and restore throughput to at least 50% of the
//! pre-invalidation baseline within 500ms (30 ticks at 60 Hz).
//!
//! **Setup:** 200 compiled ObsPlans with varying regions:
//!   - ~66 `RegionSpec::All`
//!   - ~67 `RegionSpec::Disk { center, radius: 5 }`
//!   - ~67 `RegionSpec::Rect { min, max }` (10x10)
//!
//! **Trigger:** At tick 100, call `world.reset(new_seed)` to change the
//! `WorldGenerationId`. All 200 plans receive `PlanInvalidated`.
//!
//! **Pass criterion:** Throughput reaches 50% of pre-invalidation baseline
//! within 500ms. All 200 plans successfully recompiled.
//!
//! **Fail criterion:** Throughput below 50% after 500ms, or any plan fails
//! to recompile.
//!
//! Marked `#[ignore]` because this is a stress test.

use std::time::Instant;

use murk_bench::reference_profile;
use murk_core::error::ObsError;
use murk_core::traits::SnapshotAccess;
use murk_core::{FieldId, WorldGenerationId};
use murk_engine::LockstepWorld;
use murk_obs::spec::{ObsDtype, ObsEntry, ObsRegion, ObsSpec, ObsTransform};
use murk_obs::{ObsPlan, ObsPlanResult};
use murk_propagators::agent_movement::new_action_buffer;
use murk_space::RegionSpec;

/// Total number of observation plans to compile and invalidate.
const PLAN_COUNT: usize = 200;

/// Number of warm-up ticks before measuring baseline.
const WARMUP_TICKS: usize = 100;

/// Number of baseline measurement iterations.
const BASELINE_ITERATIONS: usize = 5;

/// Maximum recovery time in milliseconds.
const MAX_RECOVERY_MS: u64 = 500;

/// Minimum throughput ratio (post-recovery / baseline) to pass.
const MIN_THROUGHPUT_RATIO: f64 = 0.50;

// ── Plan mix ──────────────────────────────────────────────────────

/// Number of `All` region plans (~1/3 of PLAN_COUNT).
const ALL_COUNT: usize = 66;

/// Number of `Disk` region plans (~1/3 of PLAN_COUNT).
const DISK_COUNT: usize = 67;

/// Number of `Rect` region plans (remainder).
const RECT_COUNT: usize = PLAN_COUNT - ALL_COUNT - DISK_COUNT;

/// Build 200 ObsSpecs with a mix of region types.
///
/// Each spec has a single entry observing the heat field (FieldId(0))
/// with varying spatial regions. The field is scalar so it works with
/// the Simple plan class (all Fixed regions).
fn build_specs() -> Vec<ObsSpec> {
    let mut specs = Vec::with_capacity(PLAN_COUNT);

    // ~66 All plans
    for _ in 0..ALL_COUNT {
        specs.push(ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0), // heat
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        });
    }

    // ~67 Disk plans: center varies, radius=5
    for i in 0..DISK_COUNT {
        // Place disk centers at different grid locations to exercise
        // varying region geometries (interior, edge, corner).
        let cx = 10 + (i as i32 * 7) % 80;
        let cy = 10 + (i as i32 * 11) % 80;
        specs.push(ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0), // heat
                region: ObsRegion::Fixed(RegionSpec::Disk {
                    center: smallvec::smallvec![cx, cy],
                    radius: 5,
                }),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        });
    }

    // ~67 Rect plans: 10x10 regions at varying positions
    for i in 0..RECT_COUNT {
        let x0 = 5 + (i as i32 * 13) % 85;
        let y0 = 5 + (i as i32 * 17) % 85;
        let x1 = x0 + 9;
        let y1 = y0 + 9;
        specs.push(ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0), // heat
                region: ObsRegion::Fixed(RegionSpec::Rect {
                    min: smallvec::smallvec![x0, y0],
                    max: smallvec::smallvec![x1, y1],
                }),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        });
    }

    specs
}

/// Compile all specs against the given space and generation, returning
/// the compiled plans and their output/mask buffer sizes.
fn compile_all_plans(
    specs: &[ObsSpec],
    space: &dyn murk_space::Space,
    generation: WorldGenerationId,
) -> Vec<(ObsPlanResult, usize, usize)> {
    specs
        .iter()
        .enumerate()
        .map(|(i, spec)| {
            let result = ObsPlan::compile_bound(spec, space, generation)
                .unwrap_or_else(|e| panic!("plan {i} failed to compile: {e}"));
            let output_len = result.output_len;
            let mask_len = result.mask_len;
            (result, output_len, mask_len)
        })
        .collect()
}

/// Execute all plans against a snapshot. Returns (successes, failures, elapsed_us).
fn execute_all_plans(
    plans: &[(ObsPlanResult, usize, usize)],
    snapshot: &dyn SnapshotAccess,
) -> (usize, usize, u128) {
    let start = Instant::now();
    let mut successes = 0usize;
    let mut failures = 0usize;

    for (plan_result, output_len, mask_len) in plans {
        let mut output = vec![0.0f32; *output_len];
        let mut mask = vec![0u8; *mask_len];
        match plan_result
            .plan
            .execute(snapshot, None, &mut output, &mut mask)
        {
            Ok(_) => successes += 1,
            Err(_) => failures += 1,
        }
    }

    let elapsed_us = start.elapsed().as_micros();
    (successes, failures, elapsed_us)
}

/// Compute throughput in observations per second from plan count and elapsed microseconds.
fn throughput_obs_per_sec(obs_count: usize, elapsed_us: u128) -> f64 {
    if elapsed_us == 0 {
        return f64::INFINITY;
    }
    obs_count as f64 / (elapsed_us as f64 / 1_000_000.0)
}

// ── Main stress test ──────────────────────────────────────────────

#[test]
#[ignore]
fn stress_mass_plan_invalidation_recovery() {
    eprintln!("=== Stress Test #16: Mass Plan Invalidation Recovery ===\n");

    // --- 1. Create a LockstepWorld with reference_profile ---
    let action_buffer = new_action_buffer();
    let config = reference_profile(42, action_buffer);
    let mut world = LockstepWorld::new(config).unwrap();

    // --- 2. Warm up: run 100 ticks ---
    eprintln!("Phase 1: Warming up ({WARMUP_TICKS} ticks)...");
    for _ in 0..WARMUP_TICKS {
        world.step_sync(vec![]).unwrap();
    }
    eprintln!(
        "  Warm-up complete. Current tick: {}",
        world.current_tick().0
    );

    // --- 3. Compile 200 ObsPlans bound to current generation ---
    let pre_reset_generation = world.snapshot().world_generation_id();
    eprintln!(
        "  Pre-reset generation: {}",
        pre_reset_generation.0
    );

    let specs = build_specs();
    let plans = compile_all_plans(&specs, world.space(), pre_reset_generation);

    eprintln!(
        "  Compiled {} plans (All={}, Disk={}, Rect={})",
        plans.len(),
        ALL_COUNT,
        DISK_COUNT,
        RECT_COUNT
    );

    // Verify all plans have the expected compiled generation.
    for (i, (plan_result, _, _)) in plans.iter().enumerate() {
        assert_eq!(
            plan_result.plan.compiled_generation(),
            Some(pre_reset_generation),
            "plan {i} has wrong compiled generation"
        );
    }

    // --- 4. Measure baseline throughput ---
    // Execute all plans against the current snapshot (gen=100) multiple times.
    // We do NOT step between iterations: this isolates observation extraction
    // throughput from tick overhead, and avoids advancing the generation past
    // the value the plans were compiled against.
    eprintln!("\nPhase 2: Measuring baseline throughput...");
    let mut baseline_throughputs = Vec::with_capacity(BASELINE_ITERATIONS);

    for iteration in 0..BASELINE_ITERATIONS {
        let snapshot = world.snapshot();

        let (successes, failures, elapsed_us) = execute_all_plans(&plans, &snapshot);
        assert_eq!(successes, PLAN_COUNT, "all plans should succeed pre-reset");
        assert_eq!(failures, 0, "no plans should fail pre-reset");

        let tput = throughput_obs_per_sec(PLAN_COUNT, elapsed_us);
        baseline_throughputs.push(tput);
        eprintln!(
            "  Baseline iteration {}: {} obs in {}us = {:.0} obs/sec",
            iteration + 1,
            successes,
            elapsed_us,
            tput
        );
    }

    let baseline_avg: f64 =
        baseline_throughputs.iter().sum::<f64>() / baseline_throughputs.len() as f64;
    let target_throughput = baseline_avg * MIN_THROUGHPUT_RATIO;
    eprintln!(
        "  Baseline average: {baseline_avg:.0} obs/sec, 50% target: {target_throughput:.0} obs/sec"
    );

    // --- 5. Trigger topology change via reset ---
    eprintln!("\nPhase 3: Triggering topology change (reset)...");
    let invalidation_start = Instant::now();
    let _post_reset_snap = world.reset(9999).unwrap();
    let post_reset_generation = world.snapshot().world_generation_id();
    eprintln!(
        "  Post-reset generation: {} (was {})",
        post_reset_generation.0, pre_reset_generation.0
    );
    assert_ne!(
        pre_reset_generation, post_reset_generation,
        "reset must change WorldGenerationId"
    );

    // --- 6. Verify all plans get PlanInvalidated ---
    eprintln!("\nPhase 4: Verifying all plans receive PlanInvalidated...");
    {
        let snapshot = world.snapshot();
        let mut invalidated_count = 0usize;

        for (i, (plan_result, output_len, mask_len)) in plans.iter().enumerate() {
            let mut output = vec![0.0f32; *output_len];
            let mut mask = vec![0u8; *mask_len];
            match plan_result
                .plan
                .execute(&snapshot, None, &mut output, &mut mask)
            {
                Err(ObsError::PlanInvalidated { .. }) => {
                    invalidated_count += 1;
                }
                Ok(_) => panic!("plan {i} should have been invalidated but succeeded"),
                Err(e) => panic!("plan {i} returned unexpected error: {e}"),
            }
        }

        assert_eq!(
            invalidated_count, PLAN_COUNT,
            "all {PLAN_COUNT} plans must receive PlanInvalidated"
        );
        eprintln!("  All {invalidated_count} plans correctly received PlanInvalidated.");
    }

    // --- 7. Recompile all plans and measure recovery ---
    // Step once to produce a valid post-reset snapshot with meaningful state.
    // This advances generation from 0 to 1.
    world.step_sync(vec![]).unwrap();
    let post_step_generation = world.snapshot().world_generation_id();

    eprintln!("\nPhase 5: Recompiling plans against new generation...");
    let recompile_start = Instant::now();

    let new_plans = compile_all_plans(&specs, world.space(), post_step_generation);
    let recompile_elapsed = recompile_start.elapsed();

    eprintln!(
        "  Recompiled {} plans in {:.3}ms (generation {})",
        new_plans.len(),
        recompile_elapsed.as_secs_f64() * 1000.0,
        post_step_generation.0,
    );

    // Verify all recompiled plans have the new generation.
    for (i, (plan_result, _, _)) in new_plans.iter().enumerate() {
        assert_eq!(
            plan_result.plan.compiled_generation(),
            Some(post_step_generation),
            "recompiled plan {i} has wrong generation"
        );
    }

    // --- 8. Measure post-recovery throughput ---
    // As with baseline, we do NOT step between iterations.
    eprintln!("\nPhase 6: Measuring post-recovery throughput...");
    let mut recovery_throughputs = Vec::with_capacity(BASELINE_ITERATIONS);

    for iteration in 0..BASELINE_ITERATIONS {
        let snapshot = world.snapshot();

        let (successes, failures, elapsed_us) = execute_all_plans(&new_plans, &snapshot);
        assert_eq!(
            successes, PLAN_COUNT,
            "all recompiled plans should succeed post-reset"
        );
        assert_eq!(
            failures, 0,
            "no recompiled plans should fail post-reset"
        );

        let tput = throughput_obs_per_sec(PLAN_COUNT, elapsed_us);
        recovery_throughputs.push(tput);
        eprintln!(
            "  Recovery iteration {}: {} obs in {}us = {:.0} obs/sec",
            iteration + 1,
            successes,
            elapsed_us,
            tput
        );
    }

    let recovery_avg: f64 =
        recovery_throughputs.iter().sum::<f64>() / recovery_throughputs.len() as f64;

    // --- 9. Check total recovery time ---
    let total_recovery_elapsed = invalidation_start.elapsed();
    eprintln!(
        "\n  Total recovery time (reset + recompile + verify): {:.1}ms",
        total_recovery_elapsed.as_secs_f64() * 1000.0
    );

    // --- 10. Assert pass criteria ---
    eprintln!("\n=== Results ===");
    eprintln!("  Baseline throughput:  {baseline_avg:.0} obs/sec");
    eprintln!("  Recovery throughput:  {recovery_avg:.0} obs/sec");
    let ratio = recovery_avg / baseline_avg;
    eprintln!("  Throughput ratio:     {ratio:.4} (target >= {MIN_THROUGHPUT_RATIO})");
    eprintln!(
        "  Recompile time:       {:.3}ms",
        recompile_elapsed.as_secs_f64() * 1000.0
    );
    eprintln!(
        "  Total recovery time:  {:.1}ms (budget: {MAX_RECOVERY_MS}ms)",
        total_recovery_elapsed.as_secs_f64() * 1000.0
    );

    // Pass criterion 1: throughput ratio >= 50% of baseline
    assert!(
        ratio >= MIN_THROUGHPUT_RATIO,
        "FAIL: post-recovery throughput {recovery_avg:.0} obs/sec is only {:.1}% of \
         baseline {baseline_avg:.0} obs/sec (need >= {:.0}%)",
        ratio * 100.0,
        MIN_THROUGHPUT_RATIO * 100.0,
    );

    // Pass criterion 2: total recovery within 500ms budget
    assert!(
        total_recovery_elapsed.as_millis() <= MAX_RECOVERY_MS as u128,
        "FAIL: total recovery time {:.1}ms exceeds {MAX_RECOVERY_MS}ms budget",
        total_recovery_elapsed.as_secs_f64() * 1000.0,
    );

    // Pass criterion 3: all plans successfully recompiled (already verified above)
    assert_eq!(
        new_plans.len(),
        PLAN_COUNT,
        "all {PLAN_COUNT} plans must be successfully recompiled"
    );

    eprintln!("PASS: Mass plan invalidation recovery completed successfully.");
}

// ── Supplementary non-stress tests ────────────────────────────────

/// Verify that the spec mix compiles successfully against the reference profile space.
#[test]
fn mass_invalidation_specs_compile() {
    let action_buffer = new_action_buffer();
    let config = reference_profile(42, action_buffer);
    let world = LockstepWorld::new(config).unwrap();
    let specs = build_specs();

    assert_eq!(specs.len(), PLAN_COUNT);

    for (i, spec) in specs.iter().enumerate() {
        let result = ObsPlan::compile(spec, world.space());
        assert!(
            result.is_ok(),
            "spec {i} failed to compile: {}",
            result.unwrap_err()
        );
    }
}

/// Verify that reset changes the WorldGenerationId.
#[test]
fn reset_changes_generation_id() {
    let action_buffer = new_action_buffer();
    let config = reference_profile(42, action_buffer);
    let mut world = LockstepWorld::new(config).unwrap();

    // Step once to advance generation from 0.
    world.step_sync(vec![]).unwrap();
    let gen_before = world.snapshot().world_generation_id();

    world.reset(99).unwrap();
    let gen_after = world.snapshot().world_generation_id();

    assert_ne!(
        gen_before, gen_after,
        "reset must change WorldGenerationId (before={}, after={})",
        gen_before.0, gen_after.0
    );
}

/// Verify that compile_bound plans detect PlanInvalidated after reset.
#[test]
fn bound_plan_detects_invalidation_after_reset() {
    let action_buffer = new_action_buffer();
    let config = reference_profile(42, action_buffer);
    let mut world = LockstepWorld::new(config).unwrap();

    // Step to advance generation.
    world.step_sync(vec![]).unwrap();
    let gen = world.snapshot().world_generation_id();

    // Compile a plan bound to this generation.
    let spec = ObsSpec {
        entries: vec![ObsEntry {
            field_id: FieldId(0),
            region: ObsRegion::Fixed(RegionSpec::All),
            pool: None,
            transform: ObsTransform::Identity,
            dtype: ObsDtype::F32,
        }],
    };
    let result = ObsPlan::compile_bound(&spec, world.space(), gen).unwrap();

    // Verify plan works before reset.
    {
        let snapshot = world.snapshot();
        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result
            .plan
            .execute(&snapshot, None, &mut output, &mut mask)
            .expect("plan should succeed before reset");
    }

    // Reset changes generation.
    world.reset(99).unwrap();

    // Plan should now fail with PlanInvalidated.
    {
        let snapshot = world.snapshot();
        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let err = result
            .plan
            .execute(&snapshot, None, &mut output, &mut mask)
            .unwrap_err();
        assert!(
            matches!(err, ObsError::PlanInvalidated { .. }),
            "expected PlanInvalidated, got: {err}"
        );
    }
}
