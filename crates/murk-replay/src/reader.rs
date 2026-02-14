//! Replay playback reader.
//!
//! [`ReplayReader`] reads frames from any `Read` source, decoding the
//! binary replay format. The header is validated on construction.

use std::io::Read;

use crate::codec::{decode_frame, decode_header};
use crate::error::ReplayError;
use crate::types::{BuildMetadata, Frame, InitDescriptor};

/// Reads replay data from a byte stream.
///
/// Generic over `R: Read` so tests can use `&[u8]` and production
/// code can use `BufReader<File>`.
pub struct ReplayReader<R: Read> {
    reader: R,
    metadata: BuildMetadata,
    init: InitDescriptor,
    frames_read: u64,
}

impl<R: Read> ReplayReader<R> {
    /// Open a replay stream, reading and validating the header.
    pub fn open(mut reader: R) -> Result<Self, ReplayError> {
        let (metadata, init) = decode_header(&mut reader)?;
        Ok(Self {
            reader,
            metadata,
            init,
            frames_read: 0,
        })
    }

    /// Build metadata from the replay header.
    pub fn metadata(&self) -> &BuildMetadata {
        &self.metadata
    }

    /// Initialization descriptor from the replay header.
    pub fn init_descriptor(&self) -> &InitDescriptor {
        &self.init
    }

    /// Read the next frame, or `None` if the stream is exhausted.
    pub fn next_frame(&mut self) -> Result<Option<Frame>, ReplayError> {
        let frame = decode_frame(&mut self.reader)?;
        if frame.is_some() {
            self.frames_read += 1;
        }
        Ok(frame)
    }

    /// Number of frames read so far.
    pub fn frames_read(&self) -> u64 {
        self.frames_read
    }

    /// Convert into a frame iterator.
    pub fn frames(self) -> FrameIter<R> {
        FrameIter {
            reader: self.reader,
            frames_read: self.frames_read,
            done: false,
        }
    }
}

/// Iterator adapter over replay frames.
pub struct FrameIter<R: Read> {
    reader: R,
    frames_read: u64,
    done: bool,
}

impl<R: Read> Iterator for FrameIter<R> {
    type Item = Result<Frame, ReplayError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        match decode_frame(&mut self.reader) {
            Ok(Some(frame)) => {
                self.frames_read += 1;
                Some(Ok(frame))
            }
            Ok(None) => {
                self.done = true;
                None
            }
            Err(e) => {
                self.done = true;
                Some(Err(e))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;
    use crate::writer::ReplayWriter;
    use murk_core::command::{Command, CommandPayload};
    use murk_core::id::{ParameterKey, ParameterVersion, TickId, WorldGenerationId};
    use murk_test_utils::MockSnapshot;

    fn test_metadata() -> BuildMetadata {
        BuildMetadata {
            toolchain: "test".into(),
            target_triple: "test".into(),
            murk_version: "0.1.0".into(),
            compile_flags: "test".into(),
        }
    }

    fn test_init() -> InitDescriptor {
        InitDescriptor {
            seed: 42,
            config_hash: 123,
            field_count: 1,
            cell_count: 10,
            space_descriptor: vec![],
        }
    }

    fn make_snapshot(tick: u64, data: Vec<f32>) -> MockSnapshot {
        let mut snap =
            MockSnapshot::new(TickId(tick), WorldGenerationId(tick), ParameterVersion(0));
        snap.set_field(murk_core::FieldId(0), data);
        snap
    }

    #[test]
    fn roundtrip_write_read_frames() {
        let meta = test_metadata();
        let init = test_init();

        // Write 5 frames
        let mut buf = Vec::new();
        {
            let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
            for tick in 1..=5u64 {
                let snap = make_snapshot(tick, vec![tick as f32; 10]);
                writer.write_frame(tick, &[], &snap).unwrap();
            }
            assert_eq!(writer.frames_written(), 5);
        }

        // Read them back
        let mut reader = ReplayReader::open(buf.as_slice()).unwrap();
        assert_eq!(reader.metadata(), &meta);
        assert_eq!(reader.init_descriptor(), &init);

        for tick in 1..=5u64 {
            let frame = reader.next_frame().unwrap().unwrap();
            assert_eq!(frame.tick_id, tick);
            assert!(frame.commands.is_empty());
        }
        assert!(reader.next_frame().unwrap().is_none());
        assert_eq!(reader.frames_read(), 5);
    }

    #[test]
    fn roundtrip_with_commands() {
        let meta = test_metadata();
        let init = test_init();

        let cmd = Command {
            payload: CommandPayload::SetParameter {
                key: ParameterKey(0),
                value: 1.5,
            },
            expires_after_tick: TickId(u64::MAX),
            source_id: Some(1),
            source_seq: Some(1),
            priority_class: 1,
            arrival_seq: 0,
        };

        let mut buf = Vec::new();
        {
            let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
            let snap = make_snapshot(1, vec![0.0; 10]);
            writer.write_frame(1, &[cmd], &snap).unwrap();
        }

        let mut reader = ReplayReader::open(buf.as_slice()).unwrap();
        let frame = reader.next_frame().unwrap().unwrap();
        assert_eq!(frame.tick_id, 1);
        assert_eq!(frame.commands.len(), 1);
        assert_eq!(frame.commands[0].payload_type, PAYLOAD_SET_PARAMETER);
    }

    #[test]
    fn frame_iterator_works() {
        let meta = test_metadata();
        let init = test_init();

        let mut buf = Vec::new();
        {
            let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
            for tick in 1..=3u64 {
                let snap = make_snapshot(tick, vec![0.0; 10]);
                writer.write_frame(tick, &[], &snap).unwrap();
            }
        }

        let reader = ReplayReader::open(buf.as_slice()).unwrap();
        let frames: Vec<_> = reader.frames().collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!(frames[0].tick_id, 1);
        assert_eq!(frames[1].tick_id, 2);
        assert_eq!(frames[2].tick_id, 3);
    }

    #[test]
    fn truncated_stream_errors() {
        let meta = test_metadata();
        let init = test_init();

        let mut buf = Vec::new();
        {
            let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
            let snap = make_snapshot(1, vec![0.0; 10]);
            writer.write_frame(1, &[], &snap).unwrap();
        }

        // Truncate mid-frame
        buf.truncate(buf.len() - 4);
        let mut reader = ReplayReader::open(buf.as_slice()).unwrap();
        // Should get an error, not Ok(None)
        assert!(reader.next_frame().is_err());
    }

    #[test]
    fn bad_magic_on_open() {
        let data = b"XURK\x01rest of data";
        let result = ReplayReader::open(data.as_slice());
        assert!(matches!(result, Err(ReplayError::InvalidMagic)));
    }
}
