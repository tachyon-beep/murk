//! Data types for replay recording and playback.

/// Build environment metadata stored in the replay header.
///
/// Enables detection of builds compiled with different toolchains or
/// flags that might affect floating-point determinism.
///
/// # Examples
///
/// ```
/// use murk_replay::BuildMetadata;
///
/// let meta = BuildMetadata {
///     toolchain: "1.78.0".into(),
///     target_triple: "x86_64-unknown-linux-gnu".into(),
///     murk_version: "0.1.0".into(),
///     compile_flags: "release".into(),
/// };
///
/// assert_eq!(meta.murk_version, "0.1.0");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildMetadata {
    /// Rust toolchain version (e.g. `"1.78.0"`).
    pub toolchain: String,
    /// Compilation target triple (e.g. `"x86_64-unknown-linux-gnu"`).
    pub target_triple: String,
    /// Murk crate version (e.g. `"0.1.0"`).
    pub murk_version: String,
    /// Compilation flags or profile (e.g. `"release"`, `"debug"`).
    pub compile_flags: String,
}

/// Simulation initialization parameters stored in the replay header.
///
/// Captures everything needed to reconstruct an identical world configuration
/// for replay: the RNG seed, configuration hash, field/cell counts, and
/// an opaque space descriptor.
///
/// # Examples
///
/// ```
/// use murk_replay::InitDescriptor;
///
/// let init = InitDescriptor {
///     seed: 42,
///     config_hash: 0xDEAD_BEEF,
///     field_count: 3,
///     cell_count: 100,
///     space_descriptor: vec![],
/// };
///
/// assert_eq!(init.seed, 42);
/// assert_eq!(init.field_count, 3);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitDescriptor {
    /// RNG seed used for deterministic simulation.
    pub seed: u64,
    /// Hash of the world configuration (fields, propagators, dt, etc.).
    pub config_hash: u64,
    /// Number of fields in the world.
    pub field_count: u32,
    /// Total number of cells in the spatial topology.
    pub cell_count: u64,
    /// Opaque serialized space descriptor for reconstruction.
    pub space_descriptor: Vec<u8>,
}

/// A serialized command within a replay frame.
///
/// Commands are serialized to a flat binary representation for compact
/// storage. The `payload_type` tag identifies the `CommandPayload` variant.
#[derive(Clone, Debug, PartialEq)]
pub struct SerializedCommand {
    /// Payload type discriminant (see [`PAYLOAD_*`](crate) constants).
    pub payload_type: u8,
    /// Serialized payload bytes.
    pub payload: Vec<u8>,
    /// Priority class (lower = higher priority).
    pub priority_class: u8,
    /// Source identifier for ordering (`None` if not set).
    pub source_id: Option<u64>,
    /// Per-source sequence number (`None` if not set).
    pub source_seq: Option<u64>,
    /// Tick after which the command expires (raw `TickId` value).
    pub expires_after_tick: u64,
    /// Monotonic arrival sequence number for deterministic ordering.
    pub arrival_seq: u64,
}

/// A single tick's worth of recorded data.
///
/// Contains the tick identifier, all commands submitted during that tick,
/// and a hash of the post-tick snapshot for verification.
///
/// # Examples
///
/// ```
/// use murk_replay::Frame;
///
/// let frame = Frame {
///     tick_id: 1,
///     commands: vec![],
///     snapshot_hash: 0,
/// };
///
/// assert_eq!(frame.tick_id, 1);
/// assert!(frame.commands.is_empty());
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// The tick number this frame represents.
    pub tick_id: u64,
    /// Commands submitted during this tick.
    pub commands: Vec<SerializedCommand>,
    /// FNV-1a hash of the post-tick snapshot.
    pub snapshot_hash: u64,
}

// ── Payload type tag constants ──────────────────────────────────

/// Payload type tag for `CommandPayload::Move`.
pub const PAYLOAD_MOVE: u8 = 0;
/// Payload type tag for `CommandPayload::Spawn`.
pub const PAYLOAD_SPAWN: u8 = 1;
/// Payload type tag for `CommandPayload::Despawn`.
pub const PAYLOAD_DESPAWN: u8 = 2;
/// Payload type tag for `CommandPayload::SetField`.
pub const PAYLOAD_SET_FIELD: u8 = 3;
/// Payload type tag for `CommandPayload::Custom`.
pub const PAYLOAD_CUSTOM: u8 = 4;
/// Payload type tag for `CommandPayload::SetParameter`.
pub const PAYLOAD_SET_PARAMETER: u8 = 5;
/// Payload type tag for `CommandPayload::SetParameterBatch`.
pub const PAYLOAD_SET_PARAMETER_BATCH: u8 = 6;
