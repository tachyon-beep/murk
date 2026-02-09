//! Test utilities and mock types for Murk development.
//!
//! Provides mock implementations of core traits ([`FieldReader`],
//! [`FieldWriter`], [`SnapshotAccess`]) and a [`TestWorldBuilder`]
//! scaffold for constructing test scenarios.

#![forbid(unsafe_code)]
#![allow(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

use std::collections::HashMap;

use murk_core::{
    FieldId, FieldReader, FieldWriter, ParameterVersion, SnapshotAccess, TickId, WorldGenerationId,
};

/// Builder for constructing test worlds with preconfigured state.
///
/// Currently a scaffold — full implementation arrives when `WorldConfig`
/// is defined in a later work package.
pub struct TestWorldBuilder;

impl TestWorldBuilder {
    pub fn new() -> Self {
        TestWorldBuilder
    }
}

impl Default for TestWorldBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Mock implementation of [`FieldReader`].
///
/// Backed by a `HashMap<FieldId, Vec<f32>>` for flexible test setup.
/// Pre-populate fields with [`set_field`](MockFieldReader::set_field)
/// before passing to code under test.
pub struct MockFieldReader {
    fields: HashMap<FieldId, Vec<f32>>,
}

impl MockFieldReader {
    pub fn new() -> Self {
        Self {
            fields: HashMap::new(),
        }
    }

    /// Pre-populate a field with data for testing.
    pub fn set_field(&mut self, field: FieldId, data: Vec<f32>) {
        self.fields.insert(field, data);
    }
}

impl Default for MockFieldReader {
    fn default() -> Self {
        Self::new()
    }
}

impl FieldReader for MockFieldReader {
    fn read(&self, field: FieldId) -> Option<&[f32]> {
        self.fields.get(&field).map(|v| v.as_slice())
    }
}

/// Mock implementation of [`FieldWriter`].
///
/// Backed by a `HashMap<FieldId, Vec<f32>>` for flexible test setup.
/// Pre-allocate field buffers with [`add_field`](MockFieldWriter::add_field),
/// then pass to code under test. Inspect results with
/// [`get_field`](MockFieldWriter::get_field).
pub struct MockFieldWriter {
    fields: HashMap<FieldId, Vec<f32>>,
}

impl MockFieldWriter {
    pub fn new() -> Self {
        Self {
            fields: HashMap::new(),
        }
    }

    /// Pre-allocate a field buffer with the given size, initialized to zero.
    pub fn add_field(&mut self, field: FieldId, size: usize) {
        self.fields.insert(field, vec![0.0; size]);
    }

    /// Read back the current field data for test assertions.
    pub fn get_field(&self, field: FieldId) -> Option<&[f32]> {
        self.fields.get(&field).map(|v| v.as_slice())
    }
}

impl Default for MockFieldWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl FieldWriter for MockFieldWriter {
    fn write(&mut self, field: FieldId) -> Option<&mut [f32]> {
        self.fields.get_mut(&field).map(|v| v.as_mut_slice())
    }
}

/// Mock snapshot implementing [`SnapshotAccess`].
///
/// Backed by a `HashMap<FieldId, Vec<f32>>` with configurable
/// tick, generation, and parameter version metadata.
pub struct MockSnapshot {
    fields: HashMap<FieldId, Vec<f32>>,
    tick: TickId,
    world_gen: WorldGenerationId,
    param_ver: ParameterVersion,
}

impl MockSnapshot {
    /// Create a new mock snapshot with the given metadata.
    pub fn new(tick: TickId, world_gen: WorldGenerationId, param_ver: ParameterVersion) -> Self {
        Self {
            fields: HashMap::new(),
            tick,
            world_gen,
            param_ver,
        }
    }

    /// Pre-populate a field with data for testing.
    pub fn set_field(&mut self, field: FieldId, data: Vec<f32>) {
        self.fields.insert(field, data);
    }

    /// Returns the number of fields in the snapshot.
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }
}

impl SnapshotAccess for MockSnapshot {
    fn read_field(&self, field: FieldId) -> Option<&[f32]> {
        self.fields.get(&field).map(|v| v.as_slice())
    }

    fn tick_id(&self) -> TickId {
        self.tick
    }

    fn world_generation_id(&self) -> WorldGenerationId {
        self.world_gen
    }

    fn parameter_version(&self) -> ParameterVersion {
        self.param_ver
    }
}
