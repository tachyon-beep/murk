//! 3D face-centred cubic (FCC) lattice with 12-connected neighbourhood.
//!
//! The FCC lattice is the 3D analogue of [`Hex2D`](crate::Hex2D): every cell
//! has 12 equidistant neighbours, giving minimal directional bias for diffusion
//! stencils, foveation regions, and agent movement.
//!
//! # Coordinate System
//!
//! Integer triples `(x, y, z)` with the parity constraint `(x + y + z) % 2 == 0`.
//! This selects exactly half the integer lattice points (a 3D checkerboard).
//!
//! # Edge Behavior
//!
//! - **Absorb**: offsets that go out of bounds are dropped.
//! - **Clamp**: degrades to Absorb at FCC boundaries. Clamping a single axis
//!   cancels one of the two ±1 changes in an FCC offset, which flips parity
//!   and produces an invalid coordinate. To prevent this, any offset that
//!   would clamp on any axis is dropped entirely.
//! - **Wrap**: requires even dimensions for parity consistency.
//!
//! # CFL Stability
//!
//! For a graph Laplacian with degree 12, the explicit Euler stability
//! bound is `degree * D * dt / h² < 1`. With FCC's Euclidean spacing
//! of `h = √2`, this gives `6 * D * dt < 1`. With unit graph spacing
//! (`h = 1`), this gives `12 * D * dt < 1`. Choose the convention
//! that matches your propagator's stencil weights.

use crate::edge::EdgeBehavior;
use crate::error::SpaceError;
use crate::region::{BoundingShape, RegionPlan, RegionSpec};
use crate::space::Space;
use murk_core::{Coord, SpaceInstanceId};
use smallvec::{smallvec, SmallVec};
use std::collections::VecDeque;

/// All 12 FCC neighbour offsets: permutations of `(±1, ±1, 0)`.
const FCC_OFFSETS: [(i32, i32, i32); 12] = [
    (1, 1, 0),
    (-1, 1, 0),
    (1, -1, 0),
    (-1, -1, 0),
    (1, 0, 1),
    (-1, 0, 1),
    (1, 0, -1),
    (-1, 0, -1),
    (0, 1, 1),
    (0, -1, 1),
    (0, 1, -1),
    (0, -1, -1),
];

/// A three-dimensional face-centred cubic lattice with 12-connected neighbourhood.
///
/// Each cell has coordinate `[x, y, z]` where `0 <= x < w`, `0 <= y < h`,
/// `0 <= z < d`, and `(x + y + z) % 2 == 0`.
///
/// Distance is `max(max(|dx|, |dy|, |dz|), (|dx| + |dy| + |dz|) / 2)` —
/// **not** L∞ (Chebyshev). Each FCC step changes exactly two axes, so the
/// half-L1 lower bound can dominate (e.g. `(0,0,0)→(2,2,2)` = 3, not 2).
///
/// Canonical ordering is z-then-y-then-x, skipping invalid parity.
#[derive(Debug, Clone)]
pub struct Fcc12 {
    /// Extent along x-axis. Valid x: `0..w`, filtered by parity.
    w: u32,
    /// Extent along y-axis.
    h: u32,
    /// Extent along z-axis.
    d: u32,
    /// Precomputed cell count (valid parity cells only).
    cell_count: usize,
    /// Edge behavior.
    edge: EdgeBehavior,
    instance_id: SpaceInstanceId,
}

impl Fcc12 {
    /// Maximum dimension size: coordinates use `i32`, so each axis must fit.
    pub const MAX_DIM: u32 = i32::MAX as u32;

    /// Create a new FCC lattice with dimensions `w × h × d` and given edge behavior.
    ///
    /// Returns `Err(SpaceError::EmptySpace)` if any dimension is 0,
    /// `Err(SpaceError::DimensionTooLarge)` if any exceeds `i32::MAX`, or
    /// `Err(SpaceError::InvalidComposition)` if Wrap is used with odd dimensions.
    pub fn new(w: u32, h: u32, d: u32, edge: EdgeBehavior) -> Result<Self, SpaceError> {
        if w == 0 || h == 0 || d == 0 {
            return Err(SpaceError::EmptySpace);
        }
        for (name, val) in [("w", w), ("h", h), ("d", d)] {
            if val > Self::MAX_DIM {
                return Err(SpaceError::DimensionTooLarge {
                    name,
                    value: val,
                    max: Self::MAX_DIM,
                });
            }
        }

        // Wrap requires even dimensions for parity consistency.
        if edge == EdgeBehavior::Wrap && (w % 2 != 0 || h % 2 != 0 || d % 2 != 0) {
            return Err(SpaceError::InvalidComposition {
                reason: "FCC12 with Wrap requires even dimensions for parity consistency".into(),
            });
        }

        let cell_count = count_fcc_cells_checked(w, h, d).ok_or_else(|| {
            SpaceError::InvalidComposition {
                reason: format!("FCC12 {w}x{h}x{d} exceeds maximum cell count"),
            }
        })?;

        Ok(Self {
            w,
            h,
            d,
            cell_count,
            edge,
            instance_id: SpaceInstanceId::next(),
        })
    }

    /// Width (x-axis extent).
    pub fn w(&self) -> u32 {
        self.w
    }

    /// Height (y-axis extent).
    pub fn h(&self) -> u32 {
        self.h
    }

    /// Depth (z-axis extent).
    pub fn d(&self) -> u32 {
        self.d
    }

    /// Edge behavior.
    pub fn edge_behavior(&self) -> EdgeBehavior {
        self.edge
    }

    /// Always returns `false` — construction rejects empty grids.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Check that a coordinate is in-bounds, has valid parity, and return `(x, y, z)`.
    fn check_bounds(&self, coord: &Coord) -> Result<(i32, i32, i32), SpaceError> {
        if coord.len() != 3 {
            return Err(SpaceError::CoordOutOfBounds {
                coord: coord.clone(),
                bounds: format!("expected 3D coordinate, got {}D", coord.len()),
            });
        }
        let (x, y, z) = (coord[0], coord[1], coord[2]);
        if x < 0
            || x >= self.w as i32
            || y < 0
            || y >= self.h as i32
            || z < 0
            || z >= self.d as i32
        {
            return Err(SpaceError::CoordOutOfBounds {
                coord: coord.clone(),
                bounds: format!("x in [0, {}), y in [0, {}), z in [0, {})", self.w, self.h, self.d),
            });
        }
        if (x + y + z) % 2 != 0 {
            return Err(SpaceError::CoordOutOfBounds {
                coord: coord.clone(),
                bounds: "(x + y + z) must be even (FCC parity constraint)".into(),
            });
        }
        Ok((x, y, z))
    }

    /// BFS-based disk compilation for FCC lattice.
    fn compile_fcc_disk(&self, cx: i32, cy: i32, cz: i32, radius: u32) -> RegionPlan {
        let mut visited = vec![false; self.cell_count];
        let mut queue = VecDeque::new();
        let mut result: Vec<Coord> = Vec::new();

        let center: Coord = smallvec![cx, cy, cz];
        let center_rank = self
            .canonical_rank(&center)
            .expect("disk center must be a valid FCC coord");
        visited[center_rank] = true;
        queue.push_back((center.clone(), 0u32));
        result.push(center);

        while let Some((here, dist)) = queue.pop_front() {
            if dist >= radius {
                continue;
            }
            for n in self.neighbours(&here) {
                let rank = self
                    .canonical_rank(&n)
                    .expect("neighbours() must only yield valid FCC coords");
                if !visited[rank] {
                    visited[rank] = true;
                    queue.push_back((n.clone(), dist + 1));
                    result.push(n);
                }
            }
        }

        // Sort by canonical rank for deterministic order.
        result.sort_by_key(|c| self.canonical_rank(c).unwrap());
        let cell_count = result.len();
        let tensor_indices: Vec<usize> = (0..cell_count).collect();
        let valid_mask = vec![1u8; cell_count];

        RegionPlan {
            cell_count,
            coords: result,
            tensor_indices,
            valid_mask,
            bounding_shape: BoundingShape::Rect(vec![cell_count]),
        }
    }
}

impl Space for Fcc12 {
    fn ndim(&self) -> usize {
        3
    }

    fn cell_count(&self) -> usize {
        self.cell_count
    }

    fn neighbours(&self, coord: &Coord) -> SmallVec<[Coord; 8]> {
        let (x, y, z) = (coord[0], coord[1], coord[2]);
        let mut result = SmallVec::new();
        for (dx, dy, dz) in FCC_OFFSETS {
            let (nx, x_clamped) = resolve_axis_fcc(x + dx, self.w, self.edge);
            let (ny, y_clamped) = resolve_axis_fcc(y + dy, self.h, self.edge);
            let (nz, z_clamped) = resolve_axis_fcc(z + dz, self.d, self.edge);
            // Drop the move if any axis was absorbed OR clamped.
            // Clamping cancels one of the two ±1 changes, breaking parity.
            match (nx, ny, nz) {
                (Some(nx), Some(ny), Some(nz))
                    if !(x_clamped || y_clamped || z_clamped) =>
                {
                    result.push(smallvec![nx, ny, nz]);
                }
                _ => {}
            }
        }
        result
    }

    fn distance(&self, a: &Coord, b: &Coord) -> f64 {
        let dx = axis_distance_u32(a[0], b[0], self.w, self.edge);
        let dy = axis_distance_u32(a[1], b[1], self.h, self.edge);
        let dz = axis_distance_u32(a[2], b[2], self.d, self.edge);

        let max_abs = dx.max(dy).max(dz);
        let half_l1 = (dx + dy + dz) / 2; // exact: L1 always even between valid cells

        f64::from(max_abs.max(half_l1))
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
                    bounding_shape: BoundingShape::Rect(vec![cell_count]),
                })
            }

            RegionSpec::Disk { center, radius } => {
                let (cx, cy, cz) = self.check_bounds(center)?;
                Ok(self.compile_fcc_disk(cx, cy, cz, *radius))
            }

            RegionSpec::Neighbours { center, depth } => {
                let (cx, cy, cz) = self.check_bounds(center)?;
                Ok(self.compile_fcc_disk(cx, cy, cz, *depth))
            }

            RegionSpec::Rect { min, max } => {
                let (x_lo, y_lo, z_lo) = self.check_bounds(min)?;
                let (x_hi, y_hi, z_hi) = self.check_bounds(max)?;
                if x_lo > x_hi || y_lo > y_hi || z_lo > z_hi {
                    return Err(SpaceError::InvalidRegion {
                        reason: format!(
                            "Rect min ({x_lo},{y_lo},{z_lo}) > max ({x_hi},{y_hi},{z_hi}) on some axis"
                        ),
                    });
                }
                let mut coords = Vec::new();
                for z in z_lo..=z_hi {
                    for y in y_lo..=y_hi {
                        // First valid x >= x_lo with correct parity.
                        // (x_lo + y + z) % 2 is always 0 or 1 here because all
                        // values are non-negative (check_bounds guarantees >= 0).
                        let x_start = x_lo + ((x_lo + y + z) % 2);
                        let mut x = x_start;
                        while x <= x_hi {
                            coords.push(smallvec![x, y, z]);
                            x += 2;
                        }
                    }
                }
                let cell_count = coords.len();
                let tensor_indices: Vec<usize> = (0..cell_count).collect();
                let valid_mask = vec![1u8; cell_count];
                Ok(RegionPlan {
                    cell_count,
                    coords,
                    tensor_indices,
                    valid_mask,
                    bounding_shape: BoundingShape::Rect(vec![cell_count]),
                })
            }

            RegionSpec::Coords(coords) => {
                for coord in coords {
                    self.check_bounds(coord)?;
                }
                let mut sorted: Vec<Coord> = coords.clone();
                sorted.sort_by_key(|c| self.canonical_rank(c).unwrap());
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
        let mut out = Vec::with_capacity(self.cell_count);
        for z in 0..self.d as i32 {
            for y in 0..self.h as i32 {
                let x_start = ((y + z) % 2) as i32;
                let mut x = x_start;
                while x < self.w as i32 {
                    out.push(smallvec![x, y, z]);
                    x += 2;
                }
            }
        }
        debug_assert_eq!(
            out.len(),
            self.cell_count,
            "canonical_ordering produced {} cells but cell_count is {}",
            out.len(),
            self.cell_count
        );
        out
    }

    fn canonical_rank(&self, coord: &Coord) -> Option<usize> {
        if coord.len() != 3 {
            return None;
        }
        let (x, y, z) = (coord[0], coord[1], coord[2]);

        // Bounds check.
        if x < 0
            || x >= self.w as i32
            || y < 0
            || y >= self.h as i32
            || z < 0
            || z >= self.d as i32
        {
            return None;
        }

        // Parity check.
        if (x + y + z) % 2 != 0 {
            return None;
        }

        let w = self.w as usize;
        let x_even = (w + 1) / 2; // valid x count when row start = 0
        let x_odd = w / 2; // valid x count when row start = 1

        let h = self.h as usize;
        let y_even_rows = (h + 1) / 2; // count of even-index y rows
        let y_odd_rows = h / 2; // count of odd-index y rows

        // Two slice sizes: slice cell count depends on z parity.
        // When z is even: even y-rows have start=0 (x_even cells),
        //                 odd y-rows have start=1 (x_odd cells).
        // When z is odd:  even y-rows have start=1 (x_odd cells),
        //                 odd y-rows have start=0 (x_even cells).
        let slice_even = y_even_rows * x_even + y_odd_rows * x_odd;
        let slice_odd = y_even_rows * x_odd + y_odd_rows * x_even;

        // Count cells in all complete z-slices before this one.
        let z_us = z as usize;
        let z_even_ct = (z_us + 1) / 2; // even z values in [0, z): 0, 2, 4, ...
        let z_odd_ct = z_us / 2; // odd z values in [0, z): 1, 3, 5, ...
        let cells_before_z = z_even_ct * slice_even + z_odd_ct * slice_odd;

        // Count cells in complete y-rows within this z-slice.
        let y_us = y as usize;
        let z_parity = (z & 1) as usize;
        let y_even_ct = (y_us + 1) / 2; // even y values in [0, y)
        let y_odd_ct = y_us / 2; // odd y values in [0, y)
        let cells_before_y = if z_parity == 0 {
            y_even_ct * x_even + y_odd_ct * x_odd
        } else {
            y_even_ct * x_odd + y_odd_ct * x_even
        };

        // Count cells before x in this row.
        let x_start = ((y + z) & 1) as i32;
        let cells_before_x = ((x - x_start) / 2) as usize;

        Some(cells_before_z + cells_before_y + cells_before_x)
    }

    fn instance_id(&self) -> SpaceInstanceId {
        self.instance_id
    }
}

// ── Private helpers ──────────────────────────────────────────────

/// Count valid FCC cells for dimensions `w × h × d` with overflow protection.
fn count_fcc_cells_checked(w: u32, h: u32, d: u32) -> Option<usize> {
    let hd = (h as usize).checked_mul(d as usize)?;
    let n_even_rows = (hd + 1) / 2; // rows where (y+z) % 2 == 0
    let n_odd_rows = hd / 2; // rows where (y+z) % 2 == 1
    let x_even = ((w as usize) + 1) / 2; // valid x count when start=0
    let x_odd = (w as usize) / 2; // valid x count when start=1
    let a = n_even_rows.checked_mul(x_even)?;
    let b = n_odd_rows.checked_mul(x_odd)?;
    a.checked_add(b)
}

/// Resolve a single axis value for FCC, reporting whether clamping occurred.
///
/// Returns `(Some(resolved), clamped)` or `(None, false)` for Absorb out-of-bounds.
fn resolve_axis_fcc(val: i32, len: u32, edge: EdgeBehavior) -> (Option<i32>, bool) {
    let n = len as i32;
    if val >= 0 && val < n {
        return (Some(val), false);
    }
    match edge {
        EdgeBehavior::Absorb => (None, false),
        EdgeBehavior::Clamp => (Some(val.clamp(0, n - 1)), true),
        EdgeBehavior::Wrap => (Some(((val % n) + n) % n), false),
    }
}

/// Per-axis absolute displacement in `u32`, accounting for wrap.
fn axis_distance_u32(a: i32, b: i32, len: u32, edge: EdgeBehavior) -> u32 {
    debug_assert!(len > 0, "axis_distance_u32 called with zero-length axis");
    let diff = (a - b).unsigned_abs();
    debug_assert!(
        diff < len,
        "axis_distance_u32: diff {diff} >= len {len} (out-of-bounds coord?)"
    );
    match edge {
        EdgeBehavior::Wrap => diff.min(len - diff),
        EdgeBehavior::Absorb | EdgeBehavior::Clamp => diff,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compliance;
    use murk_core::Coord;
    use proptest::prelude::*;

    fn c(x: i32, y: i32, z: i32) -> Coord {
        smallvec![x, y, z]
    }

    // ── Constructor tests ─────────────────────────────────────────

    #[test]
    fn new_zero_dim() {
        assert!(matches!(
            Fcc12::new(0, 4, 4, EdgeBehavior::Absorb),
            Err(SpaceError::EmptySpace)
        ));
        assert!(matches!(
            Fcc12::new(4, 0, 4, EdgeBehavior::Absorb),
            Err(SpaceError::EmptySpace)
        ));
        assert!(matches!(
            Fcc12::new(4, 4, 0, EdgeBehavior::Absorb),
            Err(SpaceError::EmptySpace)
        ));
    }

    #[test]
    fn new_dim_too_large() {
        let big = i32::MAX as u32 + 1;
        assert!(matches!(
            Fcc12::new(big, 4, 4, EdgeBehavior::Absorb),
            Err(SpaceError::DimensionTooLarge { name: "w", .. })
        ));
        assert!(matches!(
            Fcc12::new(4, big, 4, EdgeBehavior::Absorb),
            Err(SpaceError::DimensionTooLarge { name: "h", .. })
        ));
        assert!(matches!(
            Fcc12::new(4, 4, big, EdgeBehavior::Absorb),
            Err(SpaceError::DimensionTooLarge { name: "d", .. })
        ));
    }

    #[test]
    fn new_wrap_odd_dim() {
        assert!(matches!(
            Fcc12::new(3, 4, 4, EdgeBehavior::Wrap),
            Err(SpaceError::InvalidComposition { .. })
        ));
        assert!(matches!(
            Fcc12::new(4, 3, 4, EdgeBehavior::Wrap),
            Err(SpaceError::InvalidComposition { .. })
        ));
        assert!(matches!(
            Fcc12::new(4, 4, 3, EdgeBehavior::Wrap),
            Err(SpaceError::InvalidComposition { .. })
        ));
        // Even dims should succeed.
        assert!(Fcc12::new(4, 4, 4, EdgeBehavior::Wrap).is_ok());
    }

    // ── Cell count tests ──────────────────────────────────────────

    #[test]
    fn cell_count_small() {
        let s = Fcc12::new(2, 2, 2, EdgeBehavior::Absorb).unwrap();
        assert_eq!(s.cell_count(), 4);
    }

    #[test]
    fn cell_count_formula() {
        // 4x4x4: all even dims, hd=16, n_even=8, n_odd=8, x_even=2, x_odd=2
        // total = 8*2 + 8*2 = 32 = 64/2
        let s = Fcc12::new(4, 4, 4, EdgeBehavior::Absorb).unwrap();
        assert_eq!(s.cell_count(), 32);

        // Verify against manual enumeration.
        let ordering = s.canonical_ordering();
        assert_eq!(ordering.len(), 32);
    }

    #[test]
    fn cell_count_odd_dims() {
        // 5x5x5: alternating slice sizes (13, 12, 13, 12, 13) = 63
        let s = Fcc12::new(5, 5, 5, EdgeBehavior::Absorb).unwrap();
        assert_eq!(s.cell_count(), 63);
    }

    #[test]
    fn single_cell() {
        // 1x1x1: only (0,0,0) with sum=0 even.
        let s = Fcc12::new(1, 1, 1, EdgeBehavior::Absorb).unwrap();
        assert_eq!(s.cell_count(), 1);
        assert!(s.neighbours(&c(0, 0, 0)).is_empty());
        assert_eq!(s.distance(&c(0, 0, 0), &c(0, 0, 0)), 0.0);
    }

    // ── Canonical ordering & rank ─────────────────────────────────

    #[test]
    fn canonical_ordering_lex() {
        let s = Fcc12::new(3, 3, 3, EdgeBehavior::Absorb).unwrap();
        let order = s.canonical_ordering();
        // z=0: y=0: x=0,2; y=1: x=1; y=2: x=0,2
        // z=1: y=0: x=1; y=1: x=0,2; y=2: x=1
        // z=2: y=0: x=0,2; y=1: x=1; y=2: x=0,2
        let expected = vec![
            // z=0
            c(0, 0, 0),
            c(2, 0, 0),
            c(1, 1, 0),
            c(0, 2, 0),
            c(2, 2, 0),
            // z=1
            c(1, 0, 1),
            c(0, 1, 1),
            c(2, 1, 1),
            c(1, 2, 1),
            // z=2
            c(0, 0, 2),
            c(2, 0, 2),
            c(1, 1, 2),
            c(0, 2, 2),
            c(2, 2, 2),
        ];
        assert_eq!(order, expected);
    }

    #[test]
    fn canonical_rank_roundtrip() {
        let s = Fcc12::new(4, 4, 4, EdgeBehavior::Absorb).unwrap();
        let ordering = s.canonical_ordering();
        for (i, coord) in ordering.iter().enumerate() {
            assert_eq!(
                s.canonical_rank(coord),
                Some(i),
                "rank({coord:?}) should be {i}"
            );
        }
    }

    #[test]
    fn canonical_rank_odd_dims() {
        let s = Fcc12::new(5, 5, 5, EdgeBehavior::Absorb).unwrap();
        let ordering = s.canonical_ordering();
        assert_eq!(ordering.len(), 63);
        for (i, coord) in ordering.iter().enumerate() {
            assert_eq!(
                s.canonical_rank(coord),
                Some(i),
                "rank({coord:?}) should be {i} (5x5x5)"
            );
        }
    }

    // ── Neighbour tests ───────────────────────────────────────────

    #[test]
    fn neighbours_interior() {
        let s = Fcc12::new(6, 6, 6, EdgeBehavior::Absorb).unwrap();
        let n = s.neighbours(&c(2, 2, 2));
        assert_eq!(n.len(), 12, "interior cell should have 12 neighbours");
        // Check all 12 offsets are present.
        for (dx, dy, dz) in FCC_OFFSETS {
            let expected = c(2 + dx, 2 + dy, 2 + dz);
            assert!(
                n.contains(&expected),
                "missing neighbour ({}, {}, {})",
                2 + dx,
                2 + dy,
                2 + dz
            );
        }
    }

    #[test]
    fn neighbours_corner_origin() {
        let s = Fcc12::new(4, 4, 4, EdgeBehavior::Absorb).unwrap();
        let n = s.neighbours(&c(0, 0, 0));
        // (0,0,0) with Absorb: offsets with any negative component are dropped.
        // Valid: (+1,+1,0), (+1,0,+1), (0,+1,+1) = 3 neighbours.
        assert_eq!(n.len(), 3, "corner origin should have 3 neighbours with Absorb");
        assert!(n.contains(&c(1, 1, 0)));
        assert!(n.contains(&c(1, 0, 1)));
        assert!(n.contains(&c(0, 1, 1)));
    }

    #[test]
    fn neighbours_face() {
        // Cell on a face (not edge, not corner): e.g. (2, 2, 0) in a 6x6x6 grid.
        // z=0 face: offsets with dz=-1 are out of bounds.
        let s = Fcc12::new(6, 6, 6, EdgeBehavior::Absorb).unwrap();
        let n = s.neighbours(&c(2, 2, 0));
        // 4 offsets have dz=0 (all valid), 4 have dz=+1 (all valid), 4 have dz=-1 (dropped)
        assert_eq!(n.len(), 8, "face cell should have 8 neighbours with Absorb");
    }

    #[test]
    fn neighbours_edge() {
        // Cell on a 3D edge (intersection of 2 faces): e.g. (2, 0, 0) in 6x6x6.
        // y=0 and z=0: offsets with dy<0 or dz<0 are dropped.
        let s = Fcc12::new(6, 6, 6, EdgeBehavior::Absorb).unwrap();
        let n = s.neighbours(&c(2, 0, 0));
        // Offsets that survive: dz≥0 AND dy≥0.
        // (±1,+1,0): 2, (+1,0,+1),(-1,0,+1): 2, (0,+1,+1): 1 = 5
        assert_eq!(n.len(), 5, "edge cell should have 5 neighbours with Absorb");
    }

    #[test]
    fn neighbours_clamp_drops_invalid() {
        // Clamp at boundary should drop moves that would break parity.
        let s = Fcc12::new(4, 4, 4, EdgeBehavior::Clamp).unwrap();
        let n = s.neighbours(&c(0, 0, 0));
        // Clamp degrades to Absorb for FCC, so same as Absorb.
        assert_eq!(n.len(), 3, "Clamp at corner should drop parity-breaking moves");
    }

    #[test]
    fn neighbours_clamp_interior() {
        // Clamp shouldn't affect interior cells at all.
        let s = Fcc12::new(6, 6, 6, EdgeBehavior::Clamp).unwrap();
        let n = s.neighbours(&c(2, 2, 2));
        assert_eq!(n.len(), 12, "Clamp shouldn't reduce interior neighbours");
    }

    #[test]
    fn clamp_equals_absorb_boundary() {
        // Clamp must produce identical neighbour sets to Absorb at boundaries.
        let s_clamp = Fcc12::new(4, 4, 4, EdgeBehavior::Clamp).unwrap();
        let s_absorb = Fcc12::new(4, 4, 4, EdgeBehavior::Absorb).unwrap();

        // Check several boundary coords.
        let boundary_coords = [
            c(0, 0, 0),
            c(3, 3, 2),
            c(0, 2, 0),
            c(2, 0, 2),
            c(0, 3, 3),
        ];
        for coord in &boundary_coords {
            let mut n_clamp: Vec<Coord> = s_clamp.neighbours(coord).into_vec();
            let mut n_absorb: Vec<Coord> = s_absorb.neighbours(coord).into_vec();
            n_clamp.sort();
            n_absorb.sort();
            assert_eq!(
                n_clamp, n_absorb,
                "Clamp and Absorb should produce identical neighbours for {coord:?}"
            );
        }
    }

    // ── Distance tests ────────────────────────────────────────────

    #[test]
    fn distance_same_cell() {
        let s = Fcc12::new(6, 6, 6, EdgeBehavior::Absorb).unwrap();
        assert_eq!(s.distance(&c(2, 2, 2), &c(2, 2, 2)), 0.0);
    }

    #[test]
    fn distance_adjacent() {
        let s = Fcc12::new(6, 6, 6, EdgeBehavior::Absorb).unwrap();
        let center = c(2, 2, 2);
        for (dx, dy, dz) in FCC_OFFSETS {
            let nb = c(2 + dx, 2 + dy, 2 + dz);
            assert_eq!(
                s.distance(&center, &nb),
                1.0,
                "distance to neighbour ({dx},{dy},{dz}) should be 1"
            );
        }
    }

    #[test]
    fn distance_two_steps() {
        let s = Fcc12::new(8, 8, 8, EdgeBehavior::Absorb).unwrap();
        // (2,2,2) -> (4,2,2): dx=2, dy=0, dz=0. max_abs=2, half_l1=1. d=2.
        assert_eq!(s.distance(&c(2, 2, 2), &c(4, 2, 2)), 2.0);
        // (2,2,2) -> (2,4,2): same pattern.
        assert_eq!(s.distance(&c(2, 2, 2), &c(2, 4, 2)), 2.0);
    }

    #[test]
    fn distance_balanced_diagonal() {
        // The L∞ counterexample: (0,0,0) → (2,2,2).
        // max_abs=2, half_l1=3. d=3, not 2.
        let s = Fcc12::new(8, 8, 8, EdgeBehavior::Absorb).unwrap();
        assert_eq!(s.distance(&c(0, 0, 0), &c(2, 2, 2)), 3.0);
    }

    #[test]
    fn distance_unbalanced() {
        // (0,0,0) → (4,0,0): max_abs=4, half_l1=2. d=4 (max_abs dominates).
        let s = Fcc12::new(8, 8, 8, EdgeBehavior::Absorb).unwrap();
        assert_eq!(s.distance(&c(0, 0, 0), &c(4, 0, 0)), 4.0);
    }

    #[test]
    fn distance_cross_grid() {
        // Corner to corner of 4x4x4.
        // (0,0,0) → (3,3,2): dx=3, dy=3, dz=2. max_abs=3, half_l1=4. d=4.
        let s = Fcc12::new(4, 4, 4, EdgeBehavior::Absorb).unwrap();
        assert_eq!(s.distance(&c(0, 0, 0), &c(3, 3, 2)), 4.0);
    }

    #[test]
    fn distance_wrap() {
        // 4x4x4 Wrap. (0,0,0) → (3,3,2).
        // Wrap distances: dx=min(3,1)=1, dy=min(3,1)=1, dz=min(2,2)=2.
        // max_abs=2, half_l1=2. d=2.
        let s = Fcc12::new(4, 4, 4, EdgeBehavior::Wrap).unwrap();
        assert_eq!(s.distance(&c(0, 0, 0), &c(3, 3, 2)), 2.0);
    }

    #[test]
    fn distance_wrap_tie() {
        // Wrap axis with diff == len/2: should return exactly len/2.
        let s = Fcc12::new(4, 4, 4, EdgeBehavior::Wrap).unwrap();
        // (0,0,0) → (2,0,0): dx=min(2,2)=2, dy=0, dz=0. max_abs=2, half_l1=1. d=2.
        assert_eq!(s.distance(&c(0, 0, 0), &c(2, 0, 0)), 2.0);
        // (0,0,0) → (0,2,0): dy=min(2,2)=2. Same pattern.
        assert_eq!(s.distance(&c(0, 0, 0), &c(0, 2, 0)), 2.0);
    }

    // ── Region tests ──────────────────────────────────────────────

    #[test]
    fn compile_region_all() {
        let s = Fcc12::new(4, 4, 4, EdgeBehavior::Absorb).unwrap();
        let plan = s.compile_region(&RegionSpec::All).unwrap();
        assert_eq!(plan.cell_count, 32);
        assert_eq!(plan.valid_ratio(), 1.0);
    }

    #[test]
    fn compile_region_disk_r1() {
        // Interior cell: center + 12 neighbours = 13.
        let s = Fcc12::new(8, 8, 8, EdgeBehavior::Absorb).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Disk {
                center: c(4, 4, 4),
                radius: 1,
            })
            .unwrap();
        assert_eq!(plan.cell_count, 13);
    }

    #[test]
    fn compile_region_disk_r2() {
        // Interior radius-2 disk count.
        let s = Fcc12::new(10, 10, 10, EdgeBehavior::Absorb).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Disk {
                center: c(4, 4, 4),
                radius: 2,
            })
            .unwrap();
        // R=2: 1 (center) + 12 (r=1) + some r=2 cells.
        // Each r=1 neighbour has 12 neighbours, but many overlap.
        // Verified via BFS: should be 55 cells for interior r=2 on FCC.
        assert!(
            plan.cell_count > 13,
            "r=2 disk should have more than 13 cells"
        );
    }

    #[test]
    fn compile_region_disk_boundary() {
        // Corner: truncated by Absorb.
        let s = Fcc12::new(4, 4, 4, EdgeBehavior::Absorb).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Disk {
                center: c(0, 0, 0),
                radius: 2,
            })
            .unwrap();
        assert!(plan.cell_count < 55, "boundary disk should be truncated");
        assert!(plan.cell_count >= 1);
    }

    #[test]
    fn compile_region_disk_huge_radius() {
        // Huge radius: should return all cells without overflow.
        let s = Fcc12::new(4, 4, 4, EdgeBehavior::Absorb).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Disk {
                center: c(2, 2, 2),
                radius: u32::MAX,
            })
            .unwrap();
        assert_eq!(plan.cell_count, s.cell_count());
    }

    #[test]
    fn compile_region_rect() {
        let s = Fcc12::new(6, 6, 6, EdgeBehavior::Absorb).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Rect {
                min: c(0, 0, 0),
                max: c(2, 2, 2),
            })
            .unwrap();
        // 3x3x3 sub-grid with parity filtering.
        // z=0: (0,0,0),(2,0,0),(1,1,0),(0,2,0),(2,2,0) = 5
        // z=1: (1,0,1),(0,1,1),(2,1,1),(1,2,1) = 4
        // z=2: (0,0,2),(2,0,2),(1,1,2),(0,2,2),(2,2,2) = 5
        // Total = 14
        assert_eq!(plan.cell_count, 14);
    }

    #[test]
    fn compile_region_rect_invalid() {
        let s = Fcc12::new(6, 6, 6, EdgeBehavior::Absorb).unwrap();
        assert!(s
            .compile_region(&RegionSpec::Rect {
                min: c(4, 0, 0),
                max: c(2, 2, 2),
            })
            .is_err());
    }

    #[test]
    fn compile_region_coords() {
        let s = Fcc12::new(6, 6, 6, EdgeBehavior::Absorb).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Coords(vec![
                c(2, 2, 2),
                c(0, 0, 0),
                c(1, 1, 0),
            ]))
            .unwrap();
        // Should be sorted by canonical rank.
        assert_eq!(plan.coords[0], c(0, 0, 0));
        assert_eq!(plan.coords[1], c(1, 1, 0));
        assert_eq!(plan.coords[2], c(2, 2, 2));
    }

    #[test]
    fn compile_region_coords_oob() {
        let s = Fcc12::new(4, 4, 4, EdgeBehavior::Absorb).unwrap();
        assert!(s
            .compile_region(&RegionSpec::Coords(vec![c(10, 0, 0)]))
            .is_err());
    }

    #[test]
    fn compile_region_coords_bad_parity() {
        let s = Fcc12::new(4, 4, 4, EdgeBehavior::Absorb).unwrap();
        // (1, 0, 0) has sum=1, odd parity — invalid.
        assert!(s
            .compile_region(&RegionSpec::Coords(vec![c(1, 0, 0)]))
            .is_err());
    }

    // ── Downcast ──────────────────────────────────────────────────

    #[test]
    fn downcast_ref() {
        let s: Box<dyn Space> = Box::new(Fcc12::new(4, 4, 4, EdgeBehavior::Absorb).unwrap());
        assert!(s.downcast_ref::<Fcc12>().is_some());
        assert!(s.downcast_ref::<crate::Square4>().is_none());
    }

    // ── Compliance suites ─────────────────────────────────────────

    #[test]
    fn compliance_4x4x4_absorb() {
        let s = Fcc12::new(4, 4, 4, EdgeBehavior::Absorb).unwrap();
        compliance::run_full_compliance(&s);
    }

    #[test]
    fn compliance_4x4x4_clamp() {
        let s = Fcc12::new(4, 4, 4, EdgeBehavior::Clamp).unwrap();
        compliance::run_full_compliance(&s);
    }

    #[test]
    fn compliance_4x4x4_wrap() {
        let s = Fcc12::new(4, 4, 4, EdgeBehavior::Wrap).unwrap();
        compliance::run_full_compliance(&s);
    }

    #[test]
    fn compliance_6x4x8_absorb() {
        let s = Fcc12::new(6, 4, 8, EdgeBehavior::Absorb).unwrap();
        compliance::run_full_compliance(&s);
    }

    #[test]
    fn compliance_2x2x2_absorb() {
        let s = Fcc12::new(2, 2, 2, EdgeBehavior::Absorb).unwrap();
        compliance::run_full_compliance(&s);
    }

    // ── Property tests ────────────────────────────────────────────

    /// Snap a coordinate to valid FCC parity within bounds.
    fn snap_fcc(x: i32, y: i32, z: i32, w: u32, h: u32, d: u32) -> (i32, i32, i32) {
        let x = x.rem_euclid(w as i32);
        let y = y.rem_euclid(h as i32);
        let mut z = z.rem_euclid(d as i32);
        // Fix parity: if (x+y+z) is odd, bump z by 1 (wrapping).
        if (x + y + z) % 2 != 0 {
            z = (z + 1) % d as i32;
        }
        (x, y, z)
    }

    proptest! {
        #[test]
        fn distance_is_metric(
            w in 2u32..6, h in 2u32..6, d in 2u32..6,
            ax in 0i32..6, ay in 0i32..6, az in 0i32..6,
            bx in 0i32..6, by in 0i32..6, bz in 0i32..6,
            cx in 0i32..6, cy in 0i32..6, cz in 0i32..6,
        ) {
            let (ax, ay, az) = snap_fcc(ax, ay, az, w, h, d);
            let (bx, by, bz) = snap_fcc(bx, by, bz, w, h, d);
            let (cx, cy, cz) = snap_fcc(cx, cy, cz, w, h, d);
            let s = Fcc12::new(w, h, d, EdgeBehavior::Absorb).unwrap();
            let a: Coord = smallvec![ax, ay, az];
            let b: Coord = smallvec![bx, by, bz];
            let cv: Coord = smallvec![cx, cy, cz];

            // Reflexive.
            prop_assert!((s.distance(&a, &a) - 0.0).abs() < f64::EPSILON);
            // Symmetric.
            prop_assert!((s.distance(&a, &b) - s.distance(&b, &a)).abs() < f64::EPSILON);
            // Triangle inequality.
            prop_assert!(s.distance(&a, &cv) <= s.distance(&a, &b) + s.distance(&b, &cv) + f64::EPSILON);
        }

        #[test]
        fn neighbours_symmetric(
            w in 2u32..6, h in 2u32..6, d in 2u32..6,
            x in 0i32..6, y in 0i32..6, z in 0i32..6,
        ) {
            let (x, y, z) = snap_fcc(x, y, z, w, h, d);
            let s = Fcc12::new(w, h, d, EdgeBehavior::Absorb).unwrap();
            let coord: Coord = smallvec![x, y, z];
            for nb in s.neighbours(&coord) {
                let nb_neighbours = s.neighbours(&nb);
                prop_assert!(
                    nb_neighbours.contains(&coord),
                    "neighbour symmetry violated: {:?} in N({:?}) but {:?} not in N({:?})",
                    nb, coord, coord, nb,
                );
            }
        }

        #[test]
        fn canonical_rank_matches_ordering(
            w in 2u32..6, h in 2u32..6, d in 2u32..6,
        ) {
            let s = Fcc12::new(w, h, d, EdgeBehavior::Absorb).unwrap();
            let ordering = s.canonical_ordering();
            for (i, coord) in ordering.iter().enumerate() {
                prop_assert_eq!(s.canonical_rank(coord), Some(i));
            }
        }
    }
}
