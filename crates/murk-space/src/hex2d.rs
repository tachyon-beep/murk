//! 2D hexagonal lattice with axial coordinates (pointy-top orientation).

use crate::error::SpaceError;
use crate::region::{BoundingShape, RegionPlan, RegionSpec};
use crate::space::Space;
use murk_core::{Coord, SpaceInstanceId};
use smallvec::{smallvec, SmallVec};

/// Pointy-top hex offsets in axial `(dq, dr)` order: E, NE, NW, W, SW, SE.
const HEX_OFFSETS: [(i32, i32); 6] = [
    (1, 0),  // E
    (1, -1), // NE
    (0, -1), // NW
    (-1, 0), // W
    (-1, 1), // SW
    (0, 1),  // SE
];

/// A two-dimensional hexagonal lattice with axial coordinates.
///
/// Each cell has coordinate `[q, r]` where `0 <= q < cols` and `0 <= r < rows`.
/// The grid uses pointy-top orientation with six neighbours per interior cell.
/// Boundary behavior is Absorb (edge cells have fewer neighbours).
///
/// Distance is cube distance: `max(|dq|, |dr|, |dq + dr|)`, which equals
/// the graph geodesic on the hex grid.
///
/// Canonical ordering is r-then-q: outer loop over r, inner loop over q.
///
/// # Examples
///
/// ```
/// use murk_space::{Hex2D, Space};
///
/// let hex = Hex2D::new(5, 5).unwrap();
/// assert_eq!(hex.rows(), 5);
/// assert_eq!(hex.cols(), 5);
/// assert_eq!(hex.cell_count(), 25);
/// assert_eq!(hex.ndim(), 2);
///
/// // Interior cell has 6 neighbours.
/// let interior: murk_core::Coord = vec![2i32, 2].into();
/// assert_eq!(hex.neighbours(&interior).len(), 6);
///
/// // Corner cell has fewer neighbours (absorb boundary).
/// let corner: murk_core::Coord = vec![0i32, 0].into();
/// assert_eq!(hex.neighbours(&corner).len(), 2);
///
/// // Cube distance between adjacent cells is 1.
/// let a: murk_core::Coord = vec![2i32, 1].into();
/// let b: murk_core::Coord = vec![3i32, 1].into();
/// assert_eq!(hex.distance(&a, &b), 1.0);
/// ```
#[derive(Debug, Clone)]
pub struct Hex2D {
    rows: u32,
    cols: u32,
    instance_id: SpaceInstanceId,
}

impl Hex2D {
    /// Maximum dimension size: coordinates use `i32`, so each axis must fit.
    pub const MAX_DIM: u32 = i32::MAX as u32;

    /// Create a new hex grid with `rows * cols` cells.
    ///
    /// Returns `Err(SpaceError::EmptySpace)` if either dimension is 0, or
    /// `Err(SpaceError::DimensionTooLarge)` if either exceeds `i32::MAX`.
    pub fn new(rows: u32, cols: u32) -> Result<Self, SpaceError> {
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

    /// Check that a coordinate is in-bounds and return `(q, r)`.
    fn check_bounds(&self, coord: &Coord) -> Result<(i32, i32), SpaceError> {
        if coord.len() != 2 {
            return Err(SpaceError::CoordOutOfBounds {
                coord: coord.clone(),
                bounds: format!("expected 2D coordinate, got {}D", coord.len()),
            });
        }
        let q = coord[0];
        let r = coord[1];
        if q < 0 || q >= self.cols as i32 || r < 0 || r >= self.rows as i32 {
            return Err(SpaceError::CoordOutOfBounds {
                coord: coord.clone(),
                bounds: format!("q in [0, {}), r in [0, {})", self.cols, self.rows),
            });
        }
        Ok((q, r))
    }

    /// Compute the hex neighbours of `(q, r)`, filtering out-of-bounds (Absorb).
    fn neighbours_qr(&self, q: i32, r: i32) -> SmallVec<[(i32, i32); 6]> {
        let mut result = SmallVec::new();
        for (dq, dr) in HEX_OFFSETS {
            let nq = q + dq;
            let nr = r + dr;
            if nq >= 0 && nq < self.cols as i32 && nr >= 0 && nr < self.rows as i32 {
                result.push((nq, nr));
            }
        }
        result
    }

    /// Cube distance between two axial coordinates.
    fn cube_distance(q1: i32, r1: i32, q2: i32, r2: i32) -> i32 {
        let dq = (q1 - q2).abs();
        let dr = (r1 - r2).abs();
        let ds = ((q1 + r1) - (q2 + r2)).abs(); // |ds| where s = -q - r
        dq.max(dr).max(ds)
    }

    /// Compile a hex disk region via direct enumeration.
    fn compile_hex_disk(&self, center_q: i32, center_r: i32, radius: u32) -> RegionPlan {
        // Clamp effective radius to grid bounds to avoid overflow.
        // No cell can be further than (rows + cols) from center.
        let max_useful = (self.rows as u64 + self.cols as u64).min(i32::MAX as u64) as u32;
        let eff_radius = radius.min(max_useful);
        let r = eff_radius as i32;
        let side = 2i64 * r as i64 + 1;
        let bounding_size = (side * side) as usize;
        let mut valid_mask = vec![0u8; bounding_size];
        let mut coords = Vec::new();
        let mut tensor_indices = Vec::new();

        // Enumerate all (dq, dr) in the hex disk.
        for dr in -r..=r {
            for dq in -r..=r {
                if Self::cube_distance(0, 0, dq, dr) > r {
                    continue;
                }
                let q = center_q + dq;
                let rv = center_r + dr;
                if q < 0 || q >= self.cols as i32 || rv < 0 || rv >= self.rows as i32 {
                    continue;
                }
                let tensor_idx = ((dr + r) as i64 * side + (dq + r) as i64) as usize;
                valid_mask[tensor_idx] = 1;
                coords.push(smallvec![q, rv]);
                tensor_indices.push(tensor_idx);
            }
        }

        // Sort by (r, q) — canonical ordering for Hex2D.
        let mut pairs: Vec<(Coord, usize)> = coords.into_iter().zip(tensor_indices).collect();
        pairs.sort_by(|a, b| {
            let ar = a.0[1];
            let aq = a.0[0];
            let br = b.0[1];
            let bq = b.0[0];
            (ar, aq).cmp(&(br, bq))
        });
        let (coords, tensor_indices): (Vec<_>, Vec<_>) = pairs.into_iter().unzip();
        let cell_count = coords.len();

        RegionPlan {
            cell_count,
            coords,
            tensor_indices,
            valid_mask,
            bounding_shape: BoundingShape::Rect(vec![side as usize, side as usize]),
        }
    }
}

impl Space for Hex2D {
    fn ndim(&self) -> usize {
        2
    }

    fn cell_count(&self) -> usize {
        (self.rows as usize) * (self.cols as usize)
    }

    fn neighbours(&self, coord: &Coord) -> SmallVec<[Coord; 8]> {
        let q = coord[0];
        let r = coord[1];
        self.neighbours_qr(q, r)
            .into_iter()
            .map(|(nq, nr)| smallvec![nq, nr])
            .collect()
    }

    fn distance(&self, a: &Coord, b: &Coord) -> f64 {
        Self::cube_distance(a[0], a[1], b[0], b[1]) as f64
    }

    fn compile_region(&self, spec: &RegionSpec) -> Result<RegionPlan, SpaceError> {
        match spec {
            RegionSpec::All => {
                let coords = self.canonical_ordering();
                let cell_count = coords.len();
                let tensor_indices: Vec<usize> = (0..cell_count).collect();
                let valid_mask = vec![1u8; cell_count];
                Ok(RegionPlan {
                    cell_count,
                    coords,
                    tensor_indices,
                    valid_mask,
                    bounding_shape: BoundingShape::Rect(vec![
                        self.rows as usize,
                        self.cols as usize,
                    ]),
                })
            }

            RegionSpec::Disk { center, radius } => {
                let (cq, cr) = self.check_bounds(center)?;
                Ok(self.compile_hex_disk(cq, cr, *radius))
            }

            RegionSpec::Neighbours { center, depth } => {
                let (cq, cr) = self.check_bounds(center)?;
                Ok(self.compile_hex_disk(cq, cr, *depth))
            }

            RegionSpec::Rect { min, max } => {
                let (q_lo, r_lo) = self.check_bounds(min)?;
                let (q_hi, r_hi) = self.check_bounds(max)?;
                if q_lo > q_hi || r_lo > r_hi {
                    return Err(SpaceError::InvalidRegion {
                        reason: format!(
                            "Rect min ({q_lo},{r_lo}) > max ({q_hi},{r_hi}) on some axis"
                        ),
                    });
                }
                // Iterate in canonical order: r then q.
                let mut coords = Vec::new();
                for r in r_lo..=r_hi {
                    for q in q_lo..=q_hi {
                        coords.push(smallvec![q, r]);
                    }
                }
                let cell_count = coords.len();
                let tensor_indices: Vec<usize> = (0..cell_count).collect();
                let valid_mask = vec![1u8; cell_count];
                let shape_rows = (r_hi - r_lo + 1) as usize;
                let shape_cols = (q_hi - q_lo + 1) as usize;
                Ok(RegionPlan {
                    cell_count,
                    coords,
                    tensor_indices,
                    valid_mask,
                    bounding_shape: BoundingShape::Rect(vec![shape_rows, shape_cols]),
                })
            }

            RegionSpec::Coords(coords) => {
                for coord in coords {
                    self.check_bounds(coord)?;
                }
                let mut sorted: Vec<Coord> = coords.clone();
                // Sort by (r, q) — canonical ordering.
                sorted.sort_by(|a, b| (a[1], a[0]).cmp(&(b[1], b[0])));
                sorted.dedup();
                let cell_count = sorted.len();
                let tensor_indices: Vec<usize> = (0..cell_count).collect();
                let valid_mask = vec![1u8; cell_count];
                Ok(RegionPlan {
                    cell_count,
                    coords: sorted,
                    tensor_indices,
                    valid_mask,
                    bounding_shape: BoundingShape::Rect(vec![cell_count]),
                })
            }
        }
    }

    fn canonical_ordering(&self) -> Vec<Coord> {
        // r-then-q ordering: outer loop r, inner loop q.
        let mut out = Vec::with_capacity(self.cell_count());
        for r in 0..self.rows as i32 {
            for q in 0..self.cols as i32 {
                out.push(smallvec![q, r]);
            }
        }
        out
    }

    fn canonical_rank(&self, coord: &Coord) -> Option<usize> {
        if coord.len() != 2 {
            return None;
        }
        let q = coord[0];
        let r = coord[1];
        if q >= 0 && q < self.cols as i32 && r >= 0 && r < self.rows as i32 {
            Some(r as usize * self.cols as usize + q as usize)
        } else {
            None
        }
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

    fn c(q: i32, r: i32) -> Coord {
        smallvec![q, r]
    }

    // ── Neighbour tests ─────────────────────────────────────────

    #[test]
    fn neighbours_interior() {
        let s = Hex2D::new(5, 5).unwrap();
        let n = s.neighbours(&c(2, 1));
        assert_eq!(n.len(), 6);
        // HLD worked example: neighbours of (2,1)
        assert!(n.contains(&c(3, 1))); // E
        assert!(n.contains(&c(3, 0))); // NE
        assert!(n.contains(&c(2, 0))); // NW
        assert!(n.contains(&c(1, 1))); // W
        assert!(n.contains(&c(1, 2))); // SW
        assert!(n.contains(&c(2, 2))); // SE
    }

    #[test]
    fn neighbours_corner_origin() {
        let s = Hex2D::new(5, 5).unwrap();
        let n = s.neighbours(&c(0, 0));
        // (0,0): E=(1,0)ok, NE=(1,-1)oob, NW=(0,-1)oob, W=(-1,0)oob, SW=(-1,1)oob, SE=(0,1)ok
        assert_eq!(n.len(), 2);
        assert!(n.contains(&c(1, 0)));
        assert!(n.contains(&c(0, 1)));
    }

    #[test]
    fn neighbours_top_edge() {
        let s = Hex2D::new(5, 5).unwrap();
        let n = s.neighbours(&c(2, 0));
        // (2,0): E=(3,0)ok, NE=(3,-1)oob, NW=(2,-1)oob, W=(1,0)ok, SW=(1,1)ok, SE=(2,1)ok
        assert_eq!(n.len(), 4);
        assert!(n.contains(&c(3, 0)));
        assert!(n.contains(&c(1, 0)));
        assert!(n.contains(&c(1, 1)));
        assert!(n.contains(&c(2, 1)));
    }

    #[test]
    fn neighbours_bottom_right_corner() {
        let s = Hex2D::new(5, 5).unwrap();
        let n = s.neighbours(&c(4, 4));
        // (4,4): E=(5,4)oob, NE=(5,3)oob, NW=(4,3)ok, W=(3,4)ok, SW=(3,5)oob, SE=(4,5)oob
        assert_eq!(n.len(), 2);
        assert!(n.contains(&c(4, 3)));
        assert!(n.contains(&c(3, 4)));
    }

    // ── Distance tests ──────────────────────────────────────────

    #[test]
    fn distance_same_cell() {
        let s = Hex2D::new(5, 5).unwrap();
        assert_eq!(s.distance(&c(2, 1), &c(2, 1)), 0.0);
    }

    #[test]
    fn distance_adjacent() {
        let s = Hex2D::new(5, 5).unwrap();
        assert_eq!(s.distance(&c(2, 1), &c(3, 1)), 1.0); // E
        assert_eq!(s.distance(&c(2, 1), &c(3, 0)), 1.0); // NE
    }

    #[test]
    fn distance_hld_worked_example() {
        // HLD: distance((2,1), (4,0)) = 2
        let s = Hex2D::new(5, 5).unwrap();
        assert_eq!(s.distance(&c(2, 1), &c(4, 0)), 2.0);
    }

    #[test]
    fn distance_across_grid() {
        let s = Hex2D::new(5, 5).unwrap();
        // (0,0) -> (4,4): dq=4, dr=4, ds=|4+4|=8 -> max(4,4,8)=8
        assert_eq!(s.distance(&c(0, 0), &c(4, 4)), 8.0);
    }

    // ── Region tests ────────────────────────────────────────────

    #[test]
    fn compile_region_all() {
        let s = Hex2D::new(5, 5).unwrap();
        let plan = s.compile_region(&RegionSpec::All).unwrap();
        assert_eq!(plan.cell_count, 25);
        assert_eq!(plan.valid_ratio(), 1.0);
    }

    #[test]
    fn compile_region_disk_r1() {
        let s = Hex2D::new(10, 10).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Disk {
                center: c(5, 5),
                radius: 1,
            })
            .unwrap();
        // Hex disk R=1: center + 6 neighbours = 7
        assert_eq!(plan.cell_count, 7);
    }

    #[test]
    fn compile_region_disk_r2() {
        let s = Hex2D::new(10, 10).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Disk {
                center: c(5, 5),
                radius: 2,
            })
            .unwrap();
        // Hex disk R=2: 3*4+3*2+1 = 19 cells
        assert_eq!(plan.cell_count, 19);
    }

    #[test]
    fn compile_region_disk_valid_ratio_r1() {
        let s = Hex2D::new(10, 10).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Disk {
                center: c(5, 5),
                radius: 1,
            })
            .unwrap();
        // Bounding: 3x3=9, valid=7 -> 7/9 ≈ 0.778
        let ratio = plan.valid_ratio();
        assert!((ratio - 7.0 / 9.0).abs() < 0.01, "valid_ratio={ratio}");
    }

    #[test]
    fn compile_region_disk_valid_ratio_r2() {
        let s = Hex2D::new(10, 10).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Disk {
                center: c(5, 5),
                radius: 2,
            })
            .unwrap();
        // Bounding: 5x5=25, valid=19 -> 19/25 = 0.76
        let ratio = plan.valid_ratio();
        assert!((ratio - 19.0 / 25.0).abs() < 0.01, "valid_ratio={ratio}");
    }

    #[test]
    fn compile_region_disk_boundary_truncation() {
        let s = Hex2D::new(5, 5).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Disk {
                center: c(0, 0),
                radius: 2,
            })
            .unwrap();
        // Corner: many cells clipped by boundary.
        assert!(plan.cell_count < 19);
        assert!(plan.cell_count >= 1);
    }

    #[test]
    fn compile_region_disk_huge_radius_does_not_overflow() {
        // Radius larger than grid — should clamp and return all cells.
        let s = Hex2D::new(3, 3).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Disk {
                center: c(1, 1),
                radius: u32::MAX,
            })
            .unwrap();
        assert_eq!(plan.cell_count, 9);
    }

    #[test]
    fn compile_region_rect() {
        let s = Hex2D::new(10, 10).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Rect {
                min: c(2, 3),
                max: c(5, 6),
            })
            .unwrap();
        // 4 cols * 4 rows = 16
        assert_eq!(plan.cell_count, 16);
    }

    #[test]
    fn compile_region_rect_invalid() {
        let s = Hex2D::new(10, 10).unwrap();
        assert!(s
            .compile_region(&RegionSpec::Rect {
                min: c(5, 0),
                max: c(2, 3),
            })
            .is_err());
    }

    #[test]
    fn compile_region_coords() {
        let s = Hex2D::new(5, 5).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Coords(vec![c(3, 1), c(1, 2), c(0, 0)]))
            .unwrap();
        // Sorted by (r, q): (0,0), (3,1), (1,2)
        assert_eq!(plan.coords, vec![c(0, 0), c(3, 1), c(1, 2)]);
    }

    #[test]
    fn compile_region_coords_oob() {
        let s = Hex2D::new(5, 5).unwrap();
        assert!(s
            .compile_region(&RegionSpec::Coords(vec![c(10, 0)]))
            .is_err());
    }

    // ── Constructor tests ───────────────────────────────────────

    #[test]
    fn new_zero_rows_returns_error() {
        assert!(matches!(Hex2D::new(0, 5), Err(SpaceError::EmptySpace)));
    }

    #[test]
    fn new_zero_cols_returns_error() {
        assert!(matches!(Hex2D::new(5, 0), Err(SpaceError::EmptySpace)));
    }

    #[test]
    fn new_rejects_dims_exceeding_i32_max() {
        let big = i32::MAX as u32 + 1;
        assert!(matches!(
            Hex2D::new(big, 5),
            Err(SpaceError::DimensionTooLarge { name: "rows", .. })
        ));
        assert!(matches!(
            Hex2D::new(5, big),
            Err(SpaceError::DimensionTooLarge { name: "cols", .. })
        ));
        assert!(Hex2D::new(i32::MAX as u32, 1).is_ok());
    }

    // ── 1×1 edge case ──────────────────────────────────────────

    #[test]
    fn single_cell() {
        let s = Hex2D::new(1, 1).unwrap();
        assert!(s.neighbours(&c(0, 0)).is_empty());
        assert_eq!(s.cell_count(), 1);
        assert_eq!(s.distance(&c(0, 0), &c(0, 0)), 0.0);
    }

    // ── Canonical ordering ─────────────────────────────────────

    #[test]
    fn canonical_ordering_r_then_q() {
        let s = Hex2D::new(3, 3).unwrap();
        let order = s.canonical_ordering();
        // r=0: (0,0),(1,0),(2,0)  r=1: (0,1),(1,1),(2,1)  r=2: (0,2),(1,2),(2,2)
        assert_eq!(
            order,
            vec![
                c(0, 0),
                c(1, 0),
                c(2, 0),
                c(0, 1),
                c(1, 1),
                c(2, 1),
                c(0, 2),
                c(1, 2),
                c(2, 2),
            ]
        );
    }

    // ── Compliance suites ───────────────────────────────────────

    #[test]
    fn compliance_3x3() {
        let s = Hex2D::new(3, 3).unwrap();
        compliance::run_full_compliance(&s);
    }

    #[test]
    fn compliance_5x5() {
        let s = Hex2D::new(5, 5).unwrap();
        compliance::run_full_compliance(&s);
    }

    #[test]
    fn compliance_8x8() {
        let s = Hex2D::new(8, 8).unwrap();
        compliance::run_full_compliance(&s);
    }

    // ── Downcast test ───────────────────────────────────────────

    #[test]
    fn downcast_ref_hex2d() {
        let s: Box<dyn Space> = Box::new(Hex2D::new(3, 3).unwrap());
        assert!(s.downcast_ref::<Hex2D>().is_some());
        assert!(s.downcast_ref::<crate::Square4>().is_none());
    }

    // ── Property tests ──────────────────────────────────────────

    proptest! {
        #[test]
        fn distance_is_metric(
            rows in 2u32..8,
            cols in 2u32..8,
            aq in 0i32..8, ar in 0i32..8,
            bq in 0i32..8, br in 0i32..8,
            cq in 0i32..8, cr in 0i32..8,
        ) {
            let aq = aq % cols as i32;
            let ar = ar % rows as i32;
            let bq = bq % cols as i32;
            let br = br % rows as i32;
            let cq = cq % cols as i32;
            let cr = cr % rows as i32;
            let s = Hex2D::new(rows, cols).unwrap();
            let a: Coord = smallvec![aq, ar];
            let b: Coord = smallvec![bq, br];
            let cv: Coord = smallvec![cq, cr];

            prop_assert!((s.distance(&a, &a) - 0.0).abs() < f64::EPSILON);
            prop_assert!((s.distance(&a, &b) - s.distance(&b, &a)).abs() < f64::EPSILON);
            prop_assert!(s.distance(&a, &cv) <= s.distance(&a, &b) + s.distance(&b, &cv) + f64::EPSILON);
        }

        #[test]
        fn neighbours_symmetric(
            rows in 2u32..8,
            cols in 2u32..8,
            q in 0i32..8, r in 0i32..8,
        ) {
            let q = q % cols as i32;
            let r = r % rows as i32;
            let s = Hex2D::new(rows, cols).unwrap();
            let coord: Coord = smallvec![q, r];
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
