//! Criterion micro-benchmarks for space/topology operations.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use murk_space::{EdgeBehavior, Hex2D, Line1D, ProductSpace, Space, Square4};
use smallvec::smallvec;

/// Benchmark: Call neighbours() on all 10K cells of a 100x100 Square4.
fn bench_neighbours_square4_10k(c: &mut Criterion) {
    let space = Square4::new(100, 100, EdgeBehavior::Absorb).unwrap();

    c.bench_function("neighbours_square4_10k", |b| {
        b.iter(|| {
            for r in 0..100i32 {
                for col in 0..100i32 {
                    let coord = smallvec![r, col];
                    let n = space.neighbours(&coord);
                    black_box(&n);
                }
            }
        });
    });
}

/// Benchmark: Call neighbours() on all Hex2D cells for a hex grid of similar size.
///
/// Hex2D(100, 100) = 10K cells.
fn bench_neighbours_hex2d_10k(c: &mut Criterion) {
    let space = Hex2D::new(100, 100).unwrap();

    c.bench_function("neighbours_hex2d_10k", |b| {
        b.iter(|| {
            for r in 0..100i32 {
                for q in 0..100i32 {
                    let coord = smallvec![q, r];
                    let n = space.neighbours(&coord);
                    black_box(&n);
                }
            }
        });
    });
}

/// Benchmark: Compute distance() for 1000 random pairs in a Hex2D x Line1D ProductSpace.
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
                black_box(d);
            }
        });
    });
}

criterion_group!(
    benches,
    bench_neighbours_square4_10k,
    bench_neighbours_hex2d_10k,
    bench_distance_product_space
);
criterion_main!(benches);
