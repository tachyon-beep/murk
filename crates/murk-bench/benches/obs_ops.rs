//! Criterion micro-benchmarks for observation pipeline operations.
//!
//! Phase 3 baseline focus:
//! - fixed-region extraction throughput
//! - agent-relative extraction throughput under batched centers

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use murk_core::{Coord, FieldId};
use murk_engine::LockstepWorld;
use murk_obs::{ObsDtype, ObsEntry, ObsPlan, ObsRegion, ObsSpec, ObsTransform};
use murk_propagators::agent_movement::new_action_buffer;
use murk_space::RegionSpec;
use smallvec::smallvec;

/// Heat scalar field — matches the reference pipeline's field 0.
const HEAT: FieldId = FieldId(0);

use murk_bench::reference_profile;

/// Build a fixed-region ObsSpec: 1 field (heat), All region, no transform.
fn fixed_obs_spec() -> ObsSpec {
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

/// Build an agent-relative ObsSpec using AgentDisk radius 3.
fn agent_obs_spec() -> ObsSpec {
    ObsSpec {
        entries: vec![ObsEntry {
            field_id: HEAT,
            region: ObsRegion::AgentDisk { radius: 3 },
            pool: None,
            transform: ObsTransform::Identity,
            dtype: ObsDtype::F32,
        }],
    }
}

fn make_world_with_snapshot() -> LockstepWorld {
    let ab = new_action_buffer();
    let config = reference_profile(42, ab);
    let mut world = LockstepWorld::new(config).unwrap();
    world.step_sync(vec![]).unwrap();
    world
}

fn make_agent_centers(n: usize) -> Vec<Coord> {
    // 100x100 grid coordinates; deterministic spread.
    (0..n)
        .map(|i| {
            let r = (i % 100) as i32;
            let c = ((i * 7) % 100) as i32;
            smallvec![r, c]
        })
        .collect()
}

/// Benchmark: compile fixed and agent-relative plans.
fn bench_obs_compile(c: &mut Criterion) {
    let ab = new_action_buffer();
    let config = reference_profile(42, ab);
    let space = config.space.as_ref();

    let mut group = c.benchmark_group("obs_compile");
    for (name, spec) in [
        ("fixed_all", fixed_obs_spec()),
        ("agent_disk_r3", agent_obs_spec()),
    ] {
        group.bench_with_input(BenchmarkId::new("compile", name), &spec, |b, spec| {
            b.iter(|| {
                let result = ObsPlan::compile(spec, space).unwrap();
                std::hint::black_box(&result);
            });
        });
    }
    group.finish();
}

/// Benchmark: execute fixed-region extraction over a 10K-cell snapshot.
fn bench_obs_execute_fixed_10k(c: &mut Criterion) {
    let ab = new_action_buffer();
    let config = reference_profile(42, ab);
    let space = config.space.as_ref();

    let spec = fixed_obs_spec();
    let plan_result = ObsPlan::compile(&spec, space).unwrap();

    let world = make_world_with_snapshot();

    let output_len = plan_result.output_len;
    let mask_len = plan_result.mask_len;
    let plan = plan_result.plan;

    let mut output = vec![0.0f32; output_len];
    let mut mask = vec![0u8; mask_len];

    let mut group = c.benchmark_group("obs_execute_fixed");
    group.throughput(Throughput::Elements(output_len as u64));
    group.bench_function("all_10k", |b| {
        b.iter(|| {
            let snap = world.snapshot();
            let meta = plan.execute(&snap, None, &mut output, &mut mask).unwrap();
            std::hint::black_box(&meta);
        });
    });
    group.finish();
}

/// Benchmark: execute agent-relative extraction for representative batch sizes.
fn bench_obs_execute_agents(c: &mut Criterion) {
    let ab = new_action_buffer();
    let config = reference_profile(42, ab);
    let space = config.space.as_ref();

    let spec = agent_obs_spec();
    let plan_result = ObsPlan::compile(&spec, space).unwrap();
    let world = make_world_with_snapshot();
    let plan = plan_result.plan;
    let per_agent_output = plan_result.output_len;
    let per_agent_mask = plan_result.mask_len;

    let mut group = c.benchmark_group("obs_execute_agents");
    for n_agents in [16usize, 64usize] {
        let centers = make_agent_centers(n_agents);
        let mut output = vec![0.0f32; per_agent_output * n_agents];
        let mut mask = vec![0u8; per_agent_mask * n_agents];
        group.throughput(Throughput::Elements(n_agents as u64));
        group.bench_with_input(
            BenchmarkId::new("agent_disk_r3", n_agents),
            &centers,
            |b, centers| {
                b.iter(|| {
                    let snap = world.snapshot();
                    let meta = plan
                        .execute_agents(&snap, space, centers, None, &mut output, &mut mask)
                        .unwrap();
                    std::hint::black_box(&meta);
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_obs_compile,
    bench_obs_execute_fixed_10k,
    bench_obs_execute_agents
);
criterion_main!(benches);
