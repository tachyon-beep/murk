//! Error types for space operations.

use murk_core::Coord;
use std::fmt;

/// Errors arising from space construction or spatial queries.
#[derive(Debug, Clone)]
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
        }
    }
}

impl std::error::Error for SpaceError {}
