//! 1D ring lattice (always-wrap periodic boundary).

use crate::error::SpaceError;
use crate::line1d;
use crate::region::{RegionPlan, RegionSpec};
use crate::space::Space;
use murk_core::{Coord, SpaceInstanceId};
use smallvec::SmallVec;

/// A one-dimensional ring lattice (periodic boundary).
///
/// Equivalent to [`Line1D`](crate::Line1D) with [`EdgeBehavior::Wrap`](crate::EdgeBehavior::Wrap),
/// but exists as a separate type so that
/// `downcast_ref::<Ring1D>()` works correctly.
///
/// # Examples
///
/// ```
/// use murk_space::{Ring1D, Space};
///
/// let ring = Ring1D::new(10).unwrap();
/// assert_eq!(ring.len(), 10);
/// assert_eq!(ring.cell_count(), 10);
///
/// // Every cell has exactly 2 neighbours (periodic boundary).
/// let edge: murk_core::Coord = vec![0i32].into();
/// assert_eq!(ring.neighbours(&edge).len(), 2);
///
/// // Wrap-around distance: 0 and 9 are 1 step apart, not 9.
/// let a: murk_core::Coord = vec![0i32].into();
/// let b: murk_core::Coord = vec![9i32].into();
/// assert_eq!(ring.distance(&a, &b), 1.0);
/// ```
#[derive(Debug, Clone)]
pub struct Ring1D {
    len: u32,
    instance_id: SpaceInstanceId,
}

impl Ring1D {
    /// Maximum length: coordinates use `i32`, so `len` must fit.
    pub const MAX_LEN: u32 = i32::MAX as u32;

    /// Create a new 1D ring with `len` cells.
    ///
    /// Returns `Err(SpaceError::EmptySpace)` if `len == 0`, or
    /// `Err(SpaceError::DimensionTooLarge)` if `len > i32::MAX`.
    pub fn new(len: u32) -> Result<Self, SpaceError> {
        if len == 0 {
            return Err(SpaceError::EmptySpace);
        }
        if len > Self::MAX_LEN {
            return Err(SpaceError::DimensionTooLarge {
                name: "len",
                value: len,
                max: Self::MAX_LEN,
            });
        }
        Ok(Self {
            len,
            instance_id: SpaceInstanceId::next(),
        })
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

    fn max_neighbour_degree(&self) -> usize {
        2
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

    fn canonical_rank(&self, coord: &Coord) -> Option<usize> {
        if coord.len() != 1 {
            return None;
        }
        let i = coord[0];
        if i >= 0 && i < self.len as i32 {
            Some(i as usize)
        } else {
            None
        }
    }

    fn canonical_rank_slice(&self, coord: &[i32]) -> Option<usize> {
        if coord.len() != 1 {
            return None;
        }
        let i = coord[0];
        if i >= 0 && i < self.len as i32 {
            Some(i as usize)
        } else {
            None
        }
    }

    fn instance_id(&self) -> SpaceInstanceId {
        self.instance_id
    }

    fn topology_eq(&self, other: &dyn Space) -> bool {
        (other as &dyn std::any::Any)
            .downcast_ref::<Self>()
            .is_some_and(|o| self.len == o.len)
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
        assert_eq!(plan.cell_count(), 8);
        assert_eq!(plan.valid_ratio(), 1.0);
    }

    // ── Constructor test ────────────────────────────────────────

    #[test]
    fn new_zero_len_returns_error() {
        assert!(matches!(Ring1D::new(0), Err(SpaceError::EmptySpace)));
    }

    #[test]
    fn new_rejects_len_exceeding_i32_max() {
        assert!(matches!(
            Ring1D::new(i32::MAX as u32 + 1),
            Err(SpaceError::DimensionTooLarge { .. })
        ));
        assert!(Ring1D::new(i32::MAX as u32).is_ok());
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
