//! 1D ring lattice (always-wrap periodic boundary).

use crate::error::SpaceError;
use crate::line1d;
use crate::region::{RegionPlan, RegionSpec};
use crate::space::Space;
use murk_core::Coord;
use smallvec::SmallVec;

/// A one-dimensional ring lattice (periodic boundary).
///
/// Equivalent to [`Line1D`](crate::Line1D) with [`EdgeBehavior::Wrap`](crate::EdgeBehavior::Wrap),
/// but exists as a separate type so that
/// `downcast_ref::<Ring1D>()` works correctly.
#[derive(Debug, Clone)]
pub struct Ring1D {
    len: u32,
}

impl Ring1D {
    /// Create a new 1D ring with `len` cells.
    ///
    /// Returns `Err(SpaceError::EmptySpace)` if `len == 0`.
    pub fn new(len: u32) -> Result<Self, SpaceError> {
        if len == 0 {
            return Err(SpaceError::EmptySpace);
        }
        Ok(Self { len })
    }

    /// Number of cells.
    pub fn len(&self) -> u32 {
        self.len
    }

    /// Always returns `false` — construction rejects `len == 0`.
    pub fn is_empty(&self) -> bool {
        false
    }
}

impl Space for Ring1D {
    fn ndim(&self) -> usize {
        1
    }

    fn cell_count(&self) -> usize {
        self.len as usize
    }

    fn neighbours(&self, coord: &Coord) -> SmallVec<[Coord; 8]> {
        line1d::wrap_neighbours_1d(coord[0], self.len)
    }

    fn distance(&self, a: &Coord, b: &Coord) -> f64 {
        line1d::wrap_distance_1d(a[0], b[0], self.len)
    }

    fn compile_region(&self, spec: &RegionSpec) -> Result<RegionPlan, SpaceError> {
        line1d::compile_region_1d(spec, self.len, true)
    }

    fn canonical_ordering(&self) -> Vec<Coord> {
        line1d::canonical_ordering_1d(self.len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compliance;
    use murk_core::Coord;
    use smallvec::smallvec;

    fn c(i: i32) -> Coord {
        smallvec![i]
    }

    // ── Neighbour tests ─────────────────────────────────────────

    #[test]
    fn neighbours_interior() {
        let s = Ring1D::new(5).unwrap();
        let n = s.neighbours(&c(2));
        assert_eq!(n.as_slice(), &[c(1), c(3)]);
    }

    #[test]
    fn neighbours_wrap_left() {
        let s = Ring1D::new(5).unwrap();
        let n = s.neighbours(&c(0));
        assert_eq!(n.as_slice(), &[c(4), c(1)]);
    }

    #[test]
    fn neighbours_wrap_right() {
        let s = Ring1D::new(5).unwrap();
        let n = s.neighbours(&c(4));
        assert_eq!(n.as_slice(), &[c(3), c(0)]);
    }

    #[test]
    fn neighbours_len_2() {
        let s = Ring1D::new(2).unwrap();
        let n = s.neighbours(&c(0));
        assert_eq!(n.as_slice(), &[c(1), c(1)]);
    }

    // ── Distance test ───────────────────────────────────────────

    #[test]
    fn distance_worked() {
        let s = Ring1D::new(10).unwrap();
        assert_eq!(s.distance(&c(0), &c(9)), 1.0);
        assert_eq!(s.distance(&c(2), &c(7)), 5.0);
        assert_eq!(s.distance(&c(3), &c(8)), 5.0);
    }

    // ── Region test ─────────────────────────────────────────────

    #[test]
    fn compile_region_all() {
        let s = Ring1D::new(8).unwrap();
        let plan = s.compile_region(&RegionSpec::All).unwrap();
        assert_eq!(plan.cell_count, 8);
        assert_eq!(plan.valid_ratio(), 1.0);
    }

    // ── Constructor test ────────────────────────────────────────

    #[test]
    fn new_zero_len_returns_error() {
        assert!(matches!(Ring1D::new(0), Err(SpaceError::EmptySpace)));
    }

    // ── Compliance ──────────────────────────────────────────────

    #[test]
    fn compliance_full() {
        let s = Ring1D::new(20).unwrap();
        compliance::run_full_compliance(&s);
    }

    // ── Downcast tests ──────────────────────────────────────────

    #[test]
    fn downcast_ref_ring1d() {
        let s: Box<dyn Space> = Box::new(Ring1D::new(5).unwrap());
        assert!(s.downcast_ref::<Ring1D>().is_some());
    }

    #[test]
    fn downcast_ref_wrong_type() {
        let s: Box<dyn Space> = Box::new(Ring1D::new(5).unwrap());
        assert!(s.downcast_ref::<crate::Line1D>().is_none());
    }
}
