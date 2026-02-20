//! Field constants and definitions for the reference propagator pipeline.

use murk_core::{BoundaryBehavior, FieldDef, FieldId, FieldMutability, FieldType};

/// Heat scalar field (1 component/cell).
#[deprecated(since = "0.1.7", note = "use user-defined FieldId with ScalarDiffusion instead")]
pub const HEAT: FieldId = FieldId(0);
/// Velocity vector field (2 components/cell).
#[deprecated(since = "0.1.7", note = "use user-defined FieldId with ScalarDiffusion instead")]
pub const VELOCITY: FieldId = FieldId(1);
/// Agent presence scalar field (1 component/cell).
#[deprecated(since = "0.1.7", note = "use user-defined FieldId with AgentMovementPropagator instead")]
pub const AGENT_PRESENCE: FieldId = FieldId(2);
/// Heat gradient vector field (2 components/cell).
#[deprecated(since = "0.1.7", note = "use user-defined FieldId with GradientCompute instead")]
pub const HEAT_GRADIENT: FieldId = FieldId(3);
/// Reward scalar field (1 component/cell).
#[deprecated(since = "0.1.7", note = "use user-defined FieldId with RewardPropagator instead")]
pub const REWARD: FieldId = FieldId(4);

/// Returns the 5 field definitions for the reference pipeline in order.
#[deprecated(since = "0.1.7", note = "define fields in your own Config instead")]
pub fn reference_fields() -> Vec<FieldDef> {
    vec![
        FieldDef {
            name: "heat".to_string(),
            field_type: FieldType::Scalar,
            mutability: FieldMutability::PerTick,
            units: Some("kelvin".to_string()),
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        },
        FieldDef {
            name: "velocity".to_string(),
            field_type: FieldType::Vector { dims: 2 },
            mutability: FieldMutability::PerTick,
            units: Some("m/s".to_string()),
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        },
        FieldDef {
            name: "agent_presence".to_string(),
            field_type: FieldType::Scalar,
            mutability: FieldMutability::PerTick,
            units: None,
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        },
        FieldDef {
            name: "heat_gradient".to_string(),
            field_type: FieldType::Vector { dims: 2 },
            mutability: FieldMutability::PerTick,
            units: Some("kelvin/cell".to_string()),
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        },
        FieldDef {
            name: "reward".to_string(),
            field_type: FieldType::Scalar,
            mutability: FieldMutability::PerTick,
            units: None,
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        },
    ]
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;

    #[test]
    fn reference_fields_count_and_order() {
        let fields = reference_fields();
        assert_eq!(fields.len(), 5);
        assert_eq!(fields[0].name, "heat");
        assert_eq!(fields[1].name, "velocity");
        assert_eq!(fields[2].name, "agent_presence");
        assert_eq!(fields[3].name, "heat_gradient");
        assert_eq!(fields[4].name, "reward");
    }

    #[test]
    fn total_components() {
        let fields = reference_fields();
        let total: u32 = fields.iter().map(|f| f.field_type.components()).sum();
        assert_eq!(total, 7); // 1 + 2 + 1 + 2 + 1
    }

    #[test]
    fn agent_presence_bounds_accommodate_marker_values() {
        // AgentMovementPropagator writes (agent_id as f32) + 1.0 as markers.
        // Bounds must not restrict these values.
        let fields = reference_fields();
        let ap = &fields[2];
        assert_eq!(ap.name, "agent_presence");
        assert!(
            ap.bounds.is_none(),
            "agent_presence bounds should be None (markers exceed [0,1]), got {:?}",
            ap.bounds
        );
    }
}
