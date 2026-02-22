//! Criterion micro-benchmarks for arena allocation, write, and snapshot operations.
//!
//! Phase 3 baseline focus:
//! - snapshot publish throughput
//! - sparse write/reuse throughput

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use murk_arena::config::ArenaConfig;
use murk_arena::static_arena::StaticArena;
use murk_arena::PingPongArena;
use murk_core::id::{FieldId, ParameterVersion, TickId};
use murk_core::traits::{FieldWriter, SnapshotAccess};
use murk_core::{BoundaryBehavior, FieldDef, FieldMutability, FieldType};

/// Build 5 PerTick field definitions matching the reference pipeline component count.
fn make_field_defs_5() -> Vec<(FieldId, FieldDef)> {
    vec![
        (
            FieldId(0),
            FieldDef {
                name: "heat".into(),
                field_type: FieldType::Scalar,
                mutability: FieldMutability::PerTick,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            },
        ),
        (
            FieldId(1),
            FieldDef {
                name: "velocity".into(),
                field_type: FieldType::Vector { dims: 2 },
                mutability: FieldMutability::PerTick,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            },
        ),
        (
            FieldId(2),
            FieldDef {
                name: "agent_presence".into(),
                field_type: FieldType::Scalar,
                mutability: FieldMutability::PerTick,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            },
        ),
        (
            FieldId(3),
            FieldDef {
                name: "heat_gradient".into(),
                field_type: FieldType::Vector { dims: 2 },
                mutability: FieldMutability::PerTick,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            },
        ),
        (
            FieldId(4),
            FieldDef {
                name: "reward".into(),
                field_type: FieldType::Scalar,
                mutability: FieldMutability::PerTick,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            },
        ),
    ]
}

/// Build a PingPongArena with 10K cells and 5 fields.
fn make_arena_10k() -> PingPongArena {
    let cell_count = 10_000u32;
    let config = ArenaConfig::new(cell_count);
    let field_defs = make_field_defs_5();
    let static_arena = StaticArena::new(&[]).into_shared();
    PingPongArena::new(config, field_defs, static_arena).unwrap()
}

/// Build a sparse-heavy arena for sparse reuse measurements.
fn make_sparse_arena_10k() -> PingPongArena {
    let cell_count = 10_000u32;
    let config = ArenaConfig::new(cell_count);
    let field_defs = vec![
        (
            FieldId(0),
            FieldDef {
                name: "sparse_resource".into(),
                field_type: FieldType::Scalar,
                mutability: FieldMutability::Sparse,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            },
        ),
        (
            FieldId(1),
            FieldDef {
                name: "dense_aux".into(),
                field_type: FieldType::Scalar,
                mutability: FieldMutability::PerTick,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            },
        ),
    ];
    let static_arena = StaticArena::new(&[]).into_shared();
    PingPongArena::new(config, field_defs, static_arena).unwrap()
}

/// Benchmark: allocate a 10K-cell arena with 5 fields, measure new + begin_tick.
fn bench_arena_alloc_10k(c: &mut Criterion) {
    c.bench_function("arena_alloc_10k", |b| {
        b.iter(|| {
            let mut arena = make_arena_10k();
            let _guard = std::hint::black_box(arena.begin_tick().unwrap());
        });
    });
}

/// Benchmark: write 10K f32 values to a single field via WriteArena.
fn bench_arena_write_10k(c: &mut Criterion) {
    let mut arena = make_arena_10k();

    // Do one tick+publish so publish state is set up.
    {
        let _guard = arena.begin_tick().unwrap();
    }
    arena.publish(TickId(1), ParameterVersion(0)).unwrap();

    let mut tick = 2u64;
    c.bench_function("arena_write_10k", |b| {
        b.iter(|| {
            let mut guard = arena.begin_tick().unwrap();
            let data = guard.writer.write(FieldId(0)).unwrap();
            for (i, val) in data.iter_mut().enumerate() {
                *val = i as f32;
            }
            std::hint::black_box(data[0]);
            arena.publish(TickId(tick), ParameterVersion(0)).unwrap();
            tick += 1;
        });
    });
}

/// Benchmark: publish + snapshot read cycle on 10K cells.
fn bench_arena_snapshot(c: &mut Criterion) {
    let mut arena = make_arena_10k();

    let mut snap_tick = 1u64;
    let mut group = c.benchmark_group("arena_publish_snapshot");
    group.throughput(Throughput::Elements(10_000));
    group.bench_function("borrowed_snapshot_10k", |b| {
        b.iter(|| {
            // Begin tick and write something.
            {
                let mut guard = arena.begin_tick().unwrap();
                let data = guard.writer.write(FieldId(0)).unwrap();
                data[0] = 42.0;
            }
            // Publish with incrementing TickId to match internal generation.
            arena
                .publish(TickId(snap_tick), ParameterVersion(0))
                .unwrap();
            snap_tick += 1;
            // Take snapshot and read field.
            let snap = arena.snapshot();
            let data = snap.read_field(FieldId(0)).unwrap();
            std::hint::black_box(data[0]);
        });
    });
    group.finish();
}

/// Benchmark: owned snapshot creation throughput on 10K cells.
fn bench_arena_owned_snapshot_10k(c: &mut Criterion) {
    let mut arena = make_arena_10k();
    let mut snap_tick = 1u64;

    c.bench_function("arena_owned_snapshot_10k", |b| {
        b.iter(|| {
            {
                let mut guard = arena.begin_tick().unwrap();
                let data = guard.writer.write(FieldId(0)).unwrap();
                data[0] = snap_tick as f32;
            }
            arena
                .publish(TickId(snap_tick), ParameterVersion(0))
                .unwrap();
            snap_tick += 1;
            let owned = arena.owned_snapshot();
            let data = owned.read_field(FieldId(0)).unwrap();
            std::hint::black_box(data[0]);
        });
    });
}

/// Benchmark: sparse write churn and reuse counters under publish load.
fn bench_arena_sparse_reuse_10k(c: &mut Criterion) {
    let mut arena = make_sparse_arena_10k();
    let mut tick = 1u64;

    let mut group = c.benchmark_group("arena_sparse_reuse");
    for active_cells in [128usize, 1024usize] {
        group.throughput(Throughput::Elements(active_cells as u64));
        group.bench_with_input(
            BenchmarkId::new("publish_sparse", active_cells),
            &active_cells,
            |b, &active_cells| {
                b.iter(|| {
                    arena.reset_sparse_reuse_counters();
                    {
                        let mut guard = arena.begin_tick().unwrap();
                        {
                            let sparse = guard.writer.write(FieldId(0)).unwrap();
                            for i in 0..active_cells {
                                let idx = (i * 17) % sparse.len();
                                sparse[idx] = tick as f32;
                            }
                        }
                        {
                            let dense = guard.writer.write(FieldId(1)).unwrap();
                            dense[0] = tick as f32;
                        }
                    }
                    arena.publish(TickId(tick), ParameterVersion(0)).unwrap();
                    tick += 1;
                    std::hint::black_box((
                        arena.sparse_reuse_hits(),
                        arena.sparse_reuse_misses(),
                        arena.sparse_retired_range_count(),
                    ));
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_arena_alloc_10k,
    bench_arena_write_10k,
    bench_arena_snapshot,
    bench_arena_owned_snapshot_10k,
    bench_arena_sparse_reuse_10k
);
criterion_main!(benches);
