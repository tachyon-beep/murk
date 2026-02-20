//! Observation metadata populated at execution time.

use murk_core::{ParameterVersion, TickId, WorldGenerationId};

/// Metadata accompanying an observation extraction.
///
/// Populated by [`ObsPlan::execute`](crate::ObsPlan::execute) from the
/// snapshot being observed. All five fields are guaranteed to be set.
#[derive(Clone, Debug, PartialEq)]
pub struct ObsMetadata {
    /// Tick at which the observed snapshot was produced.
    pub tick_id: TickId,
    /// Age of the snapshot relative to the current engine tick.
    /// In Lockstep mode this is always 0. In RealtimeAsync it may
    /// be > 0 if reading a stale snapshot.
    pub age_ticks: u64,
    /// Fraction of the observation tensor filled with valid data
    /// (i.e., `valid_ratio` from the region plan).
    pub coverage: f64,
    /// Arena generation of the observed snapshot.
    pub world_generation_id: WorldGenerationId,
    /// Parameter version at the time of the snapshot.
    pub parameter_version: ParameterVersion,
}
