//! Criterion micro-benchmarks for observation pipeline operations.

use criterion::{criterion_group, criterion_main, Criterion};
use murk_core::FieldId;
use murk_engine::LockstepWorld;
use murk_obs::{ObsDtype, ObsEntry, ObsPlan, ObsRegion, ObsSpec, ObsTransform};
use murk_propagators::agent_movement::new_action_buffer;
use murk_space::RegionSpec;

/// Heat scalar field — matches the reference pipeline's field 0.
const HEAT: FieldId = FieldId(0);

use murk_bench::reference_profile;

/// Build a simple ObsSpec: 1 field (heat), All region, no transform.
fn simple_obs_spec() -> ObsSpec {
    ObsSpec {
        entries: vec![ObsEntry {
            field_id: HEAT,
            region: ObsRegion::Fixed(RegionSpec::All),
            pool: None,
            transform: ObsTransform::Identity,
            dtype: ObsDtype::F32,
        }],
    }
}

/// Benchmark: Compile an ObsPlan from a simple ObsSpec (1 field, All region).
fn bench_obs_compile_simple(c: &mut Criterion) {
    let ab = new_action_buffer();
    let config = reference_profile(42, ab);
    let space = config.space.as_ref();

    let spec = simple_obs_spec();

    c.bench_function("obs_compile_simple", |b| {
        b.iter(|| {
            let result = ObsPlan::compile(&spec, space).unwrap();
            std::hint::black_box(&result);
        });
    });
}

/// Benchmark: Execute a compiled plan against a 10K-cell snapshot.
fn bench_obs_execute_10k(c: &mut Criterion) {
    let ab = new_action_buffer();
    let config = reference_profile(42, ab);
    let space = config.space.as_ref();

    let spec = simple_obs_spec();
    let plan_result = ObsPlan::compile(&spec, space).unwrap();

    // Create a world and step once to get a valid snapshot.
    let ab2 = new_action_buffer();
    let config2 = reference_profile(42, ab2);
    let mut world = LockstepWorld::new(config2).unwrap();
    world.step_sync(vec![]).unwrap();

    let output_len = plan_result.output_len;
    let mask_len = plan_result.mask_len;
    let plan = plan_result.plan;

    let mut output = vec![0.0f32; output_len];
    let mut mask = vec![0u8; mask_len];

    c.bench_function("obs_execute_10k", |b| {
        b.iter(|| {
            let snap = world.snapshot();
            let meta = plan.execute(&snap, None, &mut output, &mut mask).unwrap();
            std::hint::black_box(&meta);
        });
    });
}

/// Benchmark: Execute a plan for 16 agents via 16 sequential execute calls.
fn bench_obs_execute_batch_16(c: &mut Criterion) {
    let ab = new_action_buffer();
    let config = reference_profile(42, ab);
    let space = config.space.as_ref();

    let spec = simple_obs_spec();
    let plan_result = ObsPlan::compile(&spec, space).unwrap();

    // Create a world and step once.
    let ab2 = new_action_buffer();
    let config2 = reference_profile(42, ab2);
    let mut world = LockstepWorld::new(config2).unwrap();
    world.step_sync(vec![]).unwrap();

    let output_len = plan_result.output_len;
    let mask_len = plan_result.mask_len;
    let plan = plan_result.plan;

    // Pre-allocate 16 output buffers.
    let mut outputs: Vec<Vec<f32>> = (0..16).map(|_| vec![0.0f32; output_len]).collect();
    let mut masks: Vec<Vec<u8>> = (0..16).map(|_| vec![0u8; mask_len]).collect();

    c.bench_function("obs_execute_batch_16", |b| {
        b.iter(|| {
            let snap = world.snapshot();
            for i in 0..16 {
                let meta = plan
                    .execute(&snap, None, &mut outputs[i], &mut masks[i])
                    .unwrap();
                std::hint::black_box(&meta);
            }
        });
    });
}

criterion_group!(
    benches,
    bench_obs_compile_simple,
    bench_obs_execute_10k,
    bench_obs_execute_batch_16
);
criterion_main!(benches);
