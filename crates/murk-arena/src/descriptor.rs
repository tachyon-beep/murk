//! Field descriptor mapping: `FieldId` → `(FieldHandle, metadata)`.
//!
//! The [`FieldDescriptor`] is the central metadata table for the arena. It
//! maps each registered `FieldId` to its current [`FieldHandle`] (physical
//! location) and [`FieldMeta`] (type information). Two descriptors exist
//! per `PingPongArena` — one for the published generation, one for staging.
//! They are swapped on publish.

use indexmap::IndexMap;
use murk_core::{FieldDef, FieldId, FieldMutability};

use crate::error::ArenaError;
use crate::handle::{FieldHandle, FieldLocation};

/// Metadata about a field's type and allocation requirements.
///
/// Pre-computed at arena construction from `FieldDef`. Stored separately
/// from segments to avoid double-borrow issues when `FieldWriter::write()`
/// needs to look up metadata and then mutate segment data.
#[derive(Clone, Debug)]
pub struct FieldMeta {
    /// Number of f32 components per cell for this field.
    pub components: u32,
    /// Allocation strategy.
    pub mutability: FieldMutability,
    /// Total allocation size: `cell_count * components`.
    pub total_len: u32,
    /// Human-readable name (for diagnostics).
    pub name: String,
}

/// A single entry in the descriptor table.
#[derive(Clone, Debug)]
pub struct FieldEntry {
    /// Physical location of this field's data.
    pub handle: FieldHandle,
    /// Type metadata (immutable after construction).
    pub meta: FieldMeta,
}

/// Maps `FieldId` to its current physical location and metadata.
///
/// The descriptor is the "phone book" of the arena — every field resolve
/// starts with a descriptor lookup. It uses `IndexMap` (not `HashMap`) for
/// deterministic iteration order, which matters for the pre-allocation
/// loop at `begin_tick()`.
#[derive(Clone, Debug)]
pub struct FieldDescriptor {
    entries: IndexMap<FieldId, FieldEntry>,
}

impl FieldDescriptor {
    /// Build a descriptor from field definitions and a cell count.
    ///
    /// All handles are initialised with generation 0 and placeholder locations.
    /// The caller (PingPongArena) must call [`FieldDescriptor::update_handle`] after allocating
    /// actual storage.
    pub fn from_field_defs(field_defs: &[(FieldId, FieldDef)], cell_count: u32) -> Result<Self, ArenaError> {
        let mut entries = IndexMap::with_capacity(field_defs.len());
        for (id, def) in field_defs {
            let components = def.field_type.components();
            let total_len = cell_count.checked_mul(components).ok_or(ArenaError::InvalidConfig {
                reason: format!(
                    "cell_count ({cell_count}) * components ({components}) overflows u32 for field '{}'",
                    def.name,
                ),
            })?;
            let meta = FieldMeta {
                components,
                mutability: def.mutability,
                total_len,
                name: def.name.clone(),
            };
            let handle =
                FieldHandle::new(0, 0, total_len, FieldLocation::PerTick { segment_index: 0 });
            entries.insert(*id, FieldEntry { handle, meta });
        }
        Ok(Self { entries })
    }

    /// Look up a field's entry.
    pub fn get(&self, field: FieldId) -> Option<&FieldEntry> {
        self.entries.get(&field)
    }

    /// Update the handle for a field (after allocation or ping-pong swap).
    pub fn update_handle(&mut self, field: FieldId, handle: FieldHandle) {
        if let Some(entry) = self.entries.get_mut(&field) {
            entry.handle = handle;
        }
    }

    /// Iterate over all entries in registration order.
    pub fn iter(&self) -> impl Iterator<Item = (&FieldId, &FieldEntry)> {
        self.entries.iter()
    }

    /// Iterate over all entries mutably.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&FieldId, &mut FieldEntry)> {
        self.entries.iter_mut()
    }

    /// Number of registered fields.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are no registered fields.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate over fields filtered by mutability class.
    pub fn fields_by_mutability(
        &self,
        mutability: FieldMutability,
    ) -> impl Iterator<Item = (&FieldId, &FieldEntry)> {
        self.entries
            .iter()
            .filter(move |(_, entry)| entry.meta.mutability == mutability)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::{BoundaryBehavior, FieldType};

    fn make_field_defs() -> Vec<(FieldId, FieldDef)> {
        vec![
            (
                FieldId(0),
                FieldDef {
                    name: "temperature".to_string(),
                    field_type: FieldType::Scalar,
                    mutability: FieldMutability::PerTick,
                    units: None,
                    bounds: None,
                    boundary_behavior: BoundaryBehavior::Clamp,
                },
            ),
            (
                FieldId(1),
                FieldDef {
                    name: "velocity".to_string(),
                    field_type: FieldType::Vector { dims: 3 },
                    mutability: FieldMutability::PerTick,
                    units: None,
                    bounds: None,
                    boundary_behavior: BoundaryBehavior::Clamp,
                },
            ),
            (
                FieldId(2),
                FieldDef {
                    name: "terrain".to_string(),
                    field_type: FieldType::Categorical { n_values: 4 },
                    mutability: FieldMutability::Static,
                    units: None,
                    bounds: None,
                    boundary_behavior: BoundaryBehavior::Clamp,
                },
            ),
            (
                FieldId(3),
                FieldDef {
                    name: "resources".to_string(),
                    field_type: FieldType::Scalar,
                    mutability: FieldMutability::Sparse,
                    units: None,
                    bounds: Some((0.0, 100.0)),
                    boundary_behavior: BoundaryBehavior::Clamp,
                },
            ),
        ]
    }

    #[test]
    fn from_field_defs_creates_entries() {
        let defs = make_field_defs();
        let desc = FieldDescriptor::from_field_defs(&defs, 100).unwrap();
        assert_eq!(desc.len(), 4);
    }

    #[test]
    fn total_len_is_cell_count_times_components() {
        let defs = make_field_defs();
        let desc = FieldDescriptor::from_field_defs(&defs, 100).unwrap();

        // Scalar: 100 * 1 = 100
        assert_eq!(desc.get(FieldId(0)).unwrap().meta.total_len, 100);
        // Vector(3): 100 * 3 = 300
        assert_eq!(desc.get(FieldId(1)).unwrap().meta.total_len, 300);
        // Categorical: 100 * 1 = 100
        assert_eq!(desc.get(FieldId(2)).unwrap().meta.total_len, 100);
    }

    #[test]
    fn update_handle_changes_handle() {
        let defs = make_field_defs();
        let mut desc = FieldDescriptor::from_field_defs(&defs, 100).unwrap();

        let new_handle =
            FieldHandle::new(5, 1024, 100, FieldLocation::PerTick { segment_index: 2 });
        desc.update_handle(FieldId(0), new_handle);

        let entry = desc.get(FieldId(0)).unwrap();
        assert_eq!(entry.handle.generation(), 5);
        assert_eq!(
            entry.handle.location(),
            FieldLocation::PerTick { segment_index: 2 }
        );
    }

    #[test]
    fn fields_by_mutability_filters() {
        let defs = make_field_defs();
        let desc = FieldDescriptor::from_field_defs(&defs, 100).unwrap();

        let per_tick: Vec<_> = desc
            .fields_by_mutability(FieldMutability::PerTick)
            .collect();
        assert_eq!(per_tick.len(), 2);

        let static_fields: Vec<_> = desc.fields_by_mutability(FieldMutability::Static).collect();
        assert_eq!(static_fields.len(), 1);

        let sparse: Vec<_> = desc.fields_by_mutability(FieldMutability::Sparse).collect();
        assert_eq!(sparse.len(), 1);
    }

    #[test]
    fn unknown_field_returns_none() {
        let defs = make_field_defs();
        let desc = FieldDescriptor::from_field_defs(&defs, 100).unwrap();
        assert!(desc.get(FieldId(99)).is_none());
    }

    #[test]
    fn overflow_cell_count_times_components_returns_error() {
        let defs = vec![(
            FieldId(0),
            FieldDef {
                name: "huge".to_string(),
                field_type: FieldType::Vector { dims: u32::MAX },
                mutability: FieldMutability::PerTick,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            },
        )];
        let result = FieldDescriptor::from_field_defs(&defs, u32::MAX);
        assert!(matches!(result, Err(ArenaError::InvalidConfig { .. })));
    }

    #[cfg(not(miri))]
    mod proptests {
        use super::*;
        use murk_core::{BoundaryBehavior, FieldType};
        use proptest::prelude::*;

        fn arb_field_type() -> impl Strategy<Value = FieldType> {
            prop_oneof![
                Just(FieldType::Scalar),
                (2u32..8).prop_map(|d| FieldType::Vector { dims: d }),
                (2u32..16).prop_map(|n| FieldType::Categorical { n_values: n }),
            ]
        }

        proptest! {
            #[test]
            fn total_len_equals_cell_count_times_components(
                cell_count in 1u32..1000,
                field_type in arb_field_type(),
            ) {
                let components = field_type.components();
                let defs = vec![(
                    FieldId(0),
                    FieldDef {
                        name: "f".into(),
                        field_type,
                        mutability: FieldMutability::PerTick,
                        units: None,
                        bounds: None,
                        boundary_behavior: BoundaryBehavior::Clamp,
                    },
                )];
                let desc = FieldDescriptor::from_field_defs(&defs, cell_count).unwrap();
                let entry = desc.get(FieldId(0)).unwrap();
                prop_assert_eq!(
                    entry.meta.total_len,
                    cell_count * components
                );
            }

            #[test]
            fn len_equals_number_of_field_defs(
                n_fields in 1usize..20,
            ) {
                let defs: Vec<_> = (0..n_fields)
                    .map(|i| (
                        FieldId(i as u32),
                        FieldDef {
                            name: format!("f{i}"),
                            field_type: FieldType::Scalar,
                            mutability: FieldMutability::PerTick,
                            units: None,
                            bounds: None,
                            boundary_behavior: BoundaryBehavior::Clamp,
                        },
                    ))
                    .collect();
                let desc = FieldDescriptor::from_field_defs(&defs, 100).unwrap();
                prop_assert_eq!(desc.len(), n_fields);
                prop_assert_eq!(desc.is_empty(), n_fields == 0);
            }
        }
    }
}
