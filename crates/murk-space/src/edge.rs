//! Spatial edge (boundary) behavior for lattice backends.

/// How a lattice space handles neighbors at its edges.
///
/// This is distinct from [`murk_core::BoundaryBehavior`], which controls
/// field *value* clamping. `EdgeBehavior` controls the *topology* — which
/// cells are considered neighbors of boundary cells.
///
/// # Examples
///
/// ```
/// use murk_space::{Square4, EdgeBehavior, Space};
///
/// // Absorb: corner has 2 neighbors, interior has 4.
/// let absorb = Square4::new(4, 4, EdgeBehavior::Absorb).unwrap();
/// let corner: murk_core::Coord = vec![0i32, 0].into();
/// let interior: murk_core::Coord = vec![1i32, 1].into();
/// assert_eq!(absorb.neighbours(&corner).len(), 2);
/// assert_eq!(absorb.neighbours(&interior).len(), 4);
///
/// // Wrap: all cells have exactly 4 neighbors (torus).
/// let wrap = Square4::new(4, 4, EdgeBehavior::Wrap).unwrap();
/// assert_eq!(wrap.neighbours(&corner).len(), 4);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdgeBehavior {
    /// Out-of-bounds neighbor maps to the boundary cell (self-loop).
    Clamp,
    /// Out-of-bounds neighbor wraps to the opposite side (periodic).
    Wrap,
    /// Out-of-bounds neighbor is omitted (fewer neighbors at edges).
    Absorb,
}
