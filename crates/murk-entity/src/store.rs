//! Fixed-capacity entity storage.

use std::collections::HashMap;

use murk_core::{Coord, EntityId, EntityManifest, IngressError, PropertyIndex};
use smallvec::SmallVec;

use crate::record::EntityRecord;

/// Fixed-capacity entity store with generational IDs and slab properties.
#[derive(Clone, Debug)]
pub struct EntityStore {
    records: Vec<Option<EntityRecord>>,
    properties: Vec<f32>,
    generations: Vec<u32>,
    free_list: Vec<u32>,
    coord_index: HashMap<Coord, SmallVec<[EntityId; 4]>>,
    next_slot: u32,
    capacity: u32,
    manifest: EntityManifest,
}

/// Snapshot used to roll an [`EntityStore`] back to a previous state.
#[derive(Clone, Debug)]
pub struct EntityStoreSnapshot {
    records: Vec<Option<EntityRecord>>,
    properties: Vec<f32>,
    generations: Vec<u32>,
    free_list: Vec<u32>,
    coord_index: HashMap<Coord, SmallVec<[EntityId; 4]>>,
    next_slot: u32,
}

impl EntityStore {
    /// Create an empty store with the supplied entity capacity and manifest.
    ///
    /// # Panics
    ///
    /// Panics when `capacity` is zero, the manifest is invalid, or capacity
    /// exceeds the 20-bit slot space encoded by [`EntityId`].
    #[must_use]
    pub fn new(capacity: u32, manifest: EntityManifest) -> Self {
        assert!(capacity > 0, "entity capacity must be greater than zero");
        assert!(
            capacity <= 1_048_576,
            "entity capacity exceeds EntityId slot space"
        );
        manifest
            .validate()
            .expect("entity manifest must be valid for EntityStore");

        let capacity_usize = capacity as usize;
        let property_count = manifest.property_count();

        Self {
            records: vec![None; capacity_usize],
            properties: vec![0.0; capacity_usize * property_count],
            generations: vec![0; capacity_usize],
            free_list: Vec::new(),
            coord_index: HashMap::new(),
            next_slot: 0,
            capacity,
            manifest,
        }
    }

    /// Spawn an entity at `coord` with optional property overrides.
    pub fn spawn(
        &mut self,
        coord: Coord,
        entity_type: u32,
        property_overrides: &[(PropertyIndex, f32)],
    ) -> Result<EntityId, IngressError> {
        let slot = self.allocate_slot()?;
        let generation = self.generations[slot as usize];
        let id = EntityId::new(slot, generation);
        let record = EntityRecord {
            id,
            coord: coord.clone(),
            entity_type,
        };

        self.write_defaults(slot);
        for (property, value) in property_overrides {
            if !self.set_property_at_slot(slot, *property, *value) {
                self.records[slot as usize] = None;
                return Err(IngressError::NotApplied);
            }
        }
        let alive_property = self.manifest.alive_property;
        let _ = self.set_property_at_slot(slot, alive_property, 1.0);

        self.records[slot as usize] = Some(record);
        self.coord_index.entry(coord).or_default().push(id);

        Ok(id)
    }

    /// Despawn an entity and recycle its slot unless its generation wraps.
    pub fn despawn(&mut self, id: EntityId) -> Result<(), IngressError> {
        let slot = self.validate_live(id)?;
        let coord = self.records[slot]
            .as_ref()
            .expect("validated record")
            .coord
            .clone();

        let alive_property = self.manifest.alive_property;
        let _ = self.set_property_at_slot(id.slot(), alive_property, 0.0);
        self.records[slot] = None;
        self.remove_from_coord_index(&coord, id);

        let next_generation = (self.generations[slot] + 1) & 0xFFF;
        self.generations[slot] = next_generation;
        if next_generation != 0 {
            self.free_list.push(id.slot());
        }

        Ok(())
    }

    /// Move an entity and update the coordinate index.
    pub fn move_entity(&mut self, id: EntityId, target_coord: Coord) -> Result<(), IngressError> {
        let slot = self.validate_live(id)?;
        let old_coord = self.records[slot]
            .as_ref()
            .expect("validated record")
            .coord
            .clone();

        self.remove_from_coord_index(&old_coord, id);
        self.coord_index
            .entry(target_coord.clone())
            .or_default()
            .push(id);
        self.records[slot].as_mut().expect("validated record").coord = target_coord;

        Ok(())
    }

    /// Return the structural record for a live entity.
    #[must_use]
    pub fn get(&self, id: EntityId) -> Option<&EntityRecord> {
        self.lookup_live_slot(id)
            .and_then(|slot| self.records[slot].as_ref())
    }

    /// Read an entity property for a live entity.
    #[must_use]
    pub fn property(&self, id: EntityId, property: PropertyIndex) -> Option<f32> {
        let slot = self.lookup_live_slot(id)?;
        self.property_offset(slot as u32, property)
            .map(|offset| self.properties[offset])
    }

    /// Set an entity property for a live entity.
    pub fn set_property(
        &mut self,
        id: EntityId,
        property: PropertyIndex,
        value: f32,
    ) -> Result<(), IngressError> {
        let slot = self.validate_live(id)?;
        self.set_property_at_slot(slot as u32, property, value)
            .then_some(())
            .ok_or(IngressError::NotApplied)
    }

    /// Return IDs currently indexed at a coordinate.
    #[must_use]
    pub fn at_coord(&self, coord: &Coord) -> &[EntityId] {
        self.coord_index.get(coord).map_or(&[], SmallVec::as_slice)
    }

    /// Return the number of live entities.
    #[must_use]
    pub fn alive_count(&self) -> usize {
        self.iter_alive().count()
    }

    /// Return whether the slot currently holds a live entity.
    #[must_use]
    pub fn is_alive_at_slot(&self, slot: u32) -> bool {
        let Some(Some(_)) = self.records.get(slot as usize) else {
            return false;
        };
        let alive_property = self.manifest.alive_property;
        self.property_offset(slot, alive_property)
            .is_some_and(|offset| self.properties[offset] > 0.0)
    }

    /// Iterate all occupied entity records.
    pub fn iter_all(&self) -> impl Iterator<Item = &EntityRecord> {
        self.records.iter().filter_map(Option::as_ref)
    }

    /// Iterate all live entity records.
    pub fn iter_alive(&self) -> impl Iterator<Item = &EntityRecord> {
        self.iter_all()
            .filter(|record| self.is_alive_at_slot(record.id.slot()))
    }

    /// Return the configured capacity.
    #[must_use]
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Return this store's entity manifest.
    #[must_use]
    pub fn manifest(&self) -> &EntityManifest {
        &self.manifest
    }

    /// Create an immutable snapshot of the current store state.
    #[must_use]
    pub fn snapshot(&self) -> crate::snapshot::EntitySnapshot<'_> {
        crate::snapshot::EntitySnapshot::new(
            &self.records[..self.next_slot as usize],
            &self.properties,
            &self.generations,
            &self.manifest,
            self.next_slot,
        )
    }

    /// Capture all mutable store state for rollback.
    #[must_use]
    pub fn snapshot_for_rollback(&self) -> EntityStoreSnapshot {
        EntityStoreSnapshot {
            records: self.records.clone(),
            properties: self.properties.clone(),
            generations: self.generations.clone(),
            free_list: self.free_list.clone(),
            coord_index: self.coord_index.clone(),
            next_slot: self.next_slot,
        }
    }

    /// Restore a previously captured rollback snapshot.
    pub fn restore_from_snapshot(&mut self, snapshot: EntityStoreSnapshot) {
        self.records = snapshot.records;
        self.properties = snapshot.properties;
        self.generations = snapshot.generations;
        self.free_list = snapshot.free_list;
        self.coord_index = snapshot.coord_index;
        self.next_slot = snapshot.next_slot;
    }

    /// Apply staged property values directly to this store's property slab.
    pub fn apply_staged_properties(&mut self, staging: &crate::staging::PropertyStaging) {
        staging.apply_to(&mut self.properties);
    }

    fn allocate_slot(&mut self) -> Result<u32, IngressError> {
        if let Some(slot) = self.free_list.pop() {
            return Ok(slot);
        }
        if self.next_slot >= self.capacity {
            return Err(IngressError::EntityCapacityFull);
        }

        let slot = self.next_slot;
        self.next_slot += 1;
        Ok(slot)
    }

    fn validate_live(&self, id: EntityId) -> Result<usize, IngressError> {
        self.lookup_live_slot(id).ok_or(IngressError::UnknownEntity)
    }

    fn lookup_live_slot(&self, id: EntityId) -> Option<usize> {
        let slot = usize::try_from(id.slot()).ok()?;
        if slot >= self.records.len() || self.generations[slot] != id.generation() {
            return None;
        }
        self.records[slot].as_ref().map(|_| slot)
    }

    fn write_defaults(&mut self, slot: u32) {
        let property_count = self.manifest.property_count();
        let offset = slot as usize * property_count;
        self.properties[offset..offset + property_count]
            .copy_from_slice(&self.manifest.property_defaults);
    }

    fn property_offset(&self, slot: u32, property: PropertyIndex) -> Option<usize> {
        let property = usize::try_from(property.0).ok()?;
        if property >= self.manifest.property_count() || slot >= self.capacity {
            return None;
        }
        Some(slot as usize * self.manifest.property_count() + property)
    }

    fn set_property_at_slot(&mut self, slot: u32, property: PropertyIndex, value: f32) -> bool {
        if let Some(offset) = self.property_offset(slot, property) {
            self.properties[offset] = value;
            true
        } else {
            false
        }
    }

    fn remove_from_coord_index(&mut self, coord: &Coord, id: EntityId) {
        let Some(ids) = self.coord_index.get_mut(coord) else {
            return;
        };

        ids.retain(|candidate| *candidate != id);
        if ids.is_empty() {
            self.coord_index.remove(coord);
        }
    }

    #[cfg(test)]
    fn set_generation_for_test(&mut self, slot: u32, generation: u32) {
        self.generations[slot as usize] = generation;
    }
}

#[cfg(test)]
mod tests {
    use murk_core::{EntityId, EntityManifest, IngressError, PropertyIndex};
    use proptest::prelude::*;

    use super::*;

    fn test_manifest() -> EntityManifest {
        EntityManifest {
            property_names: vec!["alive".into(), "hp".into(), "x".into(), "y".into()],
            property_defaults: vec![1.0, 100.0, 0.0, 0.0],
            alive_property: PropertyIndex(0),
        }
    }

    fn test_store() -> EntityStore {
        EntityStore::new(8, test_manifest())
    }

    #[test]
    fn spawn_returns_unique_ids_and_properties() {
        let mut store = test_store();
        let id0 = store
            .spawn(vec![0, 0].into(), 2, &[(PropertyIndex(1), 50.0)])
            .unwrap();
        let id1 = store.spawn(vec![1, 0].into(), 3, &[]).unwrap();

        assert_ne!(id0, id1);
        assert_eq!(id0.slot(), 0);
        assert_eq!(id1.slot(), 1);
        assert_eq!(store.get(id0).unwrap().entity_type, 2);
        assert_eq!(store.property(id0, PropertyIndex(0)), Some(1.0));
        assert_eq!(store.property(id0, PropertyIndex(1)), Some(50.0));
        assert_eq!(store.alive_count(), 2);
    }

    #[test]
    fn spawn_at_capacity_returns_error() {
        let mut store = EntityStore::new(2, test_manifest());
        store.spawn(vec![0].into(), 0, &[]).unwrap();
        store.spawn(vec![1].into(), 0, &[]).unwrap();

        assert_eq!(
            store.spawn(vec![2].into(), 0, &[]),
            Err(IngressError::EntityCapacityFull)
        );
    }

    #[test]
    fn despawn_recycles_slot_with_incremented_generation() {
        let mut store = test_store();
        let id0 = store.spawn(vec![0].into(), 0, &[]).unwrap();

        store.despawn(id0).unwrap();
        let id1 = store.spawn(vec![1].into(), 0, &[]).unwrap();

        assert_eq!(id1.slot(), id0.slot());
        assert_eq!(id1.generation(), id0.generation() + 1);
        assert_eq!(store.get(id0), None);
    }

    #[test]
    fn generation_wrap_retires_slot_instead_of_recycling() {
        let mut store = EntityStore::new(2, test_manifest());
        let id0 = store.spawn(vec![0].into(), 0, &[]).unwrap();
        store.despawn(id0).unwrap();

        store.set_generation_for_test(id0.slot(), 0xFFF);
        let id1 = store.spawn(vec![1].into(), 0, &[]).unwrap();
        assert_eq!(id1.slot(), id0.slot());
        assert_eq!(id1.generation(), 0xFFF);

        store.despawn(id1).unwrap();
        let id2 = store.spawn(vec![2].into(), 0, &[]).unwrap();
        assert_ne!(id2.slot(), id0.slot());
        assert_eq!(store.get(EntityId::new(id0.slot(), 0)), None);
    }

    #[test]
    fn stale_id_returns_unknown_entity() {
        let mut store = test_store();
        let old_id = store.spawn(vec![0].into(), 0, &[]).unwrap();
        store.despawn(old_id).unwrap();
        let _new_id = store.spawn(vec![1].into(), 0, &[]).unwrap();

        assert_eq!(store.despawn(old_id), Err(IngressError::UnknownEntity));
        assert_eq!(
            store.move_entity(old_id, vec![2].into()),
            Err(IngressError::UnknownEntity)
        );
        assert_eq!(store.property(old_id, PropertyIndex(0)), None);
    }

    #[test]
    fn move_updates_coord_index() {
        let mut store = test_store();
        let id = store.spawn(vec![0, 0].into(), 0, &[]).unwrap();

        assert_eq!(store.at_coord(&vec![0, 0].into()), &[id]);
        store.move_entity(id, vec![5, 5].into()).unwrap();

        assert!(store.at_coord(&vec![0, 0].into()).is_empty());
        assert_eq!(store.at_coord(&vec![5, 5].into()), &[id]);
        assert_eq!(store.get(id).unwrap().coord.as_slice(), &[5, 5]);
    }

    #[test]
    fn snapshot_and_restore_rollback() {
        let mut store = test_store();
        let id = store.spawn(vec![0].into(), 0, &[]).unwrap();
        let snap = store.snapshot_for_rollback();

        store.spawn(vec![1].into(), 1, &[]).unwrap();
        store.despawn(id).unwrap();
        assert_eq!(store.alive_count(), 1);

        store.restore_from_snapshot(snap);
        assert_eq!(store.alive_count(), 1);
        assert!(store.get(id).is_some());
        assert_eq!(store.at_coord(&vec![0].into()), &[id]);
    }

    proptest! {
        #[test]
        fn proptest_spawn_despawn_sequence_tracks_alive_count(
            ops in proptest::collection::vec(any::<bool>(), 0..128),
        ) {
            let capacity = 16_usize;
            let mut store = EntityStore::new(capacity as u32, test_manifest());
            let mut live = Vec::new();

            for op in ops {
                if op && live.len() < capacity {
                    let slot_hint = live.len() as i32;
                    let id = store
                        .spawn(vec![slot_hint].into(), 0, &[(PropertyIndex(1), 25.0)])
                        .unwrap();
                    live.push(id);
                } else if !op {
                    if let Some(id) = live.pop() {
                        store.despawn(id).unwrap();
                        prop_assert_eq!(store.get(id), None);
                    }
                } else {
                    prop_assert_eq!(
                        store.spawn(vec![capacity as i32].into(), 0, &[]),
                        Err(IngressError::EntityCapacityFull),
                    );
                }

                prop_assert_eq!(store.alive_count(), live.len());
                for id in &live {
                    prop_assert!(store.get(*id).is_some());
                    prop_assert_eq!(store.property(*id, PropertyIndex(1)), Some(25.0));
                }
            }
        }
    }
}
