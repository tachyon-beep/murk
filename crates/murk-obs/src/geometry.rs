//! Grid geometry extraction for interior/boundary dispatch.
//!
//! [`GridGeometry`] captures the dimensional structure of grid-based
//! spaces (Square4, Square8, Hex2D) via `downcast_ref` (Decision M).
//! This enables O(1) interior detection and branchless gather for
//! agent-centered foveation.

use murk_space::Space;

/// Grid connectivity type, determines graph-distance metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridConnectivity {
    /// 4-connected (Square4): graph distance = Manhattan |dr| + |dc|.
    FourWay,
    /// 8-connected (Square8): graph distance = Chebyshev max(|dr|, |dc|).
    EightWay,
    /// 6-connected hex (Hex2D, axial coords): graph distance = max(|dq|, |dr|, |dq+dr|).
    Hex,
}

/// Extracted grid geometry for fast interior/boundary dispatch.
///
/// If the space is a known grid type, we extract its dimensions and
/// strides per coordinate index. This enables:
/// - O(1) `is_interior` check (no BFS, no bounds-check per cell)
/// - Direct stride arithmetic for the fast gather path
///
/// `coord_dims\[i\]` is the valid range for `coord\[i\]` (0..coord_dims\[i\]).
/// `coord_strides\[i\]` is the stride for `coord\[i\]` in canonical rank.
#[derive(Debug, Clone)]
pub struct GridGeometry {
    /// Valid range per coordinate index: `coord[i]` must be in `0..coord_dims[i]`.
    pub coord_dims: Vec<u32>,
    /// Stride per coordinate index for canonical rank computation.
    pub coord_strides: Vec<usize>,
    /// Number of spatial dimensions.
    pub ndim: usize,
    /// Whether all boundaries wrap (torus topology → all positions interior).
    pub all_wrap: bool,
    /// Connectivity type for graph-distance computation.
    pub connectivity: GridConnectivity,
}

impl GridGeometry {
    /// Try to extract grid geometry from a `&dyn Space` via downcast.
    ///
    /// Returns `Some` for Square4, Square8, Hex2D (all 2D grids).
    /// Returns `None` for Line1D, Ring1D, ProductSpace (heterogeneous),
    /// or any unknown Space implementation.
    pub fn from_space(space: &dyn Space) -> Option<Self> {
        // Try Square4: coord = [row, col], rank = row * cols + col
        if let Some(sq4) = space.downcast_ref::<murk_space::Square4>() {
            let all_wrap = sq4.edge_behavior() == murk_space::EdgeBehavior::Wrap;
            return Some(GridGeometry {
                coord_dims: vec![sq4.rows(), sq4.cols()],
                coord_strides: vec![sq4.cols() as usize, 1],
                ndim: 2,
                all_wrap,
                connectivity: GridConnectivity::FourWay,
            });
        }

        // Try Square8: coord = [row, col], rank = row * cols + col
        if let Some(sq8) = space.downcast_ref::<murk_space::Square8>() {
            let all_wrap = sq8.edge_behavior() == murk_space::EdgeBehavior::Wrap;
            return Some(GridGeometry {
                coord_dims: vec![sq8.rows(), sq8.cols()],
                coord_strides: vec![sq8.cols() as usize, 1],
                ndim: 2,
                all_wrap,
                connectivity: GridConnectivity::EightWay,
            });
        }

        // Try Hex2D: coord = [q, r], rank = r * cols + q
        // coord[0]=q has range [0, cols) and stride 1
        // coord[1]=r has range [0, rows) and stride cols
        if let Some(hex) = space.downcast_ref::<murk_space::Hex2D>() {
            return Some(GridGeometry {
                coord_dims: vec![hex.cols(), hex.rows()],
                coord_strides: vec![1, hex.cols() as usize],
                ndim: 2,
                all_wrap: false,
                connectivity: GridConnectivity::Hex,
            });
        }

        None
    }

    /// Compute the canonical rank of a coordinate using stride arithmetic.
    ///
    /// For a 2D grid with dims `[R, C]`, coord `[a, b]`:
    /// `rank = a * strides[0] + b * strides[1]`.
    pub fn canonical_rank(&self, coord: &[i32]) -> usize {
        debug_assert_eq!(coord.len(), self.ndim);
        let mut rank = 0usize;
        for (c, &stride) in coord.iter().zip(&self.coord_strides) {
            rank += *c as usize * stride;
        }
        rank
    }

    /// Check if a coordinate is within the grid bounds.
    pub fn in_bounds(&self, coord: &[i32]) -> bool {
        if coord.len() != self.ndim {
            return false;
        }
        for (c, &dim) in coord.iter().zip(&self.coord_dims) {
            if *c < 0 || *c >= dim as i32 {
                return false;
            }
        }
        true
    }

    /// O(1) check: is the agent at `center` fully interior for a given radius?
    ///
    /// An agent is interior if all cells within `radius` of `center` are
    /// in-bounds on every coordinate axis:
    /// `radius <= center[i]` and `center[i] + radius < coord_dims[i]` for all `i`.
    ///
    /// For wrapped spaces (`all_wrap == true`), every position is interior.
    pub fn is_interior(&self, center: &[i32], radius: u32) -> bool {
        if self.all_wrap {
            return true;
        }
        let r = radius as i32;
        for (c, &dim) in center.iter().zip(&self.coord_dims) {
            if *c < r || *c + r >= dim as i32 {
                return false;
            }
        }
        true
    }

    /// Compute the graph distance from origin for a relative coordinate.
    ///
    /// This uses the distance metric appropriate for the grid connectivity:
    /// - `FourWay`: Manhattan distance `|d0| + |d1| + ...`
    /// - `EightWay`: Chebyshev distance `max(|d0|, |d1|, ...)`
    /// - `Hex`: Cube distance `max(|dq|, |dr|, |dq + dr|)` (axial coords)
    pub fn graph_distance(&self, relative: &[i32]) -> u32 {
        match self.connectivity {
            GridConnectivity::FourWay => relative.iter().map(|&d| d.unsigned_abs()).sum(),
            GridConnectivity::EightWay => relative
                .iter()
                .map(|&d| d.unsigned_abs())
                .max()
                .unwrap_or(0),
            GridConnectivity::Hex => {
                // Axial coordinates [dq, dr]. Cube distance = max(|dq|, |dr|, |dq+dr|).
                debug_assert_eq!(relative.len(), 2);
                let dq = relative[0];
                let dr = relative[1];
                let ds = dq + dr; // implicit third axis s = -(q+r)
                dq.unsigned_abs()
                    .max(dr.unsigned_abs())
                    .max(ds.unsigned_abs())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_space::{EdgeBehavior, Hex2D, Square4, Square8};

    #[test]
    fn extract_square4() {
        let s = Square4::new(10, 8, EdgeBehavior::Absorb).unwrap();
        let geo = GridGeometry::from_space(&s).unwrap();
        assert_eq!(geo.coord_dims, vec![10, 8]); // coord[0]=row, coord[1]=col
        assert_eq!(geo.coord_strides, vec![8, 1]);
        assert_eq!(geo.ndim, 2);
        assert!(!geo.all_wrap);
    }

    #[test]
    fn extract_square4_wrap() {
        let s = Square4::new(5, 5, EdgeBehavior::Wrap).unwrap();
        let geo = GridGeometry::from_space(&s).unwrap();
        assert!(geo.all_wrap);
    }

    #[test]
    fn extract_square8() {
        let s = Square8::new(6, 7, EdgeBehavior::Clamp).unwrap();
        let geo = GridGeometry::from_space(&s).unwrap();
        assert_eq!(geo.coord_dims, vec![6, 7]);
        assert!(!geo.all_wrap);
    }

    #[test]
    fn extract_hex2d() {
        let s = Hex2D::new(12, 15).unwrap();
        let geo = GridGeometry::from_space(&s).unwrap();
        // coord[0]=q has range [0,15), coord[1]=r has range [0,12)
        assert_eq!(geo.coord_dims, vec![15, 12]);
        assert_eq!(geo.coord_strides, vec![1, 15]);
        assert!(!geo.all_wrap);
    }

    #[test]
    fn extract_line1d_returns_none() {
        let s = murk_space::Line1D::new(10, EdgeBehavior::Absorb).unwrap();
        assert!(GridGeometry::from_space(&s).is_none());
    }

    #[test]
    fn extract_product_returns_none() {
        let a = murk_space::Line1D::new(5, EdgeBehavior::Absorb).unwrap();
        let b = murk_space::Line1D::new(3, EdgeBehavior::Absorb).unwrap();
        let p = murk_space::ProductSpace::new(vec![Box::new(a), Box::new(b)]).unwrap();
        assert!(GridGeometry::from_space(&p).is_none());
    }

    #[test]
    fn canonical_rank_matches_space() {
        let s = Square4::new(5, 7, EdgeBehavior::Absorb).unwrap();
        let geo = GridGeometry::from_space(&s).unwrap();
        // coord [3, 4] → rank = 3*7 + 4 = 25
        assert_eq!(geo.canonical_rank(&[3, 4]), 25);
        assert_eq!(s.canonical_rank(&smallvec::smallvec![3, 4]), Some(25));
    }

    #[test]
    fn canonical_rank_hex_matches() {
        let s = Hex2D::new(5, 7).unwrap();
        let geo = GridGeometry::from_space(&s).unwrap();
        // Hex coord [q, r] = [3, 2] → rank = r*cols + q = 2*7 + 3 = 17
        assert_eq!(geo.canonical_rank(&[3, 2]), 17);
        assert_eq!(s.canonical_rank(&smallvec::smallvec![3, 2]), Some(17));
    }

    #[test]
    fn in_bounds_checks() {
        let s = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let geo = GridGeometry::from_space(&s).unwrap();
        assert!(geo.in_bounds(&[0, 0]));
        assert!(geo.in_bounds(&[4, 4]));
        assert!(!geo.in_bounds(&[-1, 0]));
        assert!(!geo.in_bounds(&[5, 0]));
        assert!(!geo.in_bounds(&[0, 5]));
    }

    #[test]
    fn is_interior_absorb() {
        let s = Square4::new(20, 20, EdgeBehavior::Absorb).unwrap();
        let geo = GridGeometry::from_space(&s).unwrap();
        // Center (10, 10) with radius 3: interior (3..16 on both axes)
        assert!(geo.is_interior(&[10, 10], 3));
        // Center (2, 10): row 2, radius 3 → 2 < 3, boundary
        assert!(!geo.is_interior(&[2, 10], 3));
        // Center (17, 10): row 17+3=20 >= 20, boundary
        assert!(!geo.is_interior(&[17, 10], 3));
    }

    #[test]
    fn is_interior_wrap_always_true() {
        let s = Square4::new(5, 5, EdgeBehavior::Wrap).unwrap();
        let geo = GridGeometry::from_space(&s).unwrap();
        assert!(geo.is_interior(&[0, 0], 3));
        assert!(geo.is_interior(&[4, 4], 10));
    }

    #[test]
    fn interior_count_20x20_radius3() {
        let s = Square4::new(20, 20, EdgeBehavior::Absorb).unwrap();
        let geo = GridGeometry::from_space(&s).unwrap();
        let mut interior = 0;
        for r in 0..20 {
            for c in 0..20 {
                if geo.is_interior(&[r, c], 3) {
                    interior += 1;
                }
            }
        }
        // Interior: rows 3..16, cols 3..16 = 14*14 = 196
        // Total = 400, ratio = 196/400 = 0.49
        // Wait, actually radius 3 means center[i] >= 3 and center[i]+3 < 20
        // so center[i] in [3, 16], that's 14 values per axis
        assert_eq!(interior, 196);
        assert!(interior as f64 / 400.0 > 0.45);
    }

    // ── graph_distance tests ───────────────────────────────

    #[test]
    fn graph_distance_four_way_manhattan() {
        let s = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let geo = GridGeometry::from_space(&s).unwrap();
        assert_eq!(geo.graph_distance(&[0, 0]), 0);
        assert_eq!(geo.graph_distance(&[1, 0]), 1);
        assert_eq!(geo.graph_distance(&[0, 1]), 1);
        assert_eq!(geo.graph_distance(&[1, 1]), 2); // Manhattan: |1|+|1|=2
        assert_eq!(geo.graph_distance(&[-2, 3]), 5);
    }

    #[test]
    fn graph_distance_eight_way_chebyshev() {
        let s = Square8::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let geo = GridGeometry::from_space(&s).unwrap();
        assert_eq!(geo.graph_distance(&[0, 0]), 0);
        assert_eq!(geo.graph_distance(&[1, 1]), 1); // Chebyshev: max(1,1)=1
        assert_eq!(geo.graph_distance(&[-2, 3]), 3);
        assert_eq!(geo.graph_distance(&[5, -3]), 5);
    }

    #[test]
    fn graph_distance_hex_cube() {
        let s = Hex2D::new(10, 10).unwrap();
        let geo = GridGeometry::from_space(&s).unwrap();
        // Hex distance = max(|dq|, |dr|, |dq+dr|) in axial coords.
        assert_eq!(geo.graph_distance(&[0, 0]), 0);
        assert_eq!(geo.graph_distance(&[1, 0]), 1);
        assert_eq!(geo.graph_distance(&[0, 1]), 1);
        assert_eq!(geo.graph_distance(&[1, -1]), 1); // Adjacent hex
        assert_eq!(geo.graph_distance(&[1, 1]), 2); // max(1,1,2)=2
        assert_eq!(geo.graph_distance(&[-2, -2]), 4); // max(2,2,4)=4
        assert_eq!(geo.graph_distance(&[2, -1]), 2); // max(2,1,1)=2
    }

    #[test]
    fn connectivity_type_correct() {
        let sq4 = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        assert_eq!(
            GridGeometry::from_space(&sq4).unwrap().connectivity,
            GridConnectivity::FourWay
        );
        let sq8 = Square8::new(5, 5, EdgeBehavior::Absorb).unwrap();
        assert_eq!(
            GridGeometry::from_space(&sq8).unwrap().connectivity,
            GridConnectivity::EightWay
        );
        let hex = Hex2D::new(5, 5).unwrap();
        assert_eq!(
            GridGeometry::from_space(&hex).unwrap().connectivity,
            GridConnectivity::Hex
        );
    }
}
