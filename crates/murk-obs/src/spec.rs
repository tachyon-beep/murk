//! Observation specification types.
//!
//! An [`ObsSpec`] defines how to extract flat observation tensors from
//! simulation state. Each [`ObsEntry`] targets one field, specifying
//! the spatial region to observe, the transform to apply, and the
//! output data type.

use murk_core::FieldId;
use murk_space::RegionSpec;

/// Specification for observation extraction.
///
/// An `ObsSpec` is a list of entries, each describing one slice of the
/// output tensor. Entries are gathered in order: entry 0 fills the first
/// `N_0` elements, entry 1 fills the next `N_1`, etc.
#[derive(Clone, Debug)]
pub struct ObsSpec {
    /// Ordered observation entries.
    pub entries: Vec<ObsEntry>,
}

/// A single observation entry targeting one field over a spatial region.
#[derive(Clone, Debug)]
pub struct ObsEntry {
    /// Which simulation field to observe.
    pub field_id: FieldId,
    /// Spatial region to gather from.
    pub region: RegionSpec,
    /// Transform to apply to raw field values.
    pub transform: ObsTransform,
    /// Output data type.
    pub dtype: ObsDtype,
}

/// Transform applied to raw field values before output.
///
/// v1 supports `Identity` and `Normalize`. Additional transforms
/// (Pool, Foveate) are deferred to v1.5+.
#[derive(Clone, Debug, PartialEq)]
pub enum ObsTransform {
    /// Pass values through unchanged.
    Identity,
    /// Linearly map `[min, max]` to `[0, 1]`.
    ///
    /// Values outside the range are clamped. If `min == max`,
    /// all outputs are 0.0.
    Normalize {
        /// Lower bound of the input range.
        min: f64,
        /// Upper bound of the input range.
        max: f64,
    },
}

/// Output data type for observation values.
///
/// v1 supports only `F32`. `F16` and `U8` are deferred to v1.5+.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObsDtype {
    /// 32-bit float.
    F32,
}
