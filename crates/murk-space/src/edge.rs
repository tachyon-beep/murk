//! Spatial edge (boundary) behavior for lattice backends.

/// How a lattice space handles neighbors at its edges.
///
/// This is distinct from [`murk_core::BoundaryBehavior`], which controls
/// field *value* clamping. `EdgeBehavior` controls the *topology* — which
/// cells are considered neighbors of boundary cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EdgeBehavior {
    /// Out-of-bounds neighbor maps to the boundary cell (self-loop).
    Clamp,
    /// Out-of-bounds neighbor wraps to the opposite side (periodic).
    Wrap,
    /// Out-of-bounds neighbor is omitted (fewer neighbors at edges).
    Absorb,
}
