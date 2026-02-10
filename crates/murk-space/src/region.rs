//! Region specification and compiled region plans.

use murk_core::Coord;

/// Specifies a region of cells within a Space.
///
/// Used for observation gathering, propagator spatial queries,
/// and region-scoped operations.
#[derive(Clone, Debug, PartialEq)]
pub enum RegionSpec {
    /// Every cell in the space.
    All,
    /// Topology-aware disk: all cells within `radius` graph-distance of `center`.
    Disk {
        /// Center coordinate.
        center: Coord,
        /// Maximum graph distance from center (inclusive).
        radius: u32,
    },
    /// Axis-aligned bounding box in coordinate space.
    Rect {
        /// Minimum corner (inclusive).
        min: Coord,
        /// Maximum corner (inclusive).
        max: Coord,
    },
    /// BFS expansion from center to given depth.
    Neighbours {
        /// Center coordinate.
        center: Coord,
        /// BFS depth.
        depth: u32,
    },
    /// Explicit list of coordinates.
    Coords(Vec<Coord>),
}

/// Compiled region plan — precomputed for O(1) lookups during tick execution.
///
/// Created by [`Space::compile_region`](crate::Space::compile_region).
#[derive(Clone, Debug)]
pub struct RegionPlan {
    /// Number of valid cells in the region.
    pub cell_count: usize,
    /// Precomputed coordinates in canonical iteration order.
    pub coords: Vec<Coord>,
    /// Mapping: `coords[i]` -> flat tensor index for observation output.
    pub tensor_indices: Vec<usize>,
    /// Validity mask: `1` = valid cell, `0` = padding.
    /// Length = `bounding_shape.total_elements()`.
    pub valid_mask: Vec<u8>,
    /// Shape of the bounding tensor that contains this region.
    pub bounding_shape: BoundingShape,
}

impl RegionPlan {
    /// Fraction of tensor elements that are valid (non-padding).
    pub fn valid_ratio(&self) -> f64 {
        let total = self.bounding_shape.total_elements();
        if total == 0 {
            return 0.0;
        }
        self.valid_mask.iter().filter(|&&v| v == 1).count() as f64 / total as f64
    }
}

/// Shape of the bounding tensor for a compiled region.
#[derive(Clone, Debug, PartialEq)]
pub enum BoundingShape {
    /// N-dimensional rectangular bounding box.
    Rect(Vec<usize>),
}

impl BoundingShape {
    /// Total number of elements in the bounding tensor.
    pub fn total_elements(&self) -> usize {
        match self {
            Self::Rect(dims) => dims.iter().product(),
        }
    }
}
