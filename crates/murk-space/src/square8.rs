//! 2D square grid with 8-connected neighbourhood (cardinal + diagonal).

use crate::edge::EdgeBehavior;
use crate::error::SpaceError;
use crate::grid2d;
use crate::region::{RegionPlan, RegionSpec};
use crate::space::Space;
use murk_core::{Coord, SpaceInstanceId};
use smallvec::{smallvec, SmallVec};

/// All 8 offsets: N, S, W, E, NW, NE, SW, SE.
const OFFSETS_8: [(i32, i32); 8] = [
    (-1, 0),
    (1, 0),
    (0, -1),
    (0, 1),
    (-1, -1),
    (-1, 1),
    (1, -1),
    (1, 1),
];

/// A two-dimensional square grid with 8-connected neighbourhood.
///
/// Each cell has coordinate `[row, col]`. Neighbours include the four
/// cardinal directions plus four diagonals. Distance is Chebyshev (L-inf),
/// consistent with 8-connected semantics where diagonal moves cost 1.
///
/// Boundary handling is controlled by [`EdgeBehavior`]:
/// - **Absorb**: edge cells have fewer neighbors (corners have 3, edges have 5)
/// - **Clamp**: edge cells self-loop on the boundary axis
/// - **Wrap**: periodic boundary (torus topology)
#[derive(Debug, Clone)]
pub struct Square8 {
    rows: u32,
    cols: u32,
    edge: EdgeBehavior,
    instance_id: SpaceInstanceId,
}

impl Square8 {
    /// Create a new 2D grid with `rows * cols` cells and the given edge behavior.
    ///
    /// Maximum dimension size: coordinates use `i32`, so each axis must fit.
    pub const MAX_DIM: u32 = i32::MAX as u32;

    /// Returns `Err(SpaceError::EmptySpace)` if either dimension is 0, or
    /// `Err(SpaceError::DimensionTooLarge)` if either exceeds `i32::MAX`.
    pub fn new(rows: u32, cols: u32, edge: EdgeBehavior) -> Result<Self, SpaceError> {
        if rows == 0 || cols == 0 {
            return Err(SpaceError::EmptySpace);
        }
        if rows > Self::MAX_DIM {
            return Err(SpaceError::DimensionTooLarge {
                name: "rows",
                value: rows,
                max: Self::MAX_DIM,
            });
        }
        if cols > Self::MAX_DIM {
            return Err(SpaceError::DimensionTooLarge {
                name: "cols",
                value: cols,
                max: Self::MAX_DIM,
            });
        }
        Ok(Self {
            rows,
            cols,
            edge,
            instance_id: SpaceInstanceId::next(),
        })
    }

    /// Number of rows.
    pub fn rows(&self) -> u32 {
        self.rows
    }

    /// Number of columns.
    pub fn cols(&self) -> u32 {
        self.cols
    }

    /// Always returns `false` — construction rejects empty grids.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Edge behavior.
    pub fn edge_behavior(&self) -> EdgeBehavior {
        self.edge
    }

    /// Compute the 8-connected neighbours of `(r, c)` as `(row, col)` pairs.
    fn neighbours_rc(&self, r: i32, c: i32) -> Vec<(i32, i32)> {
        let mut result = Vec::with_capacity(8);
        for (dr, dc) in OFFSETS_8 {
            let nr = grid2d::resolve_axis(r + dr, self.rows, self.edge);
            let nc = grid2d::resolve_axis(c + dc, self.cols, self.edge);
            if let (Some(nr), Some(nc)) = (nr, nc) {
                result.push((nr, nc));
            }
        }
        result
    }
}

impl Space for Square8 {
    fn ndim(&self) -> usize {
        2
    }

    fn cell_count(&self) -> usize {
        (self.rows as usize) * (self.cols as usize)
    }

    fn neighbours(&self, coord: &Coord) -> SmallVec<[Coord; 8]> {
        let r = coord[0];
        let c = coord[1];
        self.neighbours_rc(r, c)
            .into_iter()
            .map(|(nr, nc)| smallvec![nr, nc])
            .collect()
    }

    fn distance(&self, a: &Coord, b: &Coord) -> f64 {
        // Chebyshev (L-inf) distance — matches graph geodesic for 8-connected.
        let dr = grid2d::axis_distance(a[0], b[0], self.rows, self.edge);
        let dc = grid2d::axis_distance(a[1], b[1], self.cols, self.edge);
        dr.max(dc)
    }

    fn compile_region(&self, spec: &RegionSpec) -> Result<RegionPlan, SpaceError> {
        let edge = self.edge;
        let rows = self.rows;
        let cols = self.cols;
        grid2d::compile_region_2d(spec, rows, cols, self, |r, c| {
            let mut result = Vec::with_capacity(8);
            for (dr, dc) in OFFSETS_8 {
                let nr = grid2d::resolve_axis(r + dr, rows, edge);
                let nc = grid2d::resolve_axis(c + dc, cols, edge);
                if let (Some(nr), Some(nc)) = (nr, nc) {
                    result.push((nr, nc));
                }
            }
            result
        })
    }

    fn canonical_ordering(&self) -> Vec<Coord> {
        grid2d::canonical_ordering_2d(self.rows, self.cols)
    }

    fn canonical_rank(&self, coord: &Coord) -> Option<usize> {
        grid2d::canonical_rank_2d(coord, self.rows, self.cols)
    }

    fn instance_id(&self) -> SpaceInstanceId {
        self.instance_id
    }

    fn topology_eq(&self, other: &dyn Space) -> bool {
        (other as &dyn std::any::Any)
            .downcast_ref::<Self>()
            .map_or(false, |o| {
                self.rows == o.rows && self.cols == o.cols && self.edge == o.edge
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compliance;
    use murk_core::Coord;
    use proptest::prelude::*;

    fn c(r: i32, col: i32) -> Coord {
        smallvec![r, col]
    }

    // ── Neighbour tests ─────────────────────────────────────────

    #[test]
    fn neighbours_absorb_interior() {
        let s = Square8::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = s.neighbours(&c(2, 2));
        assert_eq!(n.len(), 8);
    }

    #[test]
    fn neighbours_absorb_corner() {
        let s = Square8::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = s.neighbours(&c(0, 0));
        assert_eq!(n.len(), 3);
        assert!(n.contains(&c(1, 0)));
        assert!(n.contains(&c(0, 1)));
        assert!(n.contains(&c(1, 1)));
    }

    #[test]
    fn neighbours_absorb_edge() {
        let s = Square8::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = s.neighbours(&c(0, 2));
        assert_eq!(n.len(), 5);
    }

    #[test]
    fn neighbours_wrap_corner() {
        let s = Square8::new(5, 5, EdgeBehavior::Wrap).unwrap();
        let n = s.neighbours(&c(0, 0));
        assert_eq!(n.len(), 8);
        assert!(n.contains(&c(4, 4))); // NW wraps on both axes
        assert!(n.contains(&c(4, 0))); // N wraps
        assert!(n.contains(&c(0, 4))); // W wraps
    }

    // ── Distance tests ──────────────────────────────────────────

    #[test]
    fn distance_chebyshev_absorb() {
        let s = Square8::new(10, 10, EdgeBehavior::Absorb).unwrap();
        // Diagonal: max(1, 1) = 1
        assert_eq!(s.distance(&c(0, 0), &c(1, 1)), 1.0);
        // max(3, 4) = 4
        assert_eq!(s.distance(&c(0, 0), &c(3, 4)), 4.0);
        // max(3, 4) = 4
        assert_eq!(s.distance(&c(2, 3), &c(5, 7)), 4.0);
    }

    #[test]
    fn distance_chebyshev_wrap() {
        let s = Square8::new(10, 10, EdgeBehavior::Wrap).unwrap();
        // Wrap: per-axis min(9,1) = 1 each → max(1,1) = 1
        assert_eq!(s.distance(&c(0, 0), &c(9, 9)), 1.0);
    }

    // ── Region tests ────────────────────────────────────────────

    #[test]
    fn compile_region_all() {
        let s = Square8::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let plan = s.compile_region(&RegionSpec::All).unwrap();
        assert_eq!(plan.cell_count(), 25);
        assert_eq!(plan.valid_ratio(), 1.0);
    }

    #[test]
    fn compile_region_disk_square_shape() {
        // Disk on Square8 should produce a square shape (Chebyshev ball).
        let s = Square8::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Disk {
                center: c(5, 5),
                radius: 2,
            })
            .unwrap();
        // Chebyshev ball of radius 2: 5×5 = 25 cells
        assert_eq!(plan.cell_count(), 25);
    }

    #[test]
    fn compile_region_rect() {
        let s = Square8::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Rect {
                min: c(2, 3),
                max: c(4, 6),
            })
            .unwrap();
        assert_eq!(plan.cell_count(), 12); // 3 rows * 4 cols
    }

    // ── Constructor tests ───────────────────────────────────────

    #[test]
    fn new_zero_rows_returns_error() {
        assert!(matches!(
            Square8::new(0, 5, EdgeBehavior::Absorb),
            Err(SpaceError::EmptySpace)
        ));
    }

    #[test]
    fn new_rejects_dims_exceeding_i32_max() {
        let big = i32::MAX as u32 + 1;
        assert!(matches!(
            Square8::new(big, 5, EdgeBehavior::Absorb),
            Err(SpaceError::DimensionTooLarge { name: "rows", .. })
        ));
        assert!(matches!(
            Square8::new(5, big, EdgeBehavior::Absorb),
            Err(SpaceError::DimensionTooLarge { name: "cols", .. })
        ));
    }

    // ── 1×1 edge case ──────────────────────────────────────────

    #[test]
    fn single_cell_absorb() {
        let s = Square8::new(1, 1, EdgeBehavior::Absorb).unwrap();
        assert!(s.neighbours(&c(0, 0)).is_empty());
    }

    #[test]
    fn single_cell_wrap() {
        let s = Square8::new(1, 1, EdgeBehavior::Wrap).unwrap();
        let n = s.neighbours(&c(0, 0));
        // All 8 directions wrap to self
        assert_eq!(n.len(), 8);
        assert!(n.iter().all(|nb| nb == &c(0, 0)));
    }

    // ── Compliance suites ───────────────────────────────────────

    #[test]
    fn compliance_absorb() {
        let s = Square8::new(8, 8, EdgeBehavior::Absorb).unwrap();
        compliance::run_full_compliance(&s);
    }

    #[test]
    fn compliance_clamp() {
        let s = Square8::new(8, 8, EdgeBehavior::Clamp).unwrap();
        compliance::run_full_compliance(&s);
    }

    #[test]
    fn compliance_wrap() {
        let s = Square8::new(8, 8, EdgeBehavior::Wrap).unwrap();
        compliance::run_full_compliance(&s);
    }

    // ── Downcast test ───────────────────────────────────────────

    #[test]
    fn downcast_ref_square8() {
        let s: Box<dyn Space> = Box::new(Square8::new(3, 3, EdgeBehavior::Absorb).unwrap());
        assert!(s.downcast_ref::<Square8>().is_some());
        assert!(s.downcast_ref::<crate::Square4>().is_none());
    }

    // ── Property tests ──────────────────────────────────────────

    fn arb_edge() -> impl Strategy<Value = EdgeBehavior> {
        prop_oneof![
            Just(EdgeBehavior::Absorb),
            Just(EdgeBehavior::Clamp),
            Just(EdgeBehavior::Wrap),
        ]
    }

    proptest! {
        #[test]
        fn distance_is_metric(
            rows in 2u32..10,
            cols in 2u32..10,
            edge in arb_edge(),
            ar in 0i32..10, ac in 0i32..10,
            br in 0i32..10, bc in 0i32..10,
            cr in 0i32..10, cc_val in 0i32..10,
        ) {
            let ar = ar % rows as i32;
            let ac = ac % cols as i32;
            let br = br % rows as i32;
            let bc = bc % cols as i32;
            let cr = cr % rows as i32;
            let cc_val = cc_val % cols as i32;
            let s = Square8::new(rows, cols, edge).unwrap();
            let a: Coord = smallvec![ar, ac];
            let b: Coord = smallvec![br, bc];
            let cv: Coord = smallvec![cr, cc_val];

            prop_assert!((s.distance(&a, &a) - 0.0).abs() < f64::EPSILON);
            prop_assert!((s.distance(&a, &b) - s.distance(&b, &a)).abs() < f64::EPSILON);
            prop_assert!(s.distance(&a, &cv) <= s.distance(&a, &b) + s.distance(&b, &cv) + f64::EPSILON);
        }

        #[test]
        fn neighbours_symmetric(
            rows in 2u32..10,
            cols in 2u32..10,
            edge in arb_edge(),
            r in 0i32..10, col in 0i32..10,
        ) {
            let r = r % rows as i32;
            let col = col % cols as i32;
            let s = Square8::new(rows, cols, edge).unwrap();
            let coord: Coord = smallvec![r, col];
            for nb in s.neighbours(&coord) {
                let nb_neighbours = s.neighbours(&nb);
                prop_assert!(
                    nb_neighbours.contains(&coord),
                    "neighbour symmetry violated: {:?} in N({:?}) but {:?} not in N({:?})",
                    nb, coord, coord, nb,
                );
            }
        }
    }
}
