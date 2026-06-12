//! Property staging — write buffer for propagator entity property mutations.

use murk_core::{EntityId, PropertyIndex};

/// Write buffer for entity property mutations during a tick.
///
/// Values are stored in a flat slab and a `Vec<u64>` bitset tracks which
/// `(entity, property)` pairs were written. The buffer is separate from
/// [`EntityStore`](crate::EntityStore) so a tick can borrow a snapshot for
/// reads while staging property writes.
#[derive(Clone, Debug)]
pub struct PropertyStaging {
    values: Vec<f32>,
    written: Vec<u64>,
    max_entities: u32,
    property_count: u32,
}

impl PropertyStaging {
    /// Create a new staging buffer.
    #[must_use]
    pub fn new(max_entities: u32, property_count: u32) -> Self {
        let total = max_entities as usize * property_count as usize;
        let words = total.div_ceil(64);
        Self {
            values: vec![0.0; total],
            written: vec![0; words],
            max_entities,
            property_count,
        }
    }

    /// Write a property value to staging.
    ///
    /// Returns `false` if the entity slot or property index is out of bounds.
    pub fn set(&mut self, id: EntityId, property: PropertyIndex, value: f32) -> bool {
        let Some(flat) = self.flat_index(id, property) else {
            return false;
        };

        self.values[flat] = value;
        self.written[flat / 64] |= 1_u64 << (flat % 64);
        true
    }

    /// Read a staged value.
    ///
    /// Returns `None` when the pair has not been written or is out of bounds.
    #[must_use]
    pub fn get(&self, id: EntityId, property: PropertyIndex) -> Option<f32> {
        let flat = self.flat_index(id, property)?;
        if self.written[flat / 64] & (1_u64 << (flat % 64)) == 0 {
            return None;
        }

        Some(self.values[flat])
    }

    /// Clear all staged writes.
    pub fn reset(&mut self) {
        self.written.fill(0);
    }

    /// Apply all staged writes to a property slab with matching layout.
    pub fn apply_to(&self, properties: &mut [f32]) {
        let total = self.max_entities as usize * self.property_count as usize;
        for (word_idx, word) in self.written.iter().copied().enumerate() {
            let mut pending = word;
            while pending != 0 {
                let bit = pending.trailing_zeros() as usize;
                let flat = word_idx * 64 + bit;
                if flat < total && flat < properties.len() {
                    properties[flat] = self.values[flat];
                }
                pending &= pending - 1;
            }
        }
    }

    /// Maximum entities this staging buffer supports.
    #[must_use]
    pub fn max_entities(&self) -> u32 {
        self.max_entities
    }

    /// Property count per entity.
    #[must_use]
    pub fn property_count(&self) -> u32 {
        self.property_count
    }

    fn flat_index(&self, id: EntityId, property: PropertyIndex) -> Option<usize> {
        if id.slot() >= self.max_entities || property.0 >= self.property_count {
            return None;
        }

        Some(id.slot() as usize * self.property_count as usize + property.0 as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get_round_trip() {
        let mut staging = PropertyStaging::new(4, 3);
        let id = EntityId::new(1, 0);

        assert!(staging.set(id, PropertyIndex(2), 42.0));
        assert_eq!(staging.get(id, PropertyIndex(2)), Some(42.0));
    }

    #[test]
    fn get_unwritten_returns_none() {
        let staging = PropertyStaging::new(4, 3);

        assert_eq!(staging.get(EntityId::new(0, 0), PropertyIndex(0)), None);
    }

    #[test]
    fn set_out_of_bounds_returns_false() {
        let mut staging = PropertyStaging::new(4, 3);

        assert!(!staging.set(EntityId::new(10, 0), PropertyIndex(0), 1.0));
        assert!(!staging.set(EntityId::new(0, 0), PropertyIndex(10), 1.0));
    }

    #[test]
    fn reset_clears_all_writes() {
        let mut staging = PropertyStaging::new(4, 3);
        assert!(staging.set(EntityId::new(0, 0), PropertyIndex(0), 1.0));
        assert!(staging.set(EntityId::new(1, 0), PropertyIndex(1), 2.0));

        staging.reset();

        assert_eq!(staging.get(EntityId::new(0, 0), PropertyIndex(0)), None);
        assert_eq!(staging.get(EntityId::new(1, 0), PropertyIndex(1)), None);
    }

    #[test]
    fn apply_to_writes_staged_values() {
        let mut staging = PropertyStaging::new(2, 3);
        assert!(staging.set(EntityId::new(0, 0), PropertyIndex(1), 99.0));
        assert!(staging.set(EntityId::new(1, 0), PropertyIndex(2), 77.0));

        let mut properties = vec![0.0; 6];
        staging.apply_to(&mut properties);

        assert_eq!(properties[1], 99.0);
        assert_eq!(properties[5], 77.0);
        assert_eq!(properties[0], 0.0);
    }

    #[test]
    fn bitset_handles_more_than_64_entries() {
        let mut staging = PropertyStaging::new(10, 8);

        assert!(staging.set(EntityId::new(8, 0), PropertyIndex(6), 3.14));
        assert_eq!(
            staging.get(EntityId::new(8, 0), PropertyIndex(6)),
            Some(3.14)
        );
    }
}
