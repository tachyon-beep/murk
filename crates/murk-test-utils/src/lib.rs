//! Test utilities and mock types for Murk development.
//!
//! Provides scaffold structs (`TestWorldBuilder`, `MockFieldReader`,
//! `MockFieldWriter`, `MockSnapshot`) that will gain trait impls
//! once the corresponding traits are defined in WP-1.

#![forbid(unsafe_code)]
#![allow(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

/// Builder for constructing test worlds with preconfigured state.
///
/// Currently a scaffold — full implementation arrives in WP-1 when
/// core types are defined.
pub struct TestWorldBuilder;

impl TestWorldBuilder {
    /// Create a new test world builder.
    pub fn new() -> Self {
        TestWorldBuilder
    }
}

impl Default for TestWorldBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Mock implementation of field reading.
///
/// Will implement the `FieldReader` trait once it is defined in `murk-core`.
pub struct MockFieldReader;

impl MockFieldReader {
    /// Create a new mock field reader.
    pub fn new() -> Self {
        MockFieldReader
    }

    /// Read a field value by index.
    ///
    /// # Panics
    ///
    /// Always panics — this is a scaffold awaiting WP-1 trait definitions.
    pub fn read(&self, _index: usize) -> f32 {
        todo!("MockFieldReader::read awaits FieldReader trait from WP-1")
    }
}

impl Default for MockFieldReader {
    fn default() -> Self {
        Self::new()
    }
}

/// Mock implementation of field writing.
///
/// Will implement the `FieldWriter` trait once it is defined in `murk-core`.
pub struct MockFieldWriter;

impl MockFieldWriter {
    /// Create a new mock field writer.
    pub fn new() -> Self {
        MockFieldWriter
    }

    /// Write a field value by index.
    ///
    /// # Panics
    ///
    /// Always panics — this is a scaffold awaiting WP-1 trait definitions.
    pub fn write(&mut self, _index: usize, _value: f32) {
        todo!("MockFieldWriter::write awaits FieldWriter trait from WP-1")
    }
}

impl Default for MockFieldWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Mock snapshot backed by a flat `Vec<f32>`.
///
/// Will implement the `SnapshotAccess` trait once it is defined in `murk-core`.
pub struct MockSnapshot {
    data: Vec<f32>,
}

impl MockSnapshot {
    /// Create a new mock snapshot with the given size, initialized to zero.
    pub fn new(size: usize) -> Self {
        MockSnapshot {
            data: vec![0.0; size],
        }
    }

    /// Get the snapshot data as a slice.
    ///
    /// # Panics
    ///
    /// Always panics — this is a scaffold awaiting WP-1 trait definitions.
    pub fn as_slice(&self) -> &[f32] {
        todo!("MockSnapshot::as_slice awaits SnapshotAccess trait from WP-1")
    }

    /// Get the number of elements in the snapshot.
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns true if the snapshot is empty.
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
