//! Error types for the replay system.

use std::fmt;
use std::io;

/// Errors that can occur during replay recording, playback, or comparison.
#[derive(Debug)]
pub enum ReplayError {
    /// An I/O error occurred during read or write.
    Io(io::Error),
    /// The file does not start with the expected `b"MURK"` magic bytes.
    InvalidMagic,
    /// The format version is not supported by this build.
    UnsupportedVersion {
        /// The version found in the file.
        found: u8,
    },
    /// A frame could not be decoded (truncated or corrupt data).
    MalformedFrame {
        /// Human-readable description of what went wrong.
        detail: String,
    },
    /// A command payload type tag is not recognized.
    UnknownPayloadType {
        /// The unrecognized type tag.
        tag: u8,
    },
    /// The replay was recorded with a different configuration hash.
    ConfigMismatch {
        /// Hash from the replay file header.
        recorded: u64,
        /// Hash computed from the current configuration.
        current: u64,
    },
    /// A snapshot hash does not match between recorded and replayed state.
    SnapshotMismatch {
        /// The tick at which the mismatch was detected.
        tick_id: u64,
        /// Hash from the replay file.
        recorded: u64,
        /// Hash computed from the replayed simulation.
        replayed: u64,
    },
}

impl fmt::Display for ReplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::InvalidMagic => write!(f, "invalid magic bytes (expected b\"MURK\")"),
            Self::UnsupportedVersion { found } => {
                write!(f, "unsupported format version {found}")
            }
            Self::MalformedFrame { detail } => write!(f, "malformed frame: {detail}"),
            Self::UnknownPayloadType { tag } => {
                write!(f, "unknown payload type tag {tag}")
            }
            Self::ConfigMismatch { recorded, current } => {
                write!(
                    f,
                    "config hash mismatch: recorded={recorded:#018x}, current={current:#018x}"
                )
            }
            Self::SnapshotMismatch {
                tick_id,
                recorded,
                replayed,
            } => {
                write!(
                    f,
                    "snapshot mismatch at tick {tick_id}: \
                     recorded={recorded:#018x}, replayed={replayed:#018x}"
                )
            }
        }
    }
}

impl std::error::Error for ReplayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for ReplayError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
