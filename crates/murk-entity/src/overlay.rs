//! Entity overlay reader — Euler-style reads with staging fallback.

use murk_core::{EntityId, EntityManifest, PropertyIndex};

use crate::record::EntityRecord;
use crate::snapshot::EntitySnapshot;
use crate::staging::PropertyStaging;

/// Euler-style entity reader that checks staging before snapshot.
///
/// The snapshot and staging lifetimes are intentionally separate so callers can
/// compose immutable tick-start reads with mutable per-tick write staging.
#[derive(Clone, Copy, Debug)]
pub struct EntityOverlayReader<'snap, 'staging> {
    snapshot: EntitySnapshot<'snap>,
    staging: &'staging PropertyStaging,
}

impl<'snap, 'staging> EntityOverlayReader<'snap, 'staging> {
    /// Create an overlay reader.
    #[must_use]
    pub fn new(snapshot: EntitySnapshot<'snap>, staging: &'staging PropertyStaging) -> Self {
        Self { snapshot, staging }
    }

    /// Read a property with Euler semantics.
    ///
    /// Staging takes precedence, but only after the snapshot confirms the ID is
    /// valid for the current generation.
    #[must_use]
    pub fn property(&self, id: EntityId, property: PropertyIndex) -> Option<f32> {
        self.snapshot.get(id)?;
        self.staging
            .get(id, property)
            .or_else(|| self.snapshot.property(id, property))
    }

    /// Look up structural entity data from the snapshot.
    #[must_use]
    pub fn get(&self, id: EntityId) -> Option<&EntityRecord> {
        self.snapshot.get(id)
    }

    /// Iterate all occupied records from the snapshot.
    pub fn iter_all(&self) -> impl Iterator<Item = &EntityRecord> {
        self.snapshot.iter_all()
    }

    /// Iterate alive records from the snapshot.
    pub fn iter_alive(&self) -> impl Iterator<Item = &EntityRecord> + '_ {
        self.snapshot.iter_alive()
    }

    /// Return whether an entity is marked alive in the snapshot.
    #[must_use]
    pub fn is_alive(&self, id: EntityId) -> bool {
        self.snapshot.is_alive(id)
    }

    /// The property manifest.
    #[must_use]
    pub fn manifest(&self) -> &EntityManifest {
        self.snapshot.manifest()
    }
}

#[cfg(test)]
mod tests {
    use murk_core::{EntityManifest, PropertyIndex};

    use super::*;
    use crate::store::EntityStore;

    fn test_manifest() -> EntityManifest {
        EntityManifest {
            property_names: vec!["alive".into(), "hp".into()],
            property_defaults: vec![1.0, 100.0],
            alive_property: PropertyIndex(0),
        }
    }

    #[test]
    fn overlay_returns_staged_value_over_snapshot() {
        let mut store = EntityStore::new(4, test_manifest());
        let id = store.spawn(vec![0].into(), 0, &[]).unwrap();
        let snapshot = store.snapshot();

        let mut staging = PropertyStaging::new(4, 2);
        assert!(staging.set(id, PropertyIndex(1), 50.0));

        let overlay = EntityOverlayReader::new(snapshot, &staging);
        assert_eq!(overlay.property(id, PropertyIndex(1)), Some(50.0));
        assert_eq!(overlay.property(id, PropertyIndex(0)), Some(1.0));
    }

    #[test]
    fn overlay_falls_through_to_snapshot_when_not_staged() {
        let mut store = EntityStore::new(4, test_manifest());
        let id = store.spawn(vec![0].into(), 0, &[]).unwrap();
        let snapshot = store.snapshot();
        let staging = PropertyStaging::new(4, 2);

        let overlay = EntityOverlayReader::new(snapshot, &staging);
        assert_eq!(overlay.property(id, PropertyIndex(1)), Some(100.0));
    }

    #[test]
    fn overlay_get_delegates_to_snapshot() {
        let mut store = EntityStore::new(4, test_manifest());
        let id = store.spawn(vec![3, 7].into(), 1, &[]).unwrap();
        let snapshot = store.snapshot();
        let staging = PropertyStaging::new(4, 2);

        let overlay = EntityOverlayReader::new(snapshot, &staging);
        let record = overlay.get(id).unwrap();
        assert_eq!(record.coord.as_slice(), &[3, 7]);
        assert_eq!(record.entity_type, 1);
    }

    #[test]
    fn overlay_stale_id_returns_none() {
        let mut store = EntityStore::new(4, test_manifest());
        let old = store.spawn(vec![0].into(), 0, &[]).unwrap();
        store.despawn(old).unwrap();
        let _new = store.spawn(vec![1].into(), 0, &[]).unwrap();
        let snapshot = store.snapshot();
        let staging = PropertyStaging::new(4, 2);

        let overlay = EntityOverlayReader::new(snapshot, &staging);
        assert!(overlay.get(old).is_none());
        assert_eq!(overlay.property(old, PropertyIndex(0)), None);
    }

    #[test]
    fn overlay_stale_id_does_not_read_staged_value_for_recycled_slot() {
        let mut store = EntityStore::new(4, test_manifest());
        let old = store.spawn(vec![0].into(), 0, &[]).unwrap();
        store.despawn(old).unwrap();
        let new_id = store.spawn(vec![1].into(), 0, &[]).unwrap();
        assert_eq!(old.slot(), new_id.slot());

        let snapshot = store.snapshot();
        let mut staging = PropertyStaging::new(4, 2);
        assert!(staging.set(new_id, PropertyIndex(1), 999.0));

        let overlay = EntityOverlayReader::new(snapshot, &staging);
        assert_eq!(overlay.property(old, PropertyIndex(1)), None);
        assert_eq!(overlay.property(new_id, PropertyIndex(1)), Some(999.0));
    }
}
