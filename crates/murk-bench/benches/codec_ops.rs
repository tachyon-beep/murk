//! Criterion micro-benchmarks for replay codec and snapshot hashing.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use murk_core::id::{FieldId, ParameterVersion, TickId, WorldGenerationId};
use murk_replay::codec::{encode_frame, decode_frame};
use murk_replay::hash::snapshot_hash;
use murk_replay::types::{Frame, SerializedCommand};
use murk_test_utils::MockSnapshot;

/// Build a Frame with `n` commands for benchmarking.
fn make_frame(n: usize) -> Frame {
    let commands: Vec<SerializedCommand> = (0..n)
        .map(|i| {
            // Simulate a Move-like payload: 8 bytes entity_id + 12 bytes coord.
            let mut payload = Vec::with_capacity(20);
            payload.extend_from_slice(&(i as u64).to_le_bytes());
            // 2D coord: len(4 bytes) + 2 * i32(4 bytes) = 12 bytes
            payload.extend_from_slice(&2u32.to_le_bytes());
            payload.extend_from_slice(&(i as i32).to_le_bytes());
            payload.extend_from_slice(&((i + 1) as i32).to_le_bytes());

            SerializedCommand {
                payload_type: 0, // PAYLOAD_MOVE
                payload,
                priority_class: 1,
                source_id: Some(i as u64),
                source_seq: Some(i as u64),
            }
        })
        .collect();

    Frame {
        tick_id: 42,
        commands,
        snapshot_hash: 0xDEADBEEF,
    }
}

/// Build a MockSnapshot with 10K cells x 5 fields.
fn make_mock_snapshot_10k_5fields() -> MockSnapshot {
    let mut snap = MockSnapshot::new(TickId(1), WorldGenerationId(1), ParameterVersion(0));
    for field_idx in 0..5u32 {
        let data: Vec<f32> = (0..10_000).map(|i| (i + field_idx * 10_000) as f32).collect();
        snap.set_field(FieldId(field_idx), data);
    }
    snap
}

/// Benchmark: Encode a Frame with 50 commands.
fn bench_codec_encode_frame(c: &mut Criterion) {
    let frame = make_frame(50);

    c.bench_function("codec_encode_frame", |b| {
        b.iter(|| {
            let mut buf = Vec::with_capacity(4096);
            encode_frame(&mut buf, &frame).unwrap();
            black_box(&buf);
        });
    });
}

/// Benchmark: Decode the same frame.
fn bench_codec_decode_frame(c: &mut Criterion) {
    let frame = make_frame(50);

    // Pre-encode the frame into a buffer.
    let mut encoded = Vec::with_capacity(4096);
    encode_frame(&mut encoded, &frame).unwrap();

    c.bench_function("codec_decode_frame", |b| {
        b.iter(|| {
            let mut cursor = encoded.as_slice();
            let decoded = decode_frame(&mut cursor).unwrap().unwrap();
            black_box(&decoded);
        });
    });
}

/// Benchmark: Hash 10K cells x 5 fields via snapshot_hash.
fn bench_snapshot_hash_10k(c: &mut Criterion) {
    let snap = make_mock_snapshot_10k_5fields();

    c.bench_function("snapshot_hash_10k", |b| {
        b.iter(|| {
            let h = snapshot_hash(&snap, 5);
            black_box(h);
        });
    });
}

criterion_group!(
    benches,
    bench_codec_encode_frame,
    bench_codec_decode_frame,
    bench_snapshot_hash_10k
);
criterion_main!(benches);
