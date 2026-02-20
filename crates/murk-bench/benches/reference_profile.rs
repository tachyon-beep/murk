//! Criterion benchmarks for the reference propagator pipeline.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use murk_bench::{reference_profile, stress_profile};
use murk_engine::LockstepWorld;
use murk_propagators::agent_movement::new_action_buffer;

fn bench_tick_10k(c: &mut Criterion) {
    let ab = new_action_buffer();
    let config = reference_profile(42, ab);
    let mut world = LockstepWorld::new(config).unwrap();

    // Warm up: run one tick so initial allocation is done
    world.step_sync(vec![]).unwrap();

    c.bench_function("tick_10k", |b| {
        b.iter(|| {
            let result = world.step_sync(vec![]).unwrap();
            black_box(&result);
        });
    });
}

fn bench_tick_100k(c: &mut Criterion) {
    let ab = new_action_buffer();
    let config = stress_profile(42, ab);
    let mut world = LockstepWorld::new(config).unwrap();

    world.step_sync(vec![]).unwrap();

    c.bench_function("tick_100k", |b| {
        b.iter(|| {
            let result = world.step_sync(vec![]).unwrap();
            black_box(&result);
        });
    });
}

fn bench_1000_ticks_10k(c: &mut Criterion) {
    c.bench_function("1000_ticks_10k", |b| {
        b.iter(|| {
            let ab = new_action_buffer();
            let config = reference_profile(42, ab);
            let mut world = LockstepWorld::new(config).unwrap();
            for _ in 0..1000 {
                let result = world.step_sync(vec![]).unwrap();
                black_box(&result);
            }
        });
    });
}

criterion_group!(
    benches,
    bench_tick_10k,
    bench_tick_100k,
    bench_1000_ticks_10k
);
criterion_main!(benches);
