//! Core abstraction traits for field access and snapshot reading.

use crate::id::{FieldId, ParameterVersion, TickId, WorldGenerationId};

/// Read-only access to field data within a simulation step.
///
/// Implemented by arena types to provide propagators with read access
/// to field buffers. Returns `None` if the field is not readable in
/// the current context.
pub trait FieldReader {
    /// Read the data for a field as a flat f32 slice.
    ///
    /// Returns `None` if the field ID is invalid or not readable.
    fn read(&self, field: FieldId) -> Option<&[f32]>;
}

/// Mutable access to field data within a simulation step.
///
/// Implemented by arena types to provide propagators with write access
/// to staging buffers. Returns `None` if the field is not writable in
/// the current context.
pub trait FieldWriter {
    /// Get a mutable slice for writing field data.
    ///
    /// Returns `None` if the field ID is invalid or not writable.
    fn write(&mut self, field: FieldId) -> Option<&mut [f32]>;
}

/// Read-only access to a published snapshot.
///
/// This trait decouples observation extraction (`ObsPlan`) from the
/// arena implementation. `ObsPlan` reads through `&dyn SnapshotAccess`
/// rather than referencing `ReadArena` directly (Decision N).
pub trait SnapshotAccess {
    /// Read field data from the snapshot.
    ///
    /// Returns `None` if the field ID is invalid or not present.
    fn read_field(&self, field: FieldId) -> Option<&[f32]>;

    /// The tick at which this snapshot was produced.
    fn tick_id(&self) -> TickId;

    /// The arena generation of this snapshot.
    fn world_generation_id(&self) -> WorldGenerationId;

    /// The parameter version at the time of this snapshot.
    fn parameter_version(&self) -> ParameterVersion;
}
