//! Strongly-typed identifiers and the [`Coord`] type alias.

use smallvec::SmallVec;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// Identifies a field within a simulation world.
///
/// Fields are registered at world creation and assigned sequential IDs.
/// `FieldId(n)` corresponds to the n-th field in the world configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FieldId(pub u32);

impl fmt::Display for FieldId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for FieldId {
    fn from(v: u32) -> Self {
        Self(v)
    }
}

/// Identifies a space (spatial topology) within a simulation world.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpaceId(pub u32);

impl fmt::Display for SpaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for SpaceId {
    fn from(v: u32) -> Self {
        Self(v)
    }
}

/// Counter for unique [`SpaceInstanceId`] allocation.
static SPACE_INSTANCE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Unique per-instance identifier for a `Space` object.
///
/// Allocated from a monotonic atomic counter via [`SpaceInstanceId::next`].
/// Two distinct space instances always have different IDs, even if they
/// have identical topology. Used by observation plan caching to avoid
/// ABA reuse when a space is dropped and a new one is allocated at the
/// same address.
///
/// Cloning a space preserves its instance ID, which is correct because
/// immutable spaces with the same ID have the same topology.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SpaceInstanceId(u64);

impl SpaceInstanceId {
    /// Allocate a fresh, unique instance ID.
    ///
    /// Each call returns a new ID that has never been returned before
    /// within this process. Thread-safe.
    pub fn next() -> Self {
        Self(SPACE_INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl fmt::Display for SpaceInstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Monotonically increasing tick counter.
///
/// Incremented each time the simulation advances one step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TickId(pub u64);

impl fmt::Display for TickId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for TickId {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

/// Tracks arena generation for snapshot identity.
///
/// Incremented each time a new snapshot is published, enabling
/// ObsPlan invalidation detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorldGenerationId(pub u64);

impl fmt::Display for WorldGenerationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for WorldGenerationId {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

/// Tracks the version of global simulation parameters.
///
/// Incremented when any `SetParameter` or `SetParameterBatch` command
/// is applied, enabling stale-parameter detection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ParameterVersion(pub u64);

impl fmt::Display for ParameterVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for ParameterVersion {
    fn from(v: u64) -> Self {
        Self(v)
    }
}

/// Key for a global simulation parameter (e.g., learning rate, reward scale).
///
/// Parameters are registered at world creation; invalid keys are rejected
/// at ingress.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ParameterKey(pub u32);

impl fmt::Display for ParameterKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for ParameterKey {
    fn from(v: u32) -> Self {
        Self(v)
    }
}

/// A coordinate in simulation space.
///
/// Uses `SmallVec<[i32; 4]>` to avoid heap allocation for spaces
/// up to 4 dimensions, covering all v1 topologies (1D, 2D, hex).
/// Higher-dimensional spaces spill to the heap transparently.
pub type Coord = SmallVec<[i32; 4]>;
