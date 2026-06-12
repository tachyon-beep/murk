use murk_core::{Coord, EntityId, EntityManifest, PropertyIndex};
use murk_entity::{EntityOverlayReader, EntityStore, PropertyStaging};

fn manifest() -> EntityManifest {
    EntityManifest {
        property_names: vec!["alive".into(), "hp".into(), "speed".into()],
        property_defaults: vec![1.0, 100.0, 1.0],
        alive_property: PropertyIndex(0),
    }
}

fn coord(values: &[i32]) -> Coord {
    values.iter().copied().collect()
}

#[test]
fn full_pipeline_stages_and_commits_properties() {
    let mut store = EntityStore::new(4, manifest());
    let id = store.spawn(coord(&[0, 0]), 7, &[]).unwrap();
    let mut staging =
        PropertyStaging::new(store.capacity(), store.manifest().property_count() as u32);

    {
        let snapshot = store.snapshot();
        let overlay = EntityOverlayReader::new(snapshot, &staging);
        assert_eq!(overlay.property(id, PropertyIndex(1)), Some(100.0));
    }

    assert!(staging.set(id, PropertyIndex(1), 25.0));

    {
        let snapshot = store.snapshot();
        let overlay = EntityOverlayReader::new(snapshot, &staging);
        assert_eq!(overlay.property(id, PropertyIndex(1)), Some(25.0));
        assert_eq!(snapshot.property(id, PropertyIndex(1)), Some(100.0));
    }

    store.apply_staged_properties(&staging);
    staging.reset();

    let snapshot = store.snapshot();
    let overlay = EntityOverlayReader::new(snapshot, &staging);
    assert_eq!(overlay.property(id, PropertyIndex(1)), Some(25.0));
    assert_eq!(staging.get(id, PropertyIndex(1)), None);
}

#[test]
fn rollback_restores_coord_index_and_entity_state() {
    let mut store = EntityStore::new(4, manifest());
    let id0 = store.spawn(coord(&[0, 0]), 0, &[]).unwrap();
    let id1 = store.spawn(coord(&[1, 0]), 0, &[]).unwrap();
    let rollback = store.snapshot_for_rollback();

    store.move_entity(id0, coord(&[2, 0])).unwrap();
    store.despawn(id1).unwrap();
    let id2 = store.spawn(coord(&[3, 0]), 0, &[]).unwrap();

    assert_eq!(store.at_coord(&coord(&[2, 0])), &[id0]);
    assert!(store.at_coord(&coord(&[1, 0])).is_empty());
    assert_eq!(store.alive_count(), 2);

    store.restore_from_snapshot(rollback);

    assert_eq!(store.at_coord(&coord(&[0, 0])), &[id0]);
    assert_eq!(store.at_coord(&coord(&[1, 0])), &[id1]);
    assert!(store.at_coord(&coord(&[2, 0])).is_empty());
    assert!(store.at_coord(&coord(&[3, 0])).is_empty());
    assert!(store.get(id2).is_none());
    assert_eq!(store.alive_count(), 2);
}

#[test]
fn overlay_supports_multi_entity_spatial_queries() {
    let mut store = EntityStore::new(6, manifest());
    let id0 = store.spawn(coord(&[4, 1]), 10, &[]).unwrap();
    let id1 = store.spawn(coord(&[4, 1]), 11, &[]).unwrap();
    let id2 = store.spawn(coord(&[5, 1]), 12, &[]).unwrap();
    let mut staging =
        PropertyStaging::new(store.capacity(), store.manifest().property_count() as u32);
    assert!(staging.set(id1, PropertyIndex(1), 60.0));

    let snapshot = store.snapshot();
    let overlay = EntityOverlayReader::new(snapshot, &staging);
    let colocated: Vec<EntityId> = overlay
        .iter_alive()
        .filter(|record| record.coord.as_slice() == [4, 1])
        .map(|record| record.id)
        .collect();

    assert_eq!(colocated, vec![id0, id1]);
    assert_eq!(overlay.get(id2).unwrap().coord.as_slice(), &[5, 1]);
    assert_eq!(overlay.property(id0, PropertyIndex(1)), Some(100.0));
    assert_eq!(overlay.property(id1, PropertyIndex(1)), Some(60.0));
}

#[test]
fn overlay_rejects_stale_id_through_recycled_slot_pipeline() {
    let mut store = EntityStore::new(2, manifest());
    let old = store.spawn(coord(&[0]), 0, &[]).unwrap();
    store.despawn(old).unwrap();
    let new_id = store.spawn(coord(&[1]), 0, &[]).unwrap();
    assert_eq!(old.slot(), new_id.slot());

    let mut staging =
        PropertyStaging::new(store.capacity(), store.manifest().property_count() as u32);
    assert!(staging.set(new_id, PropertyIndex(1), 5.0));

    let snapshot = store.snapshot();
    let overlay = EntityOverlayReader::new(snapshot, &staging);

    assert!(overlay.get(old).is_none());
    assert_eq!(overlay.property(old, PropertyIndex(1)), None);
    assert_eq!(overlay.property(new_id, PropertyIndex(1)), Some(5.0));
}
