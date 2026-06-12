//! Core types and traits for the Murk simulation framework.
//!
//! This is the leaf crate with zero internal Murk dependencies. It defines
//! the fundamental abstractions used throughout the Murk workspace:
//! type IDs, field descriptors, error types, and core traits.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]

pub mod command;
pub mod entity;
pub mod error;
pub mod field;
pub mod id;
pub mod traits;

// Re-export core types at crate root for convenience.
pub use command::{Command, CommandPayload, Receipt};
pub use entity::EntityManifest;
pub use error::{IngressError, ObsError, PropagatorError, StepError};
pub use field::{BoundaryBehavior, FieldDef, FieldMutability, FieldSet, FieldSetIter, FieldType};
pub use id::{
    Coord, EntityId, FieldId, ParameterKey, ParameterVersion, PropertyIndex, SpaceId,
    SpaceInstanceId, TickId, WorldGenerationId,
};
pub use traits::{FieldReader, FieldWriter, SnapshotAccess};
