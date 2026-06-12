//! Entity snapshot — immutable borrow for observation and propagator reads.

use murk_core::{EntityId, EntityManifest, PropertyIndex};

use crate::record::EntityRecord;

/// Immutable borrow of the entity store.
///
/// All ID-based lookups validate the slot generation. Stale IDs return
/// `None` instead of exposing a later occupant of a recycled slot.
#[derive(Clone, Copy, Debug)]
pub struct EntitySnapshot<'a> {
    records: &'a [Option<EntityRecord>],
    properties: &'a [f32],
    generations: &'a [u32],
    manifest: &'a EntityManifest,
    next_slot: u32,
    property_count: usize,
}

impl<'a> EntitySnapshot<'a> {
    /// Create a snapshot from store internals.
    #[must_use]
    pub fn new(
        records: &'a [Option<EntityRecord>],
        properties: &'a [f32],
        generations: &'a [u32],
        manifest: &'a EntityManifest,
        next_slot: u32,
    ) -> Self {
        Self {
            records,
            properties,
            generations,
            manifest,
            next_slot,
            property_count: manifest.property_count(),
        }
    }

    /// Look up an entity by ID.
    #[must_use]
    pub fn get(&self, id: EntityId) -> Option<&EntityRecord> {
        let slot = self.lookup_slot(id)?;
        self.records[slot].as_ref()
    }

    /// Iterate all occupied records.
    pub fn iter_all(&self) -> impl Iterator<Item = &EntityRecord> {
        self.records[..self.next_slot as usize]
            .iter()
            .filter_map(Option::as_ref)
    }

    /// Iterate records whose alive property is greater than zero.
    pub fn iter_alive(&self) -> impl Iterator<Item = &EntityRecord> + '_ {
        self.iter_all().filter(|record| self.is_alive(record.id))
    }

    /// Read a property value.
    #[must_use]
    pub fn property(&self, id: EntityId, property: PropertyIndex) -> Option<f32> {
        let slot = self.lookup_slot(id)?;
        let offset = self.property_offset(slot as u32, property)?;
        Some(self.properties[offset])
    }

    /// Return whether an entity is present and marked alive.
    #[must_use]
    pub fn is_alive(&self, id: EntityId) -> bool {
        let Some(slot) = self.lookup_slot(id) else {
            return false;
        };
        let alive_property = self.manifest.alive_property;
        self.property_offset(slot as u32, alive_property)
            .is_some_and(|offset| self.properties[offset] > 0.0)
    }

    /// The property manifest.
    #[must_use]
    pub fn manifest(&self) -> &EntityManifest {
        self.manifest
    }

    /// Number of currently alive entities.
    #[must_use]
    pub fn alive_count(&self) -> u32 {
        self.iter_alive().count() as u32
    }

    /// Property count per entity.
    #[must_use]
    pub fn property_count(&self) -> usize {
        self.property_count
    }

    /// Read-only access to the generation array.
    #[must_use]
    pub fn generations(&self) -> &[u32] {
        self.generations
    }

    /// Read-only access to the property slab.
    #[must_use]
    pub fn properties(&self) -> &[f32] {
        self.properties
    }

    fn lookup_slot(&self, id: EntityId) -> Option<usize> {
        let slot = usize::try_from(id.slot()).ok()?;
        if slot >= self.next_slot as usize || self.generations[slot] != id.generation() {
            return None;
        }
        self.records.get(slot)?.as_ref().map(|_| slot)
    }

    fn property_offset(&self, slot: u32, property: PropertyIndex) -> Option<usize> {
        let property = usize::try_from(property.0).ok()?;
        if property >= self.property_count {
            return None;
        }
        Some(slot as usize * self.property_count + property)
    }
}

#[cfg(test)]
mod tests {
    use murk_core::PropertyIndex;

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
    fn snapshot_get_returns_record() {
        let mut store = EntityStore::new(4, test_manifest());
        let id = store.spawn(vec![5, 10].into(), 2, &[]).unwrap();
        let snap = store.snapshot();

        let record = snap.get(id).unwrap();
        assert_eq!(record.coord.as_slice(), &[5, 10]);
        assert_eq!(record.entity_type, 2);
    }

    #[test]
    fn snapshot_stale_id_returns_none() {
        let mut store = EntityStore::new(4, test_manifest());
        let old = store.spawn(vec![0].into(), 0, &[]).unwrap();
        store.despawn(old).unwrap();
        let _new = store.spawn(vec![1].into(), 0, &[]).unwrap();

        let snap = store.snapshot();
        assert!(snap.get(old).is_none());
    }

    #[test]
    fn snapshot_iter_alive_skips_dead_property_values() {
        let mut store = EntityStore::new(4, test_manifest());
        let id0 = store.spawn(vec![0].into(), 0, &[]).unwrap();
        let _id1 = store.spawn(vec![1].into(), 0, &[]).unwrap();
        store.set_property(id0, PropertyIndex(0), 0.0).unwrap();

        let snap = store.snapshot();
        assert_eq!(snap.alive_count(), 1);
        assert_eq!(snap.iter_alive().count(), 1);
        assert_eq!(snap.iter_all().count(), 2);
    }

    #[test]
    fn snapshot_property_validates_generation() {
        let mut store = EntityStore::new(4, test_manifest());
        let old = store.spawn(vec![0].into(), 0, &[]).unwrap();
        store.despawn(old).unwrap();
        let _new = store.spawn(vec![1].into(), 0, &[]).unwrap();

        let snap = store.snapshot();
        assert_eq!(snap.property(old, PropertyIndex(1)), None);
    }

    #[test]
    fn snapshot_property_out_of_range_returns_none() {
        let mut store = EntityStore::new(4, test_manifest());
        let id = store.spawn(vec![0].into(), 0, &[]).unwrap();
        let snap = store.snapshot();

        assert_eq!(snap.property(id, PropertyIndex(99)), None);
    }
}
