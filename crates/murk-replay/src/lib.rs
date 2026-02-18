//! Deterministic replay and action logging for Murk simulations.
//!
//! Records and replays action sequences for debugging, testing,
//! and training data collection. Provides a binary replay format
//! with per-tick snapshot hashing for determinism verification.
//!
//! # Architecture
//!
//! - [`ReplayWriter`] records frames to any `Write` sink
//! - [`ReplayReader`] plays back frames from any `Read` source
//! - [`compare_snapshot`] and [`replay_and_compare`] verify determinism
//! - All I/O uses a custom binary codec (no serde dependency)
//!
//! # Format
//!
//! ```text
//! [MAGIC "MURK"] [VERSION u8] [BuildMetadata] [InitDescriptor]
//! [Frame 1] [Frame 2] ... [Frame N]
//! ```
//!
//! Each frame contains the tick ID, serialized commands, and a
//! FNV-1a hash of the post-tick snapshot.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]

pub mod codec;
pub mod compare;
pub mod error;
pub mod hash;
pub mod reader;
pub mod types;
pub mod writer;

pub use compare::{
    compare_snapshot, replay_and_compare, DivergenceKind, DivergenceReport, FieldDivergence,
};
pub use error::ReplayError;
pub use hash::{config_hash, snapshot_hash};
pub use reader::{FrameIter, ReplayReader};
pub use types::{BuildMetadata, Frame, InitDescriptor, SerializedCommand};
pub use writer::ReplayWriter;

/// Magic bytes at the start of every replay file.
pub const MAGIC: [u8; 4] = *b"MURK";

/// Current binary format version.
///
/// History:
/// - v1: source_id and source_seq encoded as bare u64 (0 = not set)
/// - v2: source_id and source_seq use presence-flag encoding (u8 flag + optional u64)
pub const FORMAT_VERSION: u8 = 2;
