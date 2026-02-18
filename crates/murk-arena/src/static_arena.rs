//! Static generation-0 arena for immutable fields.
//!
//! [`StaticArena`] stores `Static`-mutability fields — data that is set once
//! at world creation and never modified. It is a simple `Vec<f32>` with an
//! offset table, wrapped in `Arc` for sharing across vectorized environments.

use std::sync::Arc;

use indexmap::IndexMap;
use murk_core::FieldId;

/// Arena for static (generation-0) field data.
///
/// Static fields (e.g. terrain height, topology masks) are allocated once
/// and shared via `Arc` across all snapshots and vectorized environments.
/// In M2 vectorized training, 128 `LockstepWorld` instances share a single
/// `SharedStaticArena` allocation.
///
/// The data is contiguous in memory — all static fields are packed into a
/// single `Vec<f32>` with an offset table for O(1) lookup.
pub struct StaticArena {
    /// Contiguous storage for all static fields.
    data: Vec<f32>,
    /// Maps FieldId to (offset, len) within `data`.
    field_offsets: IndexMap<FieldId, (usize, usize)>,
}

/// Shared handle for cross-environment static data sharing.
pub type SharedStaticArena = Arc<StaticArena>;

impl StaticArena {
    /// Create a new static arena with space for the given fields.
    ///
    /// `static_fields` maps each static `FieldId` to its required length
    /// (in f32 elements, i.e. `cell_count * components`).
    /// The data is zero-initialised; callers should write initial values
    /// via [`StaticArena::write_field`] before sharing.
    pub fn new(static_fields: &[(FieldId, u32)]) -> Self {
        // Reject duplicate FieldIds — duplicates cause orphaned memory regions
        // and metadata/storage mismatches (IndexMap::insert overwrites silently).
        for (i, &(id, _)) in static_fields.iter().enumerate() {
            for &(other_id, _) in &static_fields[i + 1..] {
                assert!(
                    id != other_id,
                    "duplicate FieldId({}) in static_fields",
                    id.0,
                );
            }
        }

        let total: usize = static_fields.iter().map(|(_, len)| *len as usize).sum();
        let data = vec![0.0; total];

        let mut field_offsets = IndexMap::with_capacity(static_fields.len());
        let mut cursor = 0usize;
        for &(id, len) in static_fields {
            field_offsets.insert(id, (cursor, len as usize));
            cursor += len as usize;
        }

        Self {
            data,
            field_offsets,
        }
    }

    /// Read a static field's data.
    pub fn read_field(&self, field: FieldId) -> Option<&[f32]> {
        let &(offset, len) = self.field_offsets.get(&field)?;
        Some(&self.data[offset..offset + len])
    }

    /// Write initial data to a static field.
    ///
    /// Returns `None` if the field is not registered as static.
    /// This should only be called during world construction, before the
    /// arena is shared via `Arc`.
    pub fn write_field(&mut self, field: FieldId) -> Option<&mut [f32]> {
        let &(offset, len) = self.field_offsets.get(&field)?;
        Some(&mut self.data[offset..offset + len])
    }

    /// Check whether a field is stored in this static arena.
    pub fn contains(&self, field: FieldId) -> bool {
        self.field_offsets.contains_key(&field)
    }

    /// Get the offset and length of a field within the static data vec.
    pub fn field_location(&self, field: FieldId) -> Option<(u32, u32)> {
        self.field_offsets
            .get(&field)
            .map(|&(off, len)| (off as u32, len as u32))
    }

    /// Total memory usage in bytes.
    pub fn memory_bytes(&self) -> usize {
        self.data.len() * std::mem::size_of::<f32>()
    }

    /// Number of static fields.
    pub fn field_count(&self) -> usize {
        self.field_offsets.len()
    }

    /// Wrap this arena in an `Arc` for sharing.
    pub fn into_shared(self) -> SharedStaticArena {
        Arc::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_zeroed_storage() {
        let arena = StaticArena::new(&[(FieldId(0), 100), (FieldId(1), 50)]);
        let data = arena.read_field(FieldId(0)).unwrap();
        assert_eq!(data.len(), 100);
        assert!(data.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn write_and_read_round_trip() {
        let mut arena = StaticArena::new(&[(FieldId(0), 10)]);
        {
            let data = arena.write_field(FieldId(0)).unwrap();
            data[0] = 42.0;
            data[9] = 99.0;
        }
        let data = arena.read_field(FieldId(0)).unwrap();
        assert_eq!(data[0], 42.0);
        assert_eq!(data[9], 99.0);
    }

    #[test]
    fn fields_dont_overlap() {
        let mut arena = StaticArena::new(&[(FieldId(0), 10), (FieldId(1), 5)]);
        {
            let f0 = arena.write_field(FieldId(0)).unwrap();
            f0.fill(1.0);
        }
        {
            let f1 = arena.write_field(FieldId(1)).unwrap();
            f1.fill(2.0);
        }
        assert!(arena
            .read_field(FieldId(0))
            .unwrap()
            .iter()
            .all(|&v| v == 1.0));
        assert!(arena
            .read_field(FieldId(1))
            .unwrap()
            .iter()
            .all(|&v| v == 2.0));
    }

    #[test]
    fn unknown_field_returns_none() {
        let arena = StaticArena::new(&[(FieldId(0), 10)]);
        assert!(arena.read_field(FieldId(99)).is_none());
    }

    #[test]
    fn into_shared_creates_arc() {
        let arena = StaticArena::new(&[(FieldId(0), 10)]);
        let shared = arena.into_shared();
        assert_eq!(Arc::strong_count(&shared), 1);
        let shared2 = Arc::clone(&shared);
        assert_eq!(Arc::strong_count(&shared), 2);
        assert_eq!(shared2.read_field(FieldId(0)).unwrap().len(), 10);
    }

    #[test]
    fn field_location_returns_offset_and_len() {
        let arena = StaticArena::new(&[(FieldId(0), 10), (FieldId(1), 5)]);
        let (off, len) = arena.field_location(FieldId(0)).unwrap();
        assert_eq!(off, 0);
        assert_eq!(len, 10);
        let (off, len) = arena.field_location(FieldId(1)).unwrap();
        assert_eq!(off, 10);
        assert_eq!(len, 5);
    }

    #[test]
    fn memory_bytes_accounts_for_all_fields() {
        let arena = StaticArena::new(&[(FieldId(0), 100), (FieldId(1), 50)]);
        assert_eq!(arena.memory_bytes(), 150 * 4);
    }

    #[test]
    #[should_panic(expected = "duplicate FieldId")]
    fn new_rejects_duplicate_field_ids() {
        StaticArena::new(&[(FieldId(0), 100), (FieldId(0), 50)]);
    }

    #[test]
    #[should_panic(expected = "duplicate FieldId")]
    fn new_rejects_non_adjacent_duplicate_field_ids() {
        StaticArena::new(&[
            (FieldId(0), 10),
            (FieldId(1), 20),
            (FieldId(0), 30),
        ]);
    }

    #[test]
    fn new_accepts_distinct_field_ids() {
        let arena = StaticArena::new(&[
            (FieldId(0), 10),
            (FieldId(1), 20),
            (FieldId(2), 30),
        ]);
        assert_eq!(arena.field_count(), 3);
        assert_eq!(arena.memory_bytes(), 60 * 4);
    }
}
