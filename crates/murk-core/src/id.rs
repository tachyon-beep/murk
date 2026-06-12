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
    #[must_use]
    pub fn next() -> Self {
        Self(SPACE_INSTANCE_COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl fmt::Display for SpaceInstanceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

const ENTITY_SLOT_BITS: u32 = 20;
const ENTITY_SLOT_MASK: u32 = (1 << ENTITY_SLOT_BITS) - 1;
const ENTITY_GEN_MAX: u32 = (1 << (32 - ENTITY_SLOT_BITS)) - 1;

/// Identifies an entity within a simulation world.
///
/// Packs a 20-bit slot index and 12-bit generation counter into a `u32`.
/// Entity stores validate the generation on lookup so stale IDs from recycled
/// slots fail instead of addressing a later occupant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(u32);

impl EntityId {
    /// Create an entity ID from a slot index and generation counter.
    ///
    /// # Panics
    ///
    /// Panics if `slot` exceeds 20 bits or `generation` exceeds 12 bits.
    #[must_use]
    pub fn new(slot: u32, generation: u32) -> Self {
        assert!(
            slot <= ENTITY_SLOT_MASK,
            "slot {slot} exceeds maximum {ENTITY_SLOT_MASK}"
        );
        assert!(
            generation <= ENTITY_GEN_MAX,
            "generation {generation} exceeds maximum {ENTITY_GEN_MAX}"
        );
        Self((generation << ENTITY_SLOT_BITS) | slot)
    }

    /// Slot index encoded in the low 20 bits.
    #[inline]
    #[must_use]
    pub fn slot(self) -> u32 {
        self.0 & ENTITY_SLOT_MASK
    }

    /// Generation counter encoded in the high 12 bits.
    #[inline]
    #[must_use]
    pub fn generation(self) -> u32 {
        self.0 >> ENTITY_SLOT_BITS
    }

    /// Raw packed `u32` representation.
    #[inline]
    #[must_use]
    pub fn as_u32(self) -> u32 {
        self.0
    }

    /// Reconstruct an ID from its raw packed representation.
    #[inline]
    #[must_use]
    pub fn from_u32(raw: u32) -> Self {
        Self(raw)
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "entity(slot={}, gen={})", self.slot(), self.generation())
    }
}

/// Indexes into an entity's property array.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PropertyIndex(pub u32);

impl fmt::Display for PropertyIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for PropertyIndex {
    fn from(value: u32) -> Self {
        Self(value)
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

#[cfg(test)]
mod entity_id_tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn new_packs_slot_and_generation() {
        let id = EntityId::new(42, 7);
        assert_eq!(id.slot(), 42);
        assert_eq!(id.generation(), 7);
    }

    #[test]
    fn max_slot_value() {
        let id = EntityId::new(1_048_575, 0);
        assert_eq!(id.slot(), 1_048_575);
        assert_eq!(id.generation(), 0);
    }

    #[test]
    fn max_generation_value() {
        let id = EntityId::new(0, 4095);
        assert_eq!(id.slot(), 0);
        assert_eq!(id.generation(), 4095);
    }

    #[test]
    fn raw_round_trip_preserves_bits() {
        let id = EntityId::new(1, 1);
        let raw = id.as_u32();
        assert_eq!(EntityId::from_u32(raw), id);
        assert_eq!(raw, (1 << 20) | 1);
    }

    #[test]
    #[should_panic(expected = "slot")]
    fn slot_overflow_panics() {
        let _ = EntityId::new(1_048_576, 0);
    }

    #[test]
    #[should_panic(expected = "generation")]
    fn generation_overflow_panics() {
        let _ = EntityId::new(0, 4096);
    }

    #[test]
    fn equality_requires_both_slot_and_generation() {
        assert_ne!(EntityId::new(1, 0), EntityId::new(1, 1));
    }

    #[test]
    fn property_index_displays_inner_value() {
        assert_eq!(PropertyIndex::from(17).to_string(), "17");
    }

    proptest! {
        #[test]
        fn entity_id_round_trip_preserves_slot_generation(
            slot in 0_u32..=1_048_575,
            generation in 0_u32..=4_095,
        ) {
            let id = EntityId::new(slot, generation);

            prop_assert_eq!(id.slot(), slot);
            prop_assert_eq!(id.generation(), generation);
            prop_assert_eq!(EntityId::from_u32(id.as_u32()), id);
        }
    }
}
