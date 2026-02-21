//! Hashing utilities for snapshot and configuration comparison.
//!
//! Uses FNV-1a for fast, deterministic hashing of simulation state.
//! These hashes are not cryptographically secure — they are used for
//! fast equality checks during replay comparison.

use murk_core::id::FieldId;
use murk_core::traits::SnapshotAccess;

/// FNV-1a offset basis for 64-bit.
const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV-1a prime for 64-bit.
const FNV_PRIME: u64 = 0x00000100000001B3;

/// Feed a single byte into an FNV-1a hash state.
#[inline]
fn fnv1a_byte(hash: u64, byte: u8) -> u64 {
    (hash ^ byte as u64).wrapping_mul(FNV_PRIME)
}

/// Feed a u32 (as 4 LE bytes) into an FNV-1a hash state.
#[inline]
fn fnv1a_u32(mut hash: u64, v: u32) -> u64 {
    for &b in &v.to_le_bytes() {
        hash = fnv1a_byte(hash, b);
    }
    hash
}

/// Feed a u64 (as 8 LE bytes) into an FNV-1a hash state.
#[inline]
fn fnv1a_u64(mut hash: u64, v: u64) -> u64 {
    for &b in &v.to_le_bytes() {
        hash = fnv1a_byte(hash, b);
    }
    hash
}

/// Compute a hash over all field data in a snapshot.
///
/// Iterates fields `0..field_count`, reads each via `read_field()`,
/// and hashes every `f32::to_bits()` using FNV-1a. The field index
/// is folded in at field boundaries to ensure field order matters.
///
/// Returns `FNV_OFFSET` (non-zero) when `field_count == 0`, since
/// the hash state is initialized with FNV-1a's offset basis.
pub fn snapshot_hash(snapshot: &dyn SnapshotAccess, field_count: u32) -> u64 {
    let mut hash = FNV_OFFSET;

    for field_idx in 0..field_count {
        // Fold in field index at each boundary
        hash = fnv1a_u32(hash, field_idx);

        if let Some(data) = snapshot.read_field(FieldId(field_idx)) {
            for &v in data {
                hash = fnv1a_u32(hash, v.to_bits());
            }
        }
    }

    hash
}

/// Compute a hash over simulation configuration scalars.
///
/// Hashes seed, dt (as bits), field count, cell count, and space descriptor
/// using FNV-1a. Used to detect configuration mismatches before replay.
pub fn config_hash(
    seed: u64,
    dt_bits: u64,
    field_count: u32,
    cell_count: u64,
    space_descriptor: &[u8],
) -> u64 {
    let mut hash = FNV_OFFSET;
    hash = fnv1a_u64(hash, seed);
    hash = fnv1a_u64(hash, dt_bits);
    hash = fnv1a_u32(hash, field_count);
    hash = fnv1a_u64(hash, cell_count);
    for &b in space_descriptor {
        hash = fnv1a_byte(hash, b);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::id::{ParameterVersion, TickId, WorldGenerationId};
    use murk_test_utils::MockSnapshot;

    fn make_snapshot(fields: Vec<(FieldId, Vec<f32>)>) -> MockSnapshot {
        let mut snap = MockSnapshot::new(TickId(1), WorldGenerationId(1), ParameterVersion(0));
        for (fid, data) in fields {
            snap.set_field(fid, data);
        }
        snap
    }

    #[test]
    fn same_data_same_hash() {
        let snap_a = make_snapshot(vec![
            (FieldId(0), vec![1.0, 2.0, 3.0]),
            (FieldId(1), vec![4.0, 5.0]),
        ]);
        let snap_b = make_snapshot(vec![
            (FieldId(0), vec![1.0, 2.0, 3.0]),
            (FieldId(1), vec![4.0, 5.0]),
        ]);

        assert_eq!(snapshot_hash(&snap_a, 2), snapshot_hash(&snap_b, 2));
    }

    #[test]
    fn different_data_different_hash() {
        let snap_a = make_snapshot(vec![(FieldId(0), vec![1.0, 2.0, 3.0])]);
        let snap_b = make_snapshot(vec![(FieldId(0), vec![1.0, 2.0, 4.0])]);

        assert_ne!(snapshot_hash(&snap_a, 1), snapshot_hash(&snap_b, 1));
    }

    #[test]
    fn field_order_matters() {
        // Same data but assigned to different field indices
        let snap_a = make_snapshot(vec![
            (FieldId(0), vec![1.0, 2.0]),
            (FieldId(1), vec![3.0, 4.0]),
        ]);
        let snap_b = make_snapshot(vec![
            (FieldId(0), vec![3.0, 4.0]),
            (FieldId(1), vec![1.0, 2.0]),
        ]);

        assert_ne!(snapshot_hash(&snap_a, 2), snapshot_hash(&snap_b, 2));
    }

    #[test]
    fn config_hash_same_inputs_same_output() {
        let h1 = config_hash(42, 0x3FB99999A0000000, 5, 10000, &[1, 2, 3]);
        let h2 = config_hash(42, 0x3FB99999A0000000, 5, 10000, &[1, 2, 3]);
        assert_eq!(h1, h2);
    }

    #[test]
    fn config_hash_different_seed_different_output() {
        let h1 = config_hash(42, 0x3FB99999A0000000, 5, 10000, &[]);
        let h2 = config_hash(43, 0x3FB99999A0000000, 5, 10000, &[]);
        assert_ne!(h1, h2);
    }

    #[test]
    fn empty_snapshot_hash_is_deterministic() {
        let snap = MockSnapshot::new(TickId(0), WorldGenerationId(0), ParameterVersion(0));
        let h1 = snapshot_hash(&snap, 0);
        let h2 = snapshot_hash(&snap, 0);
        assert_eq!(h1, h2);
    }

    #[test]
    fn empty_snapshot_hash_is_fnv_offset() {
        let snap = MockSnapshot::new(TickId(0), WorldGenerationId(0), ParameterVersion(0));
        let h = snapshot_hash(&snap, 0);
        assert_eq!(
            h, FNV_OFFSET,
            "empty snapshot hash must equal FNV_OFFSET for replay compatibility"
        );
    }
}
