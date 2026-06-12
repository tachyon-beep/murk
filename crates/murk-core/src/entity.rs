//! Entity manifest configuration.

use crate::PropertyIndex;

/// Declares the homogeneous property schema for entities in a world.
///
/// Every entity in a world has the same property layout. The `alive_property`
/// is the single source of truth for liveness: stores stamp it to `1.0` on
/// spawn and `0.0` on despawn.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityManifest {
    /// Human-readable property names.
    pub property_names: Vec<String>,
    /// Default values applied to each property at spawn.
    pub property_defaults: Vec<f32>,
    /// Property index used as the alive flag.
    pub alive_property: PropertyIndex,
}

impl EntityManifest {
    /// Number of properties in the manifest.
    #[must_use]
    pub fn property_count(&self) -> usize {
        self.property_defaults.len()
    }

    /// Validate the manifest shape.
    pub fn validate(&self) -> Result<(), EntityManifestError> {
        if self.property_names.len() != self.property_defaults.len() {
            return Err(EntityManifestError::PropertyLengthMismatch {
                names: self.property_names.len(),
                defaults: self.property_defaults.len(),
            });
        }
        if self.property_defaults.is_empty() {
            return Err(EntityManifestError::EmptyProperties);
        }
        if self.alive_property.0 as usize >= self.property_defaults.len() {
            return Err(EntityManifestError::AlivePropertyOutOfRange {
                alive_property: self.alive_property,
                property_count: self.property_defaults.len(),
            });
        }
        Ok(())
    }
}

/// Validation errors for [`EntityManifest`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntityManifestError {
    /// Property name/default vectors have different lengths.
    PropertyLengthMismatch {
        /// Number of property names.
        names: usize,
        /// Number of default values.
        defaults: usize,
    },
    /// The manifest declares no properties.
    EmptyProperties,
    /// The alive property index points outside the property array.
    AlivePropertyOutOfRange {
        /// Configured alive property index.
        alive_property: PropertyIndex,
        /// Number of declared properties.
        property_count: usize,
    },
}

impl std::fmt::Display for EntityManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PropertyLengthMismatch { names, defaults } => write!(
                f,
                "entity property names/defaults length mismatch: {names} names, {defaults} defaults"
            ),
            Self::EmptyProperties => {
                write!(f, "entity manifest must declare at least one property")
            }
            Self::AlivePropertyOutOfRange {
                alive_property,
                property_count,
            } => write!(
                f,
                "alive property {alive_property} is out of range for {property_count} properties"
            ),
        }
    }
}

impl std::error::Error for EntityManifestError {}

#[cfg(test)]
mod tests {
    use crate::{EntityManifest, PropertyIndex};

    #[test]
    fn property_count_matches_defaults() {
        let manifest = EntityManifest {
            property_names: vec!["alive".into(), "hp".into()],
            property_defaults: vec![1.0, 100.0],
            alive_property: PropertyIndex(0),
        };

        assert_eq!(manifest.property_count(), 2);
        assert_eq!(manifest.validate(), Ok(()));
    }

    #[test]
    fn validate_rejects_length_mismatch() {
        let manifest = EntityManifest {
            property_names: vec!["alive".into()],
            property_defaults: vec![1.0, 100.0],
            alive_property: PropertyIndex(0),
        };

        assert!(manifest.validate().is_err());
    }

    #[test]
    fn validate_rejects_alive_property_out_of_range() {
        let manifest = EntityManifest {
            property_names: vec!["alive".into()],
            property_defaults: vec![1.0],
            alive_property: PropertyIndex(2),
        };

        assert!(manifest.validate().is_err());
    }
}
