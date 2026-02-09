//! Placeholder benchmark to verify Criterion harness setup.

use criterion::{criterion_group, criterion_main, Criterion};

fn placeholder_bench(c: &mut Criterion) {
    c.bench_function("placeholder", |b| {
        b.iter(|| {
            // Placeholder — real benchmarks will be added in later work packages.
            std::hint::black_box(1 + 1)
        });
    });
}

criterion_group!(benches, placeholder_bench);
criterion_main!(benches);
