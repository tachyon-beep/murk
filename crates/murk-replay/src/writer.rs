//! Replay recording writer.
//!
//! [`ReplayWriter`] streams frames to any `Write` sink, encoding the
//! binary replay format. The header is written immediately on construction.

use std::io::Write;

use murk_core::command::Command;
use murk_core::traits::SnapshotAccess;

use crate::codec::{encode_frame, encode_header, serialize_command};
use crate::error::ReplayError;
use crate::hash::snapshot_hash;
use crate::types::{BuildMetadata, Frame, InitDescriptor};

/// Writes replay data to a byte stream.
///
/// Generic over `W: Write` so tests can use `Vec<u8>` and production
/// code can use `BufWriter<File>`.
///
/// # Examples
///
/// ```
/// use murk_replay::{ReplayWriter, ReplayReader, BuildMetadata, InitDescriptor, Frame};
///
/// let meta = BuildMetadata {
///     toolchain: "test".into(),
///     target_triple: "test".into(),
///     murk_version: "0.1.0".into(),
///     compile_flags: "test".into(),
/// };
/// let init = InitDescriptor {
///     seed: 42,
///     config_hash: 0,
///     field_count: 1,
///     cell_count: 10,
///     space_descriptor: vec![],
/// };
///
/// // Write two frames to an in-memory buffer.
/// let mut buf = Vec::new();
/// let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
/// for tick in 1..=2u64 {
///     let frame = Frame { tick_id: tick, commands: vec![], snapshot_hash: tick };
///     writer.write_raw_frame(&frame).unwrap();
/// }
/// assert_eq!(writer.frames_written(), 2);
/// drop(writer);
///
/// // Read them back.
/// let mut reader = ReplayReader::open(buf.as_slice()).unwrap();
/// assert_eq!(reader.metadata(), &meta);
/// let f1 = reader.next_frame().unwrap().unwrap();
/// assert_eq!(f1.tick_id, 1);
/// let f2 = reader.next_frame().unwrap().unwrap();
/// assert_eq!(f2.tick_id, 2);
/// assert!(reader.next_frame().unwrap().is_none());
/// ```
pub struct ReplayWriter<W: Write> {
    writer: W,
    field_count: u32,
    frames_written: u64,
}

impl<W: Write> ReplayWriter<W> {
    /// Create a new replay writer, immediately writing the header.
    pub fn new(
        mut writer: W,
        metadata: &BuildMetadata,
        init: &InitDescriptor,
    ) -> Result<Self, ReplayError> {
        encode_header(&mut writer, metadata, init)?;
        Ok(Self {
            writer,
            field_count: init.field_count,
            frames_written: 0,
        })
    }

    /// Record a frame: serialize commands, hash the snapshot, and write.
    pub fn write_frame(
        &mut self,
        tick_id: u64,
        commands: &[Command],
        snapshot: &dyn SnapshotAccess,
    ) -> Result<(), ReplayError> {
        let serialized_commands: Vec<_> = commands.iter().map(serialize_command).collect();
        let hash = snapshot_hash(snapshot, self.field_count);

        let frame = Frame {
            tick_id,
            commands: serialized_commands,
            snapshot_hash: hash,
        };
        encode_frame(&mut self.writer, &frame)?;
        self.frames_written += 1;
        Ok(())
    }

    /// Write a pre-built frame directly (useful for testing).
    pub fn write_raw_frame(&mut self, frame: &Frame) -> Result<(), ReplayError> {
        encode_frame(&mut self.writer, frame)?;
        self.frames_written += 1;
        Ok(())
    }

    /// Flush the underlying writer.
    pub fn flush(&mut self) -> Result<(), ReplayError> {
        self.writer.flush()?;
        Ok(())
    }

    /// Number of frames written so far.
    pub fn frames_written(&self) -> u64 {
        self.frames_written
    }

    /// Consume the writer and return the underlying `Write` sink.
    pub fn into_inner(self) -> W {
        self.writer
    }
}
