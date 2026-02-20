//! Error types for space operations.

use murk_core::Coord;
use std::fmt;

/// Errors arising from space construction or spatial queries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceError {
    /// A coordinate is outside the bounds of the space.
    CoordOutOfBounds {
        /// The offending coordinate.
        coord: Coord,
        /// Human-readable description of the valid range.
        bounds: String,
    },
    /// A region specification is invalid for this space.
    InvalidRegion {
        /// What went wrong.
        reason: String,
    },
    /// Attempted to construct a space with zero cells.
    EmptySpace,
    /// A dimension exceeds the representable coordinate range.
    DimensionTooLarge {
        /// Which dimension is too large (e.g. "len", "rows", "cols").
        name: &'static str,
        /// The value that was provided.
        value: u32,
        /// The maximum allowed value.
        max: u32,
    },
    /// A space composition is invalid (e.g. empty component list, overflow).
    InvalidComposition {
        /// What went wrong.
        reason: String,
    },
}

impl fmt::Display for SpaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CoordOutOfBounds { coord, bounds } => {
                write!(f, "coordinate {coord:?} out of bounds: {bounds}")
            }
            Self::InvalidRegion { reason } => {
                write!(f, "invalid region: {reason}")
            }
            Self::EmptySpace => write!(f, "space must have at least one cell"),
            Self::DimensionTooLarge { name, value, max } => {
                write!(f, "{name} ({value}) exceeds maximum ({max})")
            }
            Self::InvalidComposition { reason } => {
                write!(f, "invalid composition: {reason}")
            }
        }
    }
}

impl std::error::Error for SpaceError {}
