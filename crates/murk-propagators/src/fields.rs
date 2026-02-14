//! Field constants and definitions for the reference propagator pipeline.

use murk_core::{BoundaryBehavior, FieldDef, FieldId, FieldMutability, FieldType};

/// Heat scalar field (1 component/cell).
pub const HEAT: FieldId = FieldId(0);
/// Velocity vector field (2 components/cell).
pub const VELOCITY: FieldId = FieldId(1);
/// Agent presence scalar field (1 component/cell).
pub const AGENT_PRESENCE: FieldId = FieldId(2);
/// Heat gradient vector field (2 components/cell).
pub const HEAT_GRADIENT: FieldId = FieldId(3);
/// Reward scalar field (1 component/cell).
pub const REWARD: FieldId = FieldId(4);

/// Returns the 5 field definitions for the reference pipeline in order.
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
            bounds: Some((0.0, 1.0)),
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
}
