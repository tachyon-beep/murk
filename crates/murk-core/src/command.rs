//! Command, command payload, and receipt types for the ingress pipeline.

use crate::error::IngressError;
use crate::id::{Coord, EntityId, FieldId, ParameterKey, PropertyIndex, TickId};

/// A command submitted to the simulation via the ingress pipeline.
///
/// Commands are ordered by `priority_class` (lower = higher priority),
/// then by `source_id` for disambiguation, then by `source_seq` for
/// per-source sequencing, then by `arrival_seq` as a final tiebreaker.
///
/// # Examples
///
/// ```
/// use murk_core::{Command, CommandPayload, FieldId, TickId, ParameterKey};
///
/// // A command that sets a global parameter.
/// let cmd = Command {
///     payload: CommandPayload::SetParameter {
///         key: ParameterKey(0),
///         value: 2.5,
///     },
///     expires_after_tick: TickId(100),
///     source_id: Some(1),
///     source_seq: Some(0),
///     priority_class: 1,
///     arrival_seq: 0,
/// };
///
/// assert_eq!(cmd.priority_class, 1);
/// assert_eq!(cmd.expires_after_tick, TickId(100));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    /// The operation to perform.
    pub payload: CommandPayload,
    /// The command expires if not applied by this tick.
    pub expires_after_tick: TickId,
    /// Optional source identifier for deduplication and ordering.
    pub source_id: Option<u64>,
    /// Optional per-source sequence number for ordering.
    pub source_seq: Option<u64>,
    /// Priority class. Lower values = higher priority.
    /// 0 = system, 1 = user default.
    pub priority_class: u8,
    /// Monotonic arrival sequence number, set by the ingress pipeline.
    pub arrival_seq: u64,
}

/// All command payloads.
///
/// `WorldEvent` variants affect per-cell state; `GlobalParameter` variants
/// affect simulation-wide scalar parameters.
///
/// # Examples
///
/// ```
/// use murk_core::{CommandPayload, FieldId, ParameterKey};
///
/// // Set a single field value at a coordinate.
/// let coord: murk_core::Coord = vec![3i32, 7].into();
/// let payload = CommandPayload::SetField {
///     coord,
///     field_id: FieldId(0),
///     value: 42.0,
/// };
///
/// // Batch-set multiple global parameters atomically.
/// let batch = CommandPayload::SetParameterBatch {
///     params: vec![(ParameterKey(0), 1.0), (ParameterKey(1), 0.5)],
/// };
///
/// assert!(matches!(payload, CommandPayload::SetField { .. }));
/// assert!(matches!(batch, CommandPayload::SetParameterBatch { .. }));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub enum CommandPayload {
    // --- WorldEvent variants ---
    /// Move an entity to a target coordinate.
    ///
    /// Rejected if `entity_id` is unknown or `target_coord` is out of bounds.
    Move {
        /// The entity to move.
        entity_id: EntityId,
        /// The destination coordinate.
        target_coord: Coord,
    },
    /// Spawn a new entity at a coordinate with initial property values.
    Spawn {
        /// The spawn location.
        coord: Coord,
        /// User-defined entity type tag.
        entity_type: u32,
        /// Initial property overrides for the new entity.
        property_overrides: Vec<(PropertyIndex, f32)>,
    },
    /// Remove an entity. Associated properties are cleared at the next tick.
    Despawn {
        /// The entity to remove.
        entity_id: EntityId,
    },
    /// Set a single field value at a coordinate. Primarily for `Sparse` fields.
    SetField {
        /// The target cell coordinate.
        coord: Coord,
        /// The field to modify.
        field_id: FieldId,
        /// The new value.
        value: f32,
    },
    /// Extension point for domain-specific commands.
    Custom {
        /// User-registered type identifier.
        type_id: u32,
        /// Opaque payload data.
        data: Vec<u8>,
    },

    // --- GlobalParameter variants ---
    /// Set a single global parameter. Takes effect at the next tick boundary.
    SetParameter {
        /// The parameter to set.
        key: ParameterKey,
        /// The new value.
        value: f64,
    },
    /// Batch-set multiple parameters atomically.
    SetParameterBatch {
        /// The parameters to set.
        params: Vec<(ParameterKey, f64)>,
    },
}

/// Receipt returned for each command in a submitted batch.
///
/// Indicates whether the command was accepted and, if applied,
/// which tick it was applied in.
///
/// # Examples
///
/// ```
/// use murk_core::command::Receipt;
/// use murk_core::TickId;
///
/// let receipt = Receipt {
///     accepted: true,
///     applied_tick_id: Some(TickId(5)),
///     reason_code: None,
///     command_index: 0,
/// };
///
/// assert!(receipt.accepted);
/// assert_eq!(receipt.applied_tick_id, Some(TickId(5)));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Receipt {
    /// Whether the command was accepted by the ingress pipeline.
    pub accepted: bool,
    /// The tick at which the command was applied, if applicable.
    pub applied_tick_id: Option<TickId>,
    /// The reason the command was rejected, if applicable.
    pub reason_code: Option<IngressError>,
    /// Index of this command within the submitted batch.
    pub command_index: usize,
}

#[cfg(test)]
mod entity_command_tests {
    use super::*;

    #[test]
    fn move_and_despawn_use_entity_id() {
        let id = EntityId::new(12, 3);
        let move_payload = CommandPayload::Move {
            entity_id: id,
            target_coord: vec![1, 2].into(),
        };
        let despawn_payload = CommandPayload::Despawn { entity_id: id };

        assert!(matches!(
            move_payload,
            CommandPayload::Move {
                entity_id,
                ..
            } if entity_id == id
        ));
        assert_eq!(despawn_payload, CommandPayload::Despawn { entity_id: id });
    }

    #[test]
    fn spawn_carries_entity_type_and_property_overrides() {
        let payload = CommandPayload::Spawn {
            coord: vec![3, 4].into(),
            entity_type: 7,
            property_overrides: vec![(PropertyIndex(1), 42.0)],
        };

        match payload {
            CommandPayload::Spawn {
                entity_type,
                property_overrides,
                ..
            } => {
                assert_eq!(entity_type, 7);
                assert_eq!(property_overrides, vec![(PropertyIndex(1), 42.0)]);
            }
            _ => panic!("expected spawn"),
        }
    }
}
