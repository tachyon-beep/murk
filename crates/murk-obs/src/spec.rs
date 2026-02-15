//! Observation specification types.
//!
//! An [`ObsSpec`] defines how to extract flat observation tensors from
//! simulation state. Each [`ObsEntry`] targets one field, specifying
//! the spatial region to observe, the transform to apply, and the
//! output data type.

use murk_core::FieldId;
use murk_space::RegionSpec;
use smallvec::SmallVec;

/// Specification for observation extraction.
///
/// An `ObsSpec` is a list of entries, each describing one slice of the
/// output tensor. Entries are gathered in order: entry 0 fills the first
/// `N_0` elements, entry 1 fills the next `N_1`, etc.
///
/// # Examples
///
/// ```
/// use murk_obs::{ObsSpec, ObsEntry, ObsDtype, ObsTransform, ObsRegion};
/// use murk_core::FieldId;
/// use murk_space::RegionSpec;
///
/// let spec = ObsSpec {
///     entries: vec![
///         ObsEntry {
///             field_id: FieldId(0),
///             region: ObsRegion::Fixed(RegionSpec::All),
///             pool: None,
///             transform: ObsTransform::Identity,
///             dtype: ObsDtype::F32,
///         },
///         ObsEntry {
///             field_id: FieldId(1),
///             region: ObsRegion::AgentDisk { radius: 3 },
///             pool: None,
///             transform: ObsTransform::Normalize { min: 0.0, max: 100.0 },
///             dtype: ObsDtype::F32,
///         },
///     ],
/// };
///
/// assert_eq!(spec.entries.len(), 2);
/// assert_eq!(spec.entries[0].field_id, FieldId(0));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct ObsSpec {
    /// Ordered observation entries.
    pub entries: Vec<ObsEntry>,
}

/// Observation region — how to select spatial cells for an entry.
///
/// `Fixed` regions are resolved at plan-compile time (like the existing
/// `RegionSpec`). `AgentDisk` and `AgentRect` are resolved at execute
/// time relative to each agent's position (foveation).
#[derive(Clone, Debug, PartialEq)]
pub enum ObsRegion {
    /// Absolute region, compiled at plan-compile time.
    Fixed(RegionSpec),
    /// Disk centered on the agent, resolved at execute time.
    AgentDisk {
        /// Maximum graph distance from agent center (inclusive).
        radius: u32,
    },
    /// Axis-aligned rectangle centered on the agent, resolved at execute time.
    AgentRect {
        /// Half-extent per dimension (the full extent is `2 * half_extent + 1`).
        half_extent: SmallVec<[u32; 4]>,
    },
}

impl From<RegionSpec> for ObsRegion {
    fn from(spec: RegionSpec) -> Self {
        ObsRegion::Fixed(spec)
    }
}

/// Pooling kernel type for spatial downsampling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolKernel {
    /// Average of valid cells in the window.
    Mean,
    /// Maximum of valid cells in the window.
    Max,
    /// Minimum of valid cells in the window.
    Min,
    /// Sum of valid cells in the window.
    Sum,
}

/// Configuration for spatial pooling applied after gather.
///
/// # Examples
///
/// ```
/// use murk_obs::{PoolConfig, PoolKernel};
///
/// let pool = PoolConfig {
///     kernel: PoolKernel::Mean,
///     kernel_size: 3,
///     stride: 2,
/// };
///
/// assert_eq!(pool.kernel, PoolKernel::Mean);
/// assert_eq!(pool.kernel_size, 3);
/// assert_eq!(pool.stride, 2);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoolConfig {
    /// Pooling kernel type.
    pub kernel: PoolKernel,
    /// Window size (applies to all spatial dimensions).
    pub kernel_size: usize,
    /// Stride between windows (applies to all spatial dimensions).
    pub stride: usize,
}

/// A single observation entry targeting one field over a spatial region.
///
/// # Examples
///
/// ```
/// use murk_obs::{ObsEntry, ObsDtype, ObsTransform, ObsRegion};
/// use murk_core::FieldId;
/// use murk_space::RegionSpec;
///
/// let entry = ObsEntry {
///     field_id: FieldId(0),
///     region: RegionSpec::All.into(),
///     pool: None,
///     transform: ObsTransform::Identity,
///     dtype: ObsDtype::F32,
/// };
///
/// assert_eq!(entry.field_id, FieldId(0));
/// assert!(entry.pool.is_none());
/// assert!(matches!(entry.region, ObsRegion::Fixed(RegionSpec::All)));
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct ObsEntry {
    /// Which simulation field to observe.
    pub field_id: FieldId,
    /// Spatial region to gather from.
    pub region: ObsRegion,
    /// Optional spatial pooling applied after gather, before transform.
    pub pool: Option<PoolConfig>,
    /// Transform to apply to raw field values (element-wise, after pooling).
    pub transform: ObsTransform,
    /// Output data type.
    pub dtype: ObsDtype,
}

/// Transform applied to raw field values before output.
///
/// v1 supports `Identity` and `Normalize`. Additional transforms
/// are deferred to v1.5+.
///
/// # Examples
///
/// ```
/// use murk_obs::ObsTransform;
///
/// let t = ObsTransform::Normalize { min: 0.0, max: 1.0 };
/// assert!(matches!(t, ObsTransform::Normalize { min, max } if max > min));
///
/// let identity = ObsTransform::Identity;
/// assert_ne!(identity, t);
/// ```
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
