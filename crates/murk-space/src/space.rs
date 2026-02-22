//! The core `Space` trait and `dyn Space` downcast support.

use crate::error::SpaceError;
use crate::region::{RegionPlan, RegionSpec};
use murk_core::{Coord, SpaceInstanceId};
use smallvec::SmallVec;
use std::any::Any;

/// Central spatial abstraction for Murk simulations.
///
/// All propagators, observations, and region queries flow through this trait.
/// Concrete backends (Line1D, Ring1D, Square4, Hex2D, Fcc12, ProductSpace) implement
/// it to define their topology.
///
/// # Object Safety
///
/// This trait is designed for use as `dyn Space`. Use
/// `downcast_ref` for opt-in specialization
/// on concrete types (Decision M).
///
/// # Thread Safety
///
/// `Sync` is required because `StepContext` holds `&'a dyn Space` and must
/// be `Send` for RealtimeAsync mode (`&T: Send` requires `T: Sync`).
pub trait Space: Any + Send + Sync + 'static {
    /// Number of spatial dimensions.
    fn ndim(&self) -> usize;

    /// Total number of cells in the space.
    fn cell_count(&self) -> usize;

    /// Enumerate the neighbors of a cell.
    ///
    /// Returns coordinates in a deterministic, backend-defined order.
    /// The `SmallVec<[Coord; 8]>` avoids heap allocation for common
    /// topologies (up to 8 neighbors covers Hex2D and Square8).
    fn neighbours(&self, coord: &Coord) -> SmallVec<[Coord; 8]>;

    /// Graph-geodesic distance between two cells.
    fn distance(&self, a: &Coord, b: &Coord) -> f64;

    /// Compile a region specification into a plan for O(1) lookups.
    fn compile_region(&self, spec: &RegionSpec) -> Result<RegionPlan, SpaceError>;

    /// Iterate over the cells in a compiled region.
    ///
    /// Default implementation iterates over `plan.coords`. Backends may
    /// override for performance.
    fn iter_region<'a>(&'a self, plan: &'a RegionPlan) -> Box<dyn Iterator<Item = Coord> + 'a> {
        Box::new(plan.coords.iter().cloned())
    }

    /// Map a coordinate to its flat tensor index within a compiled region.
    ///
    /// Default implementation performs a linear search in `plan.coords`.
    /// Backends may override for O(1) index arithmetic.
    fn map_coord_to_tensor_index(&self, coord: &Coord, plan: &RegionPlan) -> Option<usize> {
        plan.coords
            .iter()
            .position(|c| c == coord)
            .map(|i| plan.tensor_indices[i])
    }

    /// All cells in deterministic canonical order.
    ///
    /// Two calls on the same space instance must return the same sequence.
    /// Used for observation export and replay reproducibility.
    fn canonical_ordering(&self) -> Vec<Coord>;

    /// Position of a coordinate in the canonical ordering.
    ///
    /// Returns the index such that `canonical_ordering()[index] == coord`.
    /// Default implementation performs a linear search; backends should
    /// override with O(1) arithmetic when possible.
    fn canonical_rank(&self, coord: &Coord) -> Option<usize> {
        self.canonical_ordering().iter().position(|c| c == coord)
    }

    /// Position of a coordinate slice in the canonical ordering.
    ///
    /// The default implementation delegates to [`canonical_rank`](Self::canonical_rank)
    /// for backwards compatibility. Backends can override this to avoid
    /// temporary `Coord` allocations when callers already have a slice.
    fn canonical_rank_slice(&self, coord: &[i32]) -> Option<usize> {
        let coord: Coord = SmallVec::from_slice(coord);
        self.canonical_rank(&coord)
    }

    /// Unique instance identifier for this space object.
    ///
    /// Allocated from a monotonic counter at construction time. Used by
    /// observation plan caching to detect when a different space instance
    /// is passed, avoiding stale plan reuse.
    fn instance_id(&self) -> SpaceInstanceId;

    /// Returns `true` if `self` and `other` are topologically equivalent:
    /// same concrete type and identical behavioral parameters.
    ///
    /// Used by `BatchedEngine` to verify all worlds share the same
    /// topology before compiling a shared observation plan.
    ///
    /// Implementors should downcast `other` to `Self` and compare all
    /// behavior-relevant fields (dimensions, edge behavior, etc.).
    /// Return `false` if the downcast fails (different concrete type).
    fn topology_eq(&self, other: &dyn Space) -> bool;
}

impl dyn Space {
    /// Attempt to downcast a trait object to a concrete Space type.
    ///
    /// This enables opt-in specialization (Decision M): code that works
    /// with `&dyn Space` can check for a known backend and use
    /// type-specific fast paths.
    pub fn downcast_ref<T: Space>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref::<T>()
    }
}
