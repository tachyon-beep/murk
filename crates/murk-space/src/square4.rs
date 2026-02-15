//! 2D square grid with 4-connected neighbourhood (N/S/E/W).

use crate::edge::EdgeBehavior;
use crate::error::SpaceError;
use crate::grid2d;
use crate::region::{RegionPlan, RegionSpec};
use crate::space::Space;
use murk_core::{Coord, SpaceInstanceId};
use smallvec::{smallvec, SmallVec};

/// A two-dimensional square grid with 4-connected neighbourhood.
///
/// Each cell has coordinate `[row, col]` where `0 <= row < rows` and
/// `0 <= col < cols`. Neighbours are the four cardinal directions
/// (north, south, east, west). Distance is Manhattan (L1).
///
/// Boundary handling is controlled by [`EdgeBehavior`]:
/// - **Absorb**: edge cells have fewer neighbors (corners have 2, edges have 3)
/// - **Clamp**: edge cells self-loop on the boundary axis
/// - **Wrap**: periodic boundary (torus topology)
#[derive(Debug, Clone)]
pub struct Square4 {
    rows: u32,
    cols: u32,
    edge: EdgeBehavior,
    instance_id: SpaceInstanceId,
}

impl Square4 {
    /// Maximum dimension size: coordinates use `i32`, so each axis must fit.
    pub const MAX_DIM: u32 = i32::MAX as u32;

    /// Create a new 2D grid with `rows * cols` cells and the given edge behavior.
    ///
    /// Returns `Err(SpaceError::EmptySpace)` if either dimension is 0, or
    /// `Err(SpaceError::DimensionTooLarge)` if either exceeds `i32::MAX`.
    ///
    /// # Examples
    ///
    /// ```
    /// use murk_space::{Square4, EdgeBehavior, Space};
    ///
    /// let grid = Square4::new(16, 16, EdgeBehavior::Absorb).unwrap();
    /// assert_eq!(grid.cell_count(), 256);
    /// assert_eq!(grid.ndim(), 2);
    ///
    /// // Neighbors of corner cell [0, 0] with Absorb: only 2 neighbors.
    /// let coord = vec![0i32, 0].into();
    /// assert_eq!(grid.neighbours(&coord).len(), 2);
    /// ```
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

    /// Compute the 4-connected neighbours of `(r, c)` as `(row, col)` pairs.
    fn neighbours_rc(&self, r: i32, c: i32) -> Vec<(i32, i32)> {
        let offsets: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
        let mut result = Vec::with_capacity(4);
        for (dr, dc) in offsets {
            let nr = grid2d::resolve_axis(r + dr, self.rows, self.edge);
            let nc = grid2d::resolve_axis(c + dc, self.cols, self.edge);
            if let (Some(nr), Some(nc)) = (nr, nc) {
                result.push((nr, nc));
            }
        }
        result
    }
}

impl Space for Square4 {
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
        // Manhattan (L1) distance — matches graph geodesic for 4-connected.
        let dr = grid2d::axis_distance(a[0], b[0], self.rows, self.edge);
        let dc = grid2d::axis_distance(a[1], b[1], self.cols, self.edge);
        dr + dc
    }

    fn compile_region(&self, spec: &RegionSpec) -> Result<RegionPlan, SpaceError> {
        let edge = self.edge;
        let rows = self.rows;
        let cols = self.cols;
        grid2d::compile_region_2d(spec, rows, cols, self, |r, c| {
            let offsets: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
            let mut result = Vec::with_capacity(4);
            for (dr, dc) in offsets {
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
        let s = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = s.neighbours(&c(2, 2));
        assert_eq!(n.len(), 4);
        assert!(n.contains(&c(1, 2))); // north
        assert!(n.contains(&c(3, 2))); // south
        assert!(n.contains(&c(2, 1))); // west
        assert!(n.contains(&c(2, 3))); // east
    }

    #[test]
    fn neighbours_absorb_corner() {
        let s = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = s.neighbours(&c(0, 0));
        assert_eq!(n.len(), 2);
        assert!(n.contains(&c(1, 0)));
        assert!(n.contains(&c(0, 1)));
    }

    #[test]
    fn neighbours_absorb_edge() {
        let s = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = s.neighbours(&c(0, 2));
        assert_eq!(n.len(), 3);
        assert!(n.contains(&c(1, 2)));
        assert!(n.contains(&c(0, 1)));
        assert!(n.contains(&c(0, 3)));
    }

    #[test]
    fn neighbours_clamp_corner() {
        let s = Square4::new(5, 5, EdgeBehavior::Clamp).unwrap();
        let n = s.neighbours(&c(0, 0));
        assert_eq!(n.len(), 4);
        // Self-loops on both axes
        assert!(n.contains(&c(0, 0))); // north clamps to self
        assert!(n.contains(&c(1, 0)));
        assert!(n.contains(&c(0, 0))); // west clamps to self
        assert!(n.contains(&c(0, 1)));
    }

    #[test]
    fn neighbours_wrap_corner() {
        let s = Square4::new(5, 5, EdgeBehavior::Wrap).unwrap();
        let n = s.neighbours(&c(0, 0));
        assert_eq!(n.len(), 4);
        assert!(n.contains(&c(4, 0))); // north wraps
        assert!(n.contains(&c(1, 0))); // south
        assert!(n.contains(&c(0, 4))); // west wraps
        assert!(n.contains(&c(0, 1))); // east
    }

    #[test]
    fn neighbours_wrap_opposite_corner() {
        let s = Square4::new(5, 5, EdgeBehavior::Wrap).unwrap();
        let n = s.neighbours(&c(4, 4));
        assert_eq!(n.len(), 4);
        assert!(n.contains(&c(3, 4))); // north
        assert!(n.contains(&c(0, 4))); // south wraps
        assert!(n.contains(&c(4, 3))); // west
        assert!(n.contains(&c(4, 0))); // east wraps
    }

    // ── Distance tests ──────────────────────────────────────────

    #[test]
    fn distance_manhattan_absorb() {
        let s = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        assert_eq!(s.distance(&c(0, 0), &c(3, 4)), 7.0);
        assert_eq!(s.distance(&c(2, 3), &c(5, 7)), 7.0);
    }

    #[test]
    fn distance_manhattan_wrap() {
        let s = Square4::new(10, 10, EdgeBehavior::Wrap).unwrap();
        // Direct: |0-9| + |0-9| = 18, Wrap: min(9,1) + min(9,1) = 2
        assert_eq!(s.distance(&c(0, 0), &c(9, 9)), 2.0);
        // Direct: |0-3| + |0-4| = 7, no benefit from wrap
        assert_eq!(s.distance(&c(0, 0), &c(3, 4)), 7.0);
    }

    // ── Region tests ────────────────────────────────────────────

    #[test]
    fn compile_region_all() {
        let s = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let plan = s.compile_region(&RegionSpec::All).unwrap();
        assert_eq!(plan.cell_count, 25);
        assert_eq!(plan.valid_ratio(), 1.0);
    }

    #[test]
    fn compile_region_disk_diamond_shape() {
        // Disk on Square4 should produce a diamond shape.
        let s = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Disk {
                center: c(5, 5),
                radius: 2,
            })
            .unwrap();
        // Diamond of radius 2: 1 + 3 + 5 + 3 + 1 = 13 cells
        assert_eq!(plan.cell_count, 13);
    }

    #[test]
    fn compile_region_rect() {
        let s = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Rect {
                min: c(2, 3),
                max: c(4, 6),
            })
            .unwrap();
        assert_eq!(plan.cell_count, 12); // 3 rows * 4 cols
    }

    #[test]
    fn compile_region_rect_invalid() {
        let s = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        assert!(s
            .compile_region(&RegionSpec::Rect {
                min: c(5, 0),
                max: c(2, 3),
            })
            .is_err());
    }

    #[test]
    fn compile_region_coords_valid() {
        let s = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Coords(vec![c(1, 2), c(3, 4), c(0, 0)]))
            .unwrap();
        assert_eq!(plan.coords, vec![c(0, 0), c(1, 2), c(3, 4)]);
    }

    #[test]
    fn compile_region_coords_oob() {
        let s = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        assert!(s
            .compile_region(&RegionSpec::Coords(vec![c(10, 0)]))
            .is_err());
    }

    // ── Constructor tests ───────────────────────────────────────

    #[test]
    fn new_zero_rows_returns_error() {
        assert!(matches!(
            Square4::new(0, 5, EdgeBehavior::Absorb),
            Err(SpaceError::EmptySpace)
        ));
    }

    #[test]
    fn new_zero_cols_returns_error() {
        assert!(matches!(
            Square4::new(5, 0, EdgeBehavior::Absorb),
            Err(SpaceError::EmptySpace)
        ));
    }

    #[test]
    fn new_rejects_dims_exceeding_i32_max() {
        let big = i32::MAX as u32 + 1;
        assert!(matches!(
            Square4::new(big, 5, EdgeBehavior::Absorb),
            Err(SpaceError::DimensionTooLarge { name: "rows", .. })
        ));
        assert!(matches!(
            Square4::new(5, big, EdgeBehavior::Absorb),
            Err(SpaceError::DimensionTooLarge { name: "cols", .. })
        ));
        assert!(Square4::new(i32::MAX as u32, 1, EdgeBehavior::Absorb).is_ok());
    }

    // ── 1×1 edge case ──────────────────────────────────────────

    #[test]
    fn single_cell_absorb() {
        let s = Square4::new(1, 1, EdgeBehavior::Absorb).unwrap();
        assert!(s.neighbours(&c(0, 0)).is_empty());
    }

    #[test]
    fn single_cell_wrap() {
        let s = Square4::new(1, 1, EdgeBehavior::Wrap).unwrap();
        let n = s.neighbours(&c(0, 0));
        // All 4 directions wrap to self
        assert_eq!(n.len(), 4);
        assert!(n.iter().all(|nb| nb == &c(0, 0)));
    }

    // ── Compliance suites ───────────────────────────────────────

    #[test]
    fn compliance_absorb() {
        let s = Square4::new(8, 8, EdgeBehavior::Absorb).unwrap();
        compliance::run_full_compliance(&s);
    }

    #[test]
    fn compliance_clamp() {
        let s = Square4::new(8, 8, EdgeBehavior::Clamp).unwrap();
        compliance::run_full_compliance(&s);
    }

    #[test]
    fn compliance_wrap() {
        let s = Square4::new(8, 8, EdgeBehavior::Wrap).unwrap();
        compliance::run_full_compliance(&s);
    }

    // ── Downcast test ───────────────────────────────────────────

    #[test]
    fn downcast_ref_square4() {
        let s: Box<dyn Space> = Box::new(Square4::new(3, 3, EdgeBehavior::Absorb).unwrap());
        assert!(s.downcast_ref::<Square4>().is_some());
        assert!(s.downcast_ref::<crate::Ring1D>().is_none());
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
            cr in 0i32..10, cc in 0i32..10,
        ) {
            let ar = ar % rows as i32;
            let ac = ac % cols as i32;
            let br = br % rows as i32;
            let bc = bc % cols as i32;
            let cr = cr % rows as i32;
            let cc = cc % cols as i32;
            let s = Square4::new(rows, cols, edge).unwrap();
            let a: Coord = smallvec![ar, ac];
            let b: Coord = smallvec![br, bc];
            let cv: Coord = smallvec![cr, cc];

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
            let s = Square4::new(rows, cols, edge).unwrap();
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
