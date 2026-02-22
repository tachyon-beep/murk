//! Criterion micro-benchmarks for space/topology operations.
//!
//! Phase 3 baseline focus:
//! - coordinate-to-rank lookup latency
//! - canonical ordering materialization cost

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use murk_space::{EdgeBehavior, Hex2D, Line1D, ProductSpace, Space, Square4};
use smallvec::smallvec;

/// Benchmark: call neighbours() on all 10K cells of a 100x100 Square4.
fn bench_neighbours_square4_10k(c: &mut Criterion) {
    let space = Square4::new(100, 100, EdgeBehavior::Absorb).unwrap();

    c.bench_function("neighbours_square4_10k", |b| {
        b.iter(|| {
            for r in 0..100i32 {
                for col in 0..100i32 {
                    let coord = smallvec![r, col];
                    let n = space.neighbours(&coord);
                    std::hint::black_box(&n);
                }
            }
        });
    });
}

/// Benchmark: call neighbours() on all Hex2D cells for a hex grid of similar size.
fn bench_neighbours_hex2d_10k(c: &mut Criterion) {
    let space = Hex2D::new(100, 100).unwrap();

    c.bench_function("neighbours_hex2d_10k", |b| {
        b.iter(|| {
            for r in 0..100i32 {
                for q in 0..100i32 {
                    let coord = smallvec![q, r];
                    let n = space.neighbours(&coord);
                    std::hint::black_box(&n);
                }
            }
        });
    });
}

/// Benchmark: compute distance() for 1000 deterministic pairs in a product space.
fn bench_distance_product_space(c: &mut Criterion) {
    let hex = Hex2D::new(20, 20).unwrap();
    let line = Line1D::new(10, EdgeBehavior::Absorb).unwrap();
    let space = ProductSpace::new(vec![Box::new(hex), Box::new(line)]).unwrap();

    // Pre-compute 1000 deterministic coordinate pairs.
    let mut pairs = Vec::with_capacity(1000);
    for i in 0u64..1000 {
        // Deterministic pseudo-random coordinates within bounds.
        let q_a = (i.wrapping_mul(6364136223846793007) % 20) as i32;
        let r_a = (i.wrapping_mul(1442695040888963407) % 20) as i32;
        let l_a = (i.wrapping_mul(2862933555777941757) % 10) as i32;

        let j = i + 500;
        let q_b = (j.wrapping_mul(6364136223846793007) % 20) as i32;
        let r_b = (j.wrapping_mul(1442695040888963407) % 20) as i32;
        let l_b = (j.wrapping_mul(2862933555777941757) % 10) as i32;

        pairs.push((smallvec![q_a, r_a, l_a], smallvec![q_b, r_b, l_b]));
    }

    c.bench_function("distance_product_space", |b| {
        b.iter(|| {
            for (a, bv) in &pairs {
                let d = space.distance(a, bv);
                std::hint::black_box(d);
            }
        });
    });
}

/// Benchmark: canonical_rank lookups over all 10K Square4 coordinates.
fn bench_canonical_rank_square4_10k(c: &mut Criterion) {
    let space = Square4::new(100, 100, EdgeBehavior::Absorb).unwrap();
    let coords: Vec<_> = (0..100i32)
        .flat_map(|r| (0..100i32).map(move |col| smallvec![r, col]))
        .collect();

    let mut group = c.benchmark_group("space_rank_lookup");
    group.throughput(Throughput::Elements(coords.len() as u64));
    group.bench_function("square4_10k", |b| {
        b.iter(|| {
            for coord in &coords {
                let rank = space.canonical_rank(coord);
                std::hint::black_box(rank);
            }
        });
    });
    group.finish();
}

/// Benchmark: canonical_rank lookups in a 3D product space.
fn bench_canonical_rank_product_space(c: &mut Criterion) {
    let square = Square4::new(100, 100, EdgeBehavior::Absorb).unwrap();
    let line = Line1D::new(10, EdgeBehavior::Absorb).unwrap();
    let space = ProductSpace::new(vec![Box::new(square), Box::new(line)]).unwrap();

    // 4096 deterministic coordinates to model repeated tensor index mappings.
    let coords: Vec<_> = (0..4096u64)
        .map(|i| {
            let r = (i % 100) as i32;
            let c = ((i * 7) % 100) as i32;
            let z = ((i * 3) % 10) as i32;
            smallvec![r, c, z]
        })
        .collect();

    let mut group = c.benchmark_group("space_rank_lookup");
    group.throughput(Throughput::Elements(coords.len() as u64));
    group.bench_with_input(
        BenchmarkId::new("product_square4xline1d", coords.len()),
        &coords,
        |b, coords| {
            b.iter(|| {
                for coord in coords {
                    let rank = space.canonical_rank(coord);
                    std::hint::black_box(rank);
                }
            });
        },
    );
    group.finish();
}

/// Benchmark: canonical_ordering materialization for a 10K-cell topology.
fn bench_canonical_ordering_square4_10k(c: &mut Criterion) {
    let space = Square4::new(100, 100, EdgeBehavior::Absorb).unwrap();
    c.bench_function("canonical_ordering_square4_10k", |b| {
        b.iter(|| {
            let ordering = space.canonical_ordering();
            std::hint::black_box(ordering);
        });
    });
}

criterion_group!(
    benches,
    bench_neighbours_square4_10k,
    bench_neighbours_hex2d_10k,
    bench_distance_product_space,
    bench_canonical_rank_square4_10k,
    bench_canonical_rank_product_space,
    bench_canonical_ordering_square4_10k
);
criterion_main!(benches);
