//! Criterion micro-benchmarks for arena allocation, write, and snapshot operations.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
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

/// Benchmark: Allocate a 10K-cell arena with 5 fields, measure new + begin_tick.
fn bench_arena_alloc_10k(c: &mut Criterion) {
    c.bench_function("arena_alloc_10k", |b| {
        b.iter(|| {
            let mut arena = make_arena_10k();
            let guard = arena.begin_tick().unwrap();
            black_box(guard);
        });
    });
}

/// Benchmark: Write 10K f32 values to a single field via WriteArena.
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
            black_box(data[0]);
            arena.publish(TickId(tick), ParameterVersion(0)).unwrap();
            tick += 1;
        });
    });
}

/// Benchmark: Publish + snapshot read cycle on 10K cells.
fn bench_arena_snapshot(c: &mut Criterion) {
    let mut arena = make_arena_10k();

    let mut snap_tick = 1u64;
    c.bench_function("arena_snapshot", |b| {
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
            black_box(data[0]);
        });
    });
}

criterion_group!(
    benches,
    bench_arena_alloc_10k,
    bench_arena_write_10k,
    bench_arena_snapshot
);
criterion_main!(benches);
