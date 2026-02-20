//! 1D line lattice with configurable edge behavior.

use crate::edge::EdgeBehavior;
use crate::error::SpaceError;
use crate::region::{BoundingShape, RegionPlan, RegionSpec};
use crate::space::Space;
use murk_core::{Coord, SpaceInstanceId};
use smallvec::{smallvec, SmallVec};
use std::collections::VecDeque;

/// A one-dimensional line lattice.
///
/// Each cell has coordinate `[i]` where `0 <= i < len`.
/// Boundary handling is controlled by [`EdgeBehavior`]:
/// - **Absorb**: edge cells have fewer neighbors
/// - **Clamp**: edge cells self-loop
/// - **Wrap**: periodic boundary (equivalent to [`Ring1D`](crate::Ring1D))
///
/// # Examples
///
/// ```
/// use murk_space::{Line1D, EdgeBehavior, Space};
///
/// let line = Line1D::new(5, EdgeBehavior::Absorb).unwrap();
/// assert_eq!(line.len(), 5);
/// assert_eq!(line.cell_count(), 5);
/// assert_eq!(line.ndim(), 1);
///
/// // Interior cell has two neighbours.
/// let coord: murk_core::Coord = vec![2i32].into();
/// assert_eq!(line.neighbours(&coord).len(), 2);
///
/// // Edge cell (absorb) has only one neighbour.
/// let edge: murk_core::Coord = vec![0i32].into();
/// assert_eq!(line.neighbours(&edge).len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct Line1D {
    len: u32,
    edge: EdgeBehavior,
    instance_id: SpaceInstanceId,
}

impl Line1D {
    /// Maximum length: coordinates use `i32`, so `len` must fit.
    pub const MAX_LEN: u32 = i32::MAX as u32;

    /// Create a new 1D line with `len` cells and the given edge behavior.
    ///
    /// Returns `Err(SpaceError::EmptySpace)` if `len == 0`, or
    /// `Err(SpaceError::DimensionTooLarge)` if `len > i32::MAX`.
    pub fn new(len: u32, edge: EdgeBehavior) -> Result<Self, SpaceError> {
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
            edge,
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

    /// Edge behavior.
    pub fn edge_behavior(&self) -> EdgeBehavior {
        self.edge
    }
}

// ── pub(crate) helpers shared with Ring1D ────────────────────────────

/// Wrap-aware neighbor computation for a 1D lattice of length `len`.
pub(crate) fn wrap_neighbours_1d(i: i32, len: u32) -> SmallVec<[Coord; 8]> {
    let n = len as i32;
    let left = ((i - 1) + n) % n;
    let right = (i + 1) % n;
    smallvec![smallvec![left], smallvec![right]]
}

/// Wrap-aware distance for a 1D lattice of length `len`.
pub(crate) fn wrap_distance_1d(a: i32, b: i32, len: u32) -> f64 {
    let diff = (a - b).unsigned_abs();
    let wrap = len - diff;
    diff.min(wrap) as f64
}

/// Canonical ordering for a 1D lattice: `[0], [1], ..., [len-1]`.
pub(crate) fn canonical_ordering_1d(len: u32) -> Vec<Coord> {
    (0..len as i32).map(|i| smallvec![i]).collect()
}

/// Check that a 1D coordinate is in bounds.
pub(crate) fn check_1d_bounds(coord: &Coord, len: u32) -> Result<i32, SpaceError> {
    if coord.len() != 1 {
        return Err(SpaceError::CoordOutOfBounds {
            coord: coord.clone(),
            bounds: format!("expected 1D coordinate, got {}D", coord.len()),
        });
    }
    let i = coord[0];
    if i < 0 || i >= len as i32 {
        return Err(SpaceError::CoordOutOfBounds {
            coord: coord.clone(),
            bounds: format!("[0, {})", len),
        });
    }
    Ok(i)
}

/// Compile a region for a 1D space. `wrap` controls whether disk/BFS wraps around.
pub(crate) fn compile_region_1d(
    spec: &RegionSpec,
    len: u32,
    wrap: bool,
) -> Result<RegionPlan, SpaceError> {
    match spec {
        RegionSpec::All => {
            let coords = canonical_ordering_1d(len);
            let cell_count = coords.len();
            let tensor_indices: Vec<usize> = (0..cell_count).collect();
            let valid_mask = vec![1u8; cell_count];
            Ok(RegionPlan {
                coords,
                tensor_indices,
                valid_mask,
                bounding_shape: BoundingShape::Rect(vec![cell_count]),
            })
        }

        RegionSpec::Disk { center, radius } => {
            let c = check_1d_bounds(center, len)?;
            compile_disk_1d(c, *radius, len, wrap)
        }

        RegionSpec::Neighbours { center, depth } => {
            let c = check_1d_bounds(center, len)?;
            compile_disk_1d(c, *depth, len, wrap)
        }

        RegionSpec::Rect { min, max } => {
            let lo = check_1d_bounds(min, len)?;
            let hi = check_1d_bounds(max, len)?;
            if lo > hi {
                return Err(SpaceError::InvalidRegion {
                    reason: format!("Rect min ({lo}) > max ({hi})"),
                });
            }
            let coords: Vec<Coord> = (lo..=hi).map(|i| smallvec![i]).collect();
            let cell_count = coords.len();
            let tensor_indices: Vec<usize> = (0..cell_count).collect();
            let valid_mask = vec![1u8; cell_count];
            Ok(RegionPlan {
                coords,
                tensor_indices,
                valid_mask,
                bounding_shape: BoundingShape::Rect(vec![cell_count]),
            })
        }

        RegionSpec::Coords(coords) => {
            for coord in coords {
                check_1d_bounds(coord, len)?;
            }
            // Deduplicate and sort in canonical order.
            let mut sorted: Vec<Coord> = coords.clone();
            sorted.sort();
            sorted.dedup();
            let cell_count = sorted.len();
            let tensor_indices: Vec<usize> = (0..cell_count).collect();
            let valid_mask = vec![1u8; cell_count];
            Ok(RegionPlan {
                coords: sorted,
                tensor_indices,
                valid_mask,
                bounding_shape: BoundingShape::Rect(vec![cell_count]),
            })
        }
    }
}

/// BFS-based disk compilation for 1D. Returns cells within `radius` graph-distance
/// of `center`, handling wrap if enabled.
fn compile_disk_1d(
    center: i32,
    radius: u32,
    len: u32,
    wrap: bool,
) -> Result<RegionPlan, SpaceError> {
    let n = len as i32;
    let mut visited = vec![false; len as usize];
    let mut queue = VecDeque::new();
    let mut result_indices: Vec<i32> = Vec::new();

    visited[center as usize] = true;
    queue.push_back((center, 0u32));
    result_indices.push(center);

    while let Some((pos, dist)) = queue.pop_front() {
        if dist >= radius {
            continue;
        }
        let candidates = if wrap {
            vec![((pos - 1 + n) % n), ((pos + 1) % n)]
        } else {
            let mut c = Vec::new();
            if pos > 0 {
                c.push(pos - 1);
            }
            if pos < n - 1 {
                c.push(pos + 1);
            }
            c
        };
        for nb in candidates {
            if !visited[nb as usize] {
                visited[nb as usize] = true;
                queue.push_back((nb, dist + 1));
                result_indices.push(nb);
            }
        }
    }

    // Sort in canonical order.
    result_indices.sort();
    let coords: Vec<Coord> = result_indices.iter().map(|&i| smallvec![i]).collect();
    let cell_count = coords.len();
    let tensor_indices: Vec<usize> = (0..cell_count).collect();
    let valid_mask = vec![1u8; cell_count];

    Ok(RegionPlan {
        coords,
        tensor_indices,
        valid_mask,
        bounding_shape: BoundingShape::Rect(vec![cell_count]),
    })
}

impl Space for Line1D {
    fn ndim(&self) -> usize {
        1
    }

    fn cell_count(&self) -> usize {
        self.len as usize
    }

    fn neighbours(&self, coord: &Coord) -> SmallVec<[Coord; 8]> {
        let i = coord[0];
        let n = self.len as i32;
        match self.edge {
            EdgeBehavior::Absorb => {
                let mut result = SmallVec::new();
                if i > 0 {
                    result.push(smallvec![i - 1]);
                }
                if i < n - 1 {
                    result.push(smallvec![i + 1]);
                }
                result
            }
            EdgeBehavior::Clamp => {
                let left = (i - 1).max(0);
                let right = (i + 1).min(n - 1);
                smallvec![smallvec![left], smallvec![right]]
            }
            EdgeBehavior::Wrap => wrap_neighbours_1d(i, self.len),
        }
    }

    fn distance(&self, a: &Coord, b: &Coord) -> f64 {
        let ai = a[0];
        let bi = b[0];
        match self.edge {
            EdgeBehavior::Wrap => wrap_distance_1d(ai, bi, self.len),
            EdgeBehavior::Absorb | EdgeBehavior::Clamp => (ai - bi).abs() as f64,
        }
    }

    fn compile_region(&self, spec: &RegionSpec) -> Result<RegionPlan, SpaceError> {
        let wrap = self.edge == EdgeBehavior::Wrap;
        compile_region_1d(spec, self.len, wrap)
    }

    fn canonical_ordering(&self) -> Vec<Coord> {
        canonical_ordering_1d(self.len)
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

    fn instance_id(&self) -> SpaceInstanceId {
        self.instance_id
    }

    fn topology_eq(&self, other: &dyn Space) -> bool {
        (other as &dyn std::any::Any)
            .downcast_ref::<Self>()
            .map_or(false, |o| self.len == o.len && self.edge == o.edge)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compliance;
    use murk_core::Coord;
    use proptest::prelude::*;

    /// Helper to build a Coord from a single i32 (avoids type-inference issues
    /// when `smallvec!` is used inside array literals).
    fn c(i: i32) -> Coord {
        smallvec![i]
    }

    // ── Neighbour tests ─────────────────────────────────────────

    #[test]
    fn neighbours_absorb_interior() {
        let s = Line1D::new(5, EdgeBehavior::Absorb).unwrap();
        let n = s.neighbours(&c(2));
        assert_eq!(n.as_slice(), &[c(1), c(3)]);
    }

    #[test]
    fn neighbours_absorb_left_edge() {
        let s = Line1D::new(5, EdgeBehavior::Absorb).unwrap();
        let n = s.neighbours(&c(0));
        assert_eq!(n.as_slice(), &[c(1)]);
    }

    #[test]
    fn neighbours_absorb_right_edge() {
        let s = Line1D::new(5, EdgeBehavior::Absorb).unwrap();
        let n = s.neighbours(&c(4));
        assert_eq!(n.as_slice(), &[c(3)]);
    }

    #[test]
    fn neighbours_clamp_interior() {
        let s = Line1D::new(5, EdgeBehavior::Clamp).unwrap();
        let n = s.neighbours(&c(2));
        assert_eq!(n.as_slice(), &[c(1), c(3)]);
    }

    #[test]
    fn neighbours_clamp_left_edge() {
        let s = Line1D::new(5, EdgeBehavior::Clamp).unwrap();
        let n = s.neighbours(&c(0));
        assert_eq!(n.as_slice(), &[c(0), c(1)]);
    }

    #[test]
    fn neighbours_clamp_right_edge() {
        let s = Line1D::new(5, EdgeBehavior::Clamp).unwrap();
        let n = s.neighbours(&c(4));
        assert_eq!(n.as_slice(), &[c(3), c(4)]);
    }

    #[test]
    fn neighbours_wrap_interior() {
        let s = Line1D::new(5, EdgeBehavior::Wrap).unwrap();
        let n = s.neighbours(&c(2));
        assert_eq!(n.as_slice(), &[c(1), c(3)]);
    }

    #[test]
    fn neighbours_wrap_left_edge() {
        let s = Line1D::new(5, EdgeBehavior::Wrap).unwrap();
        let n = s.neighbours(&c(0));
        assert_eq!(n.as_slice(), &[c(4), c(1)]);
    }

    #[test]
    fn neighbours_wrap_right_edge() {
        let s = Line1D::new(5, EdgeBehavior::Wrap).unwrap();
        let n = s.neighbours(&c(4));
        assert_eq!(n.as_slice(), &[c(3), c(0)]);
    }

    // ── Single-cell edge cases ──────────────────────────────────

    #[test]
    fn neighbours_absorb_single_cell() {
        let s = Line1D::new(1, EdgeBehavior::Absorb).unwrap();
        let n = s.neighbours(&c(0));
        assert!(n.is_empty());
    }

    #[test]
    fn neighbours_clamp_single_cell() {
        let s = Line1D::new(1, EdgeBehavior::Clamp).unwrap();
        let n = s.neighbours(&c(0));
        assert_eq!(n.as_slice(), &[c(0), c(0)]);
    }

    #[test]
    fn neighbours_wrap_single_cell() {
        let s = Line1D::new(1, EdgeBehavior::Wrap).unwrap();
        let n = s.neighbours(&c(0));
        assert_eq!(n.as_slice(), &[c(0), c(0)]);
    }

    // ── Distance tests ──────────────────────────────────────────

    #[test]
    fn distance_absorb_worked() {
        let s = Line1D::new(10, EdgeBehavior::Absorb).unwrap();
        assert_eq!(s.distance(&c(2), &c(7)), 5.0);
        assert_eq!(s.distance(&c(0), &c(9)), 9.0);
    }

    #[test]
    fn distance_wrap_worked() {
        let s = Line1D::new(10, EdgeBehavior::Wrap).unwrap();
        // Direct: |2-7| = 5, Wrap: 10-5 = 5 → min = 5
        assert_eq!(s.distance(&c(2), &c(7)), 5.0);
        // Direct: |0-9| = 9, Wrap: 10-9 = 1 → min = 1
        assert_eq!(s.distance(&c(0), &c(9)), 1.0);
    }

    // ── Region compilation tests ────────────────────────────────

    #[test]
    fn compile_region_all() {
        let s = Line1D::new(5, EdgeBehavior::Absorb).unwrap();
        let plan = s.compile_region(&RegionSpec::All).unwrap();
        assert_eq!(plan.cell_count(), 5);
        assert_eq!(plan.coords.len(), 5);
        assert_eq!(plan.valid_ratio(), 1.0);
    }

    #[test]
    fn compile_region_disk_interior() {
        let s = Line1D::new(10, EdgeBehavior::Absorb).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Disk {
                center: c(5),
                radius: 2,
            })
            .unwrap();
        assert_eq!(plan.cell_count(), 5); // cells 3,4,5,6,7
        assert_eq!(plan.coords, vec![c(3), c(4), c(5), c(6), c(7)]);
    }

    #[test]
    fn compile_region_disk_wrap_edge() {
        let s = Line1D::new(10, EdgeBehavior::Wrap).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Disk {
                center: c(0),
                radius: 2,
            })
            .unwrap();
        // Should include 8, 9, 0, 1, 2 (wrapping around)
        assert_eq!(plan.cell_count(), 5);
        assert!(plan.coords.contains(&c(8)));
        assert!(plan.coords.contains(&c(9)));
        assert!(plan.coords.contains(&c(0)));
        assert!(plan.coords.contains(&c(1)));
        assert!(plan.coords.contains(&c(2)));
    }

    #[test]
    fn compile_region_rect() {
        let s = Line1D::new(10, EdgeBehavior::Absorb).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Rect {
                min: c(2),
                max: c(5),
            })
            .unwrap();
        assert_eq!(plan.cell_count(), 4);
        assert_eq!(plan.coords, vec![c(2), c(3), c(4), c(5)]);
    }

    #[test]
    fn compile_region_rect_invalid() {
        let s = Line1D::new(10, EdgeBehavior::Absorb).unwrap();
        let result = s.compile_region(&RegionSpec::Rect {
            min: c(5),
            max: c(2),
        });
        assert!(result.is_err());
    }

    #[test]
    fn compile_region_coords_valid() {
        let s = Line1D::new(10, EdgeBehavior::Absorb).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Coords(vec![c(3), c(7), c(1)]))
            .unwrap();
        // Sorted and deduplicated
        assert_eq!(plan.coords, vec![c(1), c(3), c(7)]);
    }

    #[test]
    fn compile_region_coords_oob() {
        let s = Line1D::new(5, EdgeBehavior::Absorb).unwrap();
        let result = s.compile_region(&RegionSpec::Coords(vec![c(10)]));
        assert!(result.is_err());
    }

    // ── Constructor tests ───────────────────────────────────────

    #[test]
    fn new_zero_len_returns_error() {
        let result = Line1D::new(0, EdgeBehavior::Absorb);
        assert!(matches!(result, Err(SpaceError::EmptySpace)));
    }

    #[test]
    fn new_rejects_len_exceeding_i32_max() {
        let result = Line1D::new(i32::MAX as u32 + 1, EdgeBehavior::Absorb);
        assert!(matches!(result, Err(SpaceError::DimensionTooLarge { .. })));
        // i32::MAX itself should be accepted.
        assert!(Line1D::new(i32::MAX as u32, EdgeBehavior::Absorb).is_ok());
    }

    // ── Compliance suites ───────────────────────────────────────

    #[test]
    fn compliance_absorb() {
        let s = Line1D::new(20, EdgeBehavior::Absorb).unwrap();
        compliance::run_full_compliance(&s);
    }

    #[test]
    fn compliance_clamp() {
        let s = Line1D::new(20, EdgeBehavior::Clamp).unwrap();
        compliance::run_full_compliance(&s);
    }

    #[test]
    fn compliance_wrap() {
        let s = Line1D::new(20, EdgeBehavior::Wrap).unwrap();
        compliance::run_full_compliance(&s);
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
            len in 2u32..50,
            edge in arb_edge(),
            a in 0i32..50,
            b in 0i32..50,
            c in 0i32..50,
        ) {
            let a = a % len as i32;
            let b = b % len as i32;
            let c = c % len as i32;
            let s = Line1D::new(len, edge).unwrap();
            let ca: Coord = smallvec![a];
            let cb: Coord = smallvec![b];
            let cc: Coord = smallvec![c];

            // Reflexive
            prop_assert!((s.distance(&ca, &ca) - 0.0).abs() < f64::EPSILON);
            // Symmetric
            prop_assert!((s.distance(&ca, &cb) - s.distance(&cb, &ca)).abs() < f64::EPSILON);
            // Triangle inequality
            prop_assert!(s.distance(&ca, &cc) <= s.distance(&ca, &cb) + s.distance(&cb, &cc) + f64::EPSILON);
        }

        #[test]
        fn neighbours_symmetric(
            len in 2u32..50,
            edge in arb_edge(),
            i in 0i32..50,
        ) {
            let i = i % len as i32;
            let s = Line1D::new(len, edge).unwrap();
            let coord: Coord = smallvec![i];
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
