//! Arena-specific error types.

use std::error::Error;
use std::fmt;

use murk_core::FieldId;

/// Errors that can occur during arena operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArenaError {
    /// Segment pool is full — no more segments can be allocated.
    CapacityExceeded {
        /// Number of bytes requested.
        requested: usize,
        /// Total capacity available across all segments.
        capacity: usize,
    },
    /// A `FieldHandle` from a generation that has been reclaimed.
    StaleHandle {
        /// The generation encoded in the handle.
        handle_generation: u32,
        /// The oldest live generation in the arena.
        oldest_live: u32,
    },
    /// A `FieldId` that is not registered in the arena.
    UnknownField {
        /// The unrecognised field.
        field: FieldId,
    },
    /// Attempted to write a field whose mutability does not permit writes
    /// in the current context (e.g. writing a `Static` field after init).
    NotWritable {
        /// The field that was not writable.
        field: FieldId,
    },
}

impl fmt::Display for ArenaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CapacityExceeded {
                requested,
                capacity,
            } => {
                write!(
                    f,
                    "arena capacity exceeded: requested {requested} bytes, capacity {capacity} bytes"
                )
            }
            Self::StaleHandle {
                handle_generation,
                oldest_live,
            } => {
                write!(
                    f,
                    "stale handle: generation {handle_generation}, oldest live {oldest_live}"
                )
            }
            Self::UnknownField { field } => {
                write!(f, "unknown field: {field}")
            }
            Self::NotWritable { field } => {
                write!(f, "field {field} is not writable in this context")
            }
        }
    }
}

impl Error for ArenaError {}
