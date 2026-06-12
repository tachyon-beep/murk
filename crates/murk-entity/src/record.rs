//! Entity structural records.

use murk_core::{Coord, EntityId};

/// Structural entity state indexed by [`EntityId::slot`].
///
/// Liveness is intentionally not stored here. It is represented by the
/// configured alive property in the entity property slab.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityRecord {
    /// Stable ID for this current slot generation.
    pub id: EntityId,
    /// Current coordinate in simulation space.
    pub coord: Coord,
    /// User-defined entity type tag.
    pub entity_type: u32,
}

#[cfg(test)]
mod tests {
    use murk_core::EntityId;

    use super::*;

    #[test]
    fn record_stores_identity_coordinate_and_type() {
        let record = EntityRecord {
            id: EntityId::new(3, 1),
            coord: vec![4, 5].into(),
            entity_type: 7,
        };

        assert_eq!(record.id.slot(), 3);
        assert_eq!(record.id.generation(), 1);
        assert_eq!(record.coord.as_slice(), &[4, 5]);
        assert_eq!(record.entity_type, 7);
    }
}
