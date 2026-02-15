//! Cartesian product of arbitrary spaces.

use crate::error::SpaceError;
use crate::region::{BoundingShape, RegionPlan, RegionSpec};
use crate::space::Space;
use indexmap::IndexSet;
use murk_core::{Coord, SpaceInstanceId};
use smallvec::SmallVec;
use std::collections::VecDeque;
use std::fmt;

/// Distance metric for product spaces.
#[derive(Clone, Debug)]
pub enum ProductMetric {
    /// Sum of per-component distances (default graph geodesic).
    L1,
    /// Maximum of per-component distances.
    LInfinity,
    /// Weighted sum of per-component distances.
    Weighted(Vec<f64>),
}

/// Cartesian product of arbitrary spaces.
///
/// Composes N spaces into a single space where each cell is an N-tuple
/// of per-component coordinates. Coordinates are concatenated:
/// `[q0, r0, ..., q1, r1, ...]`.
///
/// - **Neighbours** (R-SPACE-8): vary one component at a time, others held constant
/// - **Distance** (R-SPACE-9): L1 sum of per-component distances (graph geodesic)
/// - **Canonical ordering** (R-SPACE-10): lexicographic, leftmost component slowest
/// - **Regions**: Cartesian product of per-component region plans
pub struct ProductSpace {
    components: Vec<Box<dyn Space>>,
    dim_offsets: Vec<usize>,
    total_ndim: usize,
    total_cells: usize,
    instance_id: SpaceInstanceId,
}

impl fmt::Debug for ProductSpace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProductSpace")
            .field("n_components", &self.components.len())
            .field("total_ndim", &self.total_ndim)
            .field("total_cells", &self.total_cells)
            .field("dim_offsets", &self.dim_offsets)
            .finish()
    }
}

impl ProductSpace {
    /// Create a new product space from a list of component spaces.
    ///
    /// Returns `Err(SpaceError::InvalidComposition)` if:
    /// - `components` is empty
    /// - The total cell count overflows `usize`
    pub fn new(components: Vec<Box<dyn Space>>) -> Result<Self, SpaceError> {
        if components.is_empty() {
            return Err(SpaceError::InvalidComposition {
                reason: "ProductSpace requires at least one component".to_string(),
            });
        }

        // Build dim_offsets: [0, ndim_0, ndim_0+ndim_1, ...]
        let mut dim_offsets = Vec::with_capacity(components.len() + 1);
        dim_offsets.push(0);
        let mut total_ndim = 0usize;
        for comp in &components {
            total_ndim += comp.ndim();
            dim_offsets.push(total_ndim);
        }

        // Overflow-checked cell count product.
        let mut total_cells: usize = 1;
        for comp in &components {
            total_cells = total_cells.checked_mul(comp.cell_count()).ok_or_else(|| {
                SpaceError::InvalidComposition {
                    reason: "total cell count overflows usize".to_string(),
                }
            })?;
        }

        Ok(Self {
            components,
            dim_offsets,
            total_ndim,
            total_cells,
            instance_id: SpaceInstanceId::next(),
        })
    }

    /// Number of component spaces.
    pub fn n_components(&self) -> usize {
        self.components.len()
    }

    /// Access the i-th component space.
    pub fn component(&self, i: usize) -> &dyn Space {
        &*self.components[i]
    }

    /// Extract the sub-coordinate for component `i` from a product coordinate.
    fn split_coord(&self, coord: &Coord, i: usize) -> Coord {
        let start = self.dim_offsets[i];
        let end = self.dim_offsets[i + 1];
        SmallVec::from_slice(&coord[start..end])
    }

    /// Join per-component coordinates into a single product coordinate.
    fn join_coords(&self, parts: &[Coord]) -> Coord {
        let mut out = SmallVec::with_capacity(self.total_ndim);
        for part in parts {
            out.extend_from_slice(part);
        }
        out
    }

    /// Sort coordinates by product canonical order (leftmost component slowest).
    ///
    /// Uses `canonical_rank` for O(1) per-coordinate ranking — no
    /// full-ordering materialization, so cost scales with the region
    /// size, not the total space size.
    fn sort_canonical(&self, coords: &mut [Coord]) {
        coords.sort_by_key(|c| self.canonical_rank(c).unwrap_or(usize::MAX));
    }

    /// Compute distance using an alternate metric (not the default L1).
    pub fn metric_distance(&self, a: &Coord, b: &Coord, metric: &ProductMetric) -> f64 {
        let per_comp: Vec<f64> = (0..self.components.len())
            .map(|i| {
                let ca = self.split_coord(a, i);
                let cb = self.split_coord(b, i);
                self.components[i].distance(&ca, &cb)
            })
            .collect();

        match metric {
            ProductMetric::L1 => per_comp.iter().sum(),
            ProductMetric::LInfinity => per_comp.iter().copied().fold(0.0f64, f64::max),
            ProductMetric::Weighted(weights) => {
                per_comp.iter().zip(weights).map(|(d, w)| d * w).sum()
            }
        }
    }

    /// Compile a Cartesian product of per-component region plans.
    fn compile_cartesian_product(&self, per_comp: &[RegionPlan]) -> RegionPlan {
        // Bounding shape = concatenation of per-component bounding shapes.
        let mut bounding_dims = Vec::new();
        for plan in per_comp {
            match &plan.bounding_shape {
                BoundingShape::Rect(dims) => bounding_dims.extend(dims),
            }
        }
        let bounding_total: usize = bounding_dims.iter().product();

        // Compute per-component bounding sizes and strides.
        let comp_bounding_sizes: Vec<usize> = per_comp
            .iter()
            .map(|p| p.bounding_shape.total_elements())
            .collect();
        let n = per_comp.len();
        let mut strides = vec![1usize; n];
        for i in (0..n - 1).rev() {
            strides[i] = strides[i + 1] * comp_bounding_sizes[i + 1];
        }

        // Iterate all combinations using odometer.
        let mut valid_mask = vec![0u8; bounding_total];
        let mut coords = Vec::new();
        let mut tensor_indices = Vec::new();

        // Build array of per-component (coord, tensor_idx) pairs.
        let per_comp_entries: Vec<Vec<(Coord, usize)>> = per_comp
            .iter()
            .map(|plan| {
                plan.coords
                    .iter()
                    .zip(&plan.tensor_indices)
                    .map(|(c, &ti)| (c.clone(), ti))
                    .collect()
            })
            .collect();

        // Odometer iteration.
        let mut indices = vec![0usize; n];
        loop {
            // Check if all components are at valid entries.
            let mut product_tensor_idx = 0;
            let mut product_coord = SmallVec::with_capacity(self.total_ndim);
            for (i, &idx) in indices.iter().enumerate() {
                let (ref c, ti) = per_comp_entries[i][idx];
                product_tensor_idx += ti * strides[i];
                product_coord.extend_from_slice(c);
            }

            valid_mask[product_tensor_idx] = 1;
            coords.push(product_coord);
            tensor_indices.push(product_tensor_idx);

            // Advance odometer (rightmost = fastest).
            let mut carry = true;
            for i in (0..n).rev() {
                if carry {
                    indices[i] += 1;
                    if indices[i] < per_comp_entries[i].len() {
                        carry = false;
                    } else {
                        indices[i] = 0;
                    }
                }
            }
            if carry {
                break;
            }
        }

        let cell_count = coords.len();
        RegionPlan {
            cell_count,
            coords,
            tensor_indices,
            valid_mask,
            bounding_shape: BoundingShape::Rect(bounding_dims),
        }
    }
}

impl Space for ProductSpace {
    fn ndim(&self) -> usize {
        self.total_ndim
    }

    fn cell_count(&self) -> usize {
        self.total_cells
    }

    fn neighbours(&self, coord: &Coord) -> SmallVec<[Coord; 8]> {
        // R-SPACE-8: for each component, generate its neighbours
        // while holding other components constant.
        let parts: Vec<Coord> = (0..self.components.len())
            .map(|i| self.split_coord(coord, i))
            .collect();

        let mut result = SmallVec::new();
        for i in 0..self.components.len() {
            let comp_neighbours = self.components[i].neighbours(&parts[i]);
            for nb in comp_neighbours {
                let mut new_parts = parts.clone();
                new_parts[i] = nb;
                result.push(self.join_coords(&new_parts));
            }
        }
        result
    }

    fn distance(&self, a: &Coord, b: &Coord) -> f64 {
        // R-SPACE-9: L1 sum of per-component distances.
        (0..self.components.len())
            .map(|i| {
                let ca = self.split_coord(a, i);
                let cb = self.split_coord(b, i);
                self.components[i].distance(&ca, &cb)
            })
            .sum()
    }

    fn compile_region(&self, spec: &RegionSpec) -> Result<RegionPlan, SpaceError> {
        match spec {
            RegionSpec::All => {
                // Cartesian product of per-component All plans.
                let per_comp: Vec<RegionPlan> = self
                    .components
                    .iter()
                    .map(|c| c.compile_region(&RegionSpec::All))
                    .collect::<Result<_, _>>()?;
                Ok(self.compile_cartesian_product(&per_comp))
            }

            RegionSpec::Rect { min, max } => {
                // R-SPACE-11: split min/max per-component, compile per-component Rect.
                if min.len() != self.total_ndim || max.len() != self.total_ndim {
                    return Err(SpaceError::InvalidRegion {
                        reason: format!(
                            "Rect coordinates must have {} dimensions, got {}/{}",
                            self.total_ndim,
                            min.len(),
                            max.len()
                        ),
                    });
                }
                let per_comp: Vec<RegionPlan> = (0..self.components.len())
                    .map(|i| {
                        let start = self.dim_offsets[i];
                        let end = self.dim_offsets[i + 1];
                        let comp_min: Coord = SmallVec::from_slice(&min[start..end]);
                        let comp_max: Coord = SmallVec::from_slice(&max[start..end]);
                        self.components[i].compile_region(&RegionSpec::Rect {
                            min: comp_min,
                            max: comp_max,
                        })
                    })
                    .collect::<Result<_, _>>()?;
                Ok(self.compile_cartesian_product(&per_comp))
            }

            RegionSpec::Disk { center, radius } => {
                // BFS in product graph.
                self.compile_disk_bfs(center, *radius)
            }

            RegionSpec::Neighbours { center, depth } => self.compile_disk_bfs(center, *depth),

            RegionSpec::Coords(coords) => {
                // Validate all coords.
                for coord in coords {
                    if coord.len() != self.total_ndim {
                        return Err(SpaceError::CoordOutOfBounds {
                            coord: coord.clone(),
                            bounds: format!("expected {}D coordinate", self.total_ndim),
                        });
                    }
                    // Validate each component.
                    for i in 0..self.components.len() {
                        let sub = self.split_coord(coord, i);
                        // Check bounds by trying to compute distance to self.
                        // If the coord is invalid in any component, the canonical
                        // ordering won't contain it.
                        let ordering = self.components[i].canonical_ordering();
                        if !ordering.contains(&sub) {
                            return Err(SpaceError::CoordOutOfBounds {
                                coord: coord.clone(),
                                bounds: format!("component {i} coordinate out of bounds"),
                            });
                        }
                    }
                }
                let mut sorted: Vec<Coord> = coords.clone();
                self.sort_canonical(&mut sorted);
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
        // R-SPACE-10: lexicographic, leftmost slowest.
        // Odometer over per-component orderings.
        let orderings: Vec<Vec<Coord>> = self
            .components
            .iter()
            .map(|c| c.canonical_ordering())
            .collect();

        let n = self.components.len();
        let mut result = Vec::with_capacity(self.total_cells);
        let mut indices = vec![0usize; n];

        loop {
            let mut coord = SmallVec::with_capacity(self.total_ndim);
            for (i, &idx) in indices.iter().enumerate() {
                coord.extend_from_slice(&orderings[i][idx]);
            }
            result.push(coord);

            // Advance odometer (rightmost = fastest).
            let mut carry = true;
            for i in (0..n).rev() {
                if carry {
                    indices[i] += 1;
                    if indices[i] < orderings[i].len() {
                        carry = false;
                    } else {
                        indices[i] = 0;
                    }
                }
            }
            if carry {
                break;
            }
        }
        result
    }

    fn canonical_rank(&self, coord: &Coord) -> Option<usize> {
        if coord.len() != self.total_ndim {
            return None;
        }
        // rank = Σ comp_rank[i] * stride[i]
        // where stride[i] = product(cell_count[j] for j > i)
        let n = self.components.len();
        let mut rank = 0usize;
        let mut stride = 1usize;
        for i in (0..n).rev() {
            let sub = self.split_coord(coord, i);
            let comp_rank = self.components[i].canonical_rank(&sub)?;
            rank += comp_rank * stride;
            stride *= self.components[i].cell_count();
        }
        Some(rank)
    }

    fn instance_id(&self) -> SpaceInstanceId {
        self.instance_id
    }
}

impl ProductSpace {
    /// BFS-based disk compilation in the product graph.
    fn compile_disk_bfs(&self, center: &Coord, radius: u32) -> Result<RegionPlan, SpaceError> {
        if center.len() != self.total_ndim {
            return Err(SpaceError::CoordOutOfBounds {
                coord: center.clone(),
                bounds: format!("expected {}D coordinate", self.total_ndim),
            });
        }
        // Validate center is in-bounds for each component.
        for i in 0..self.components.len() {
            let sub = self.split_coord(center, i);
            let ordering = self.components[i].canonical_ordering();
            if !ordering.contains(&sub) {
                return Err(SpaceError::CoordOutOfBounds {
                    coord: center.clone(),
                    bounds: format!("component {i} coordinate {:?} out of bounds", sub),
                });
            }
        }

        let mut visited: IndexSet<Coord> = IndexSet::new();
        let mut queue: VecDeque<(Coord, u32)> = VecDeque::new();
        let mut result: Vec<Coord> = Vec::new();

        visited.insert(center.clone());
        queue.push_back((center.clone(), 0));
        result.push(center.clone());

        while let Some((coord, dist)) = queue.pop_front() {
            if dist >= radius {
                continue;
            }
            for nb in self.neighbours(&coord) {
                if visited.insert(nb.clone()) {
                    queue.push_back((nb.clone(), dist + 1));
                    result.push(nb);
                }
            }
        }

        self.sort_canonical(&mut result);
        let cell_count = result.len();
        let tensor_indices: Vec<usize> = (0..cell_count).collect();
        let valid_mask = vec![1u8; cell_count];

        Ok(RegionPlan {
            cell_count,
            coords: result,
            tensor_indices,
            valid_mask,
            bounding_shape: BoundingShape::Rect(vec![cell_count]),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compliance;
    use crate::{Hex2D, Line1D, Ring1D};
    use murk_core::Coord;
    use proptest::prelude::*;
    use smallvec::smallvec;

    // Helper: create Hex2D(5,5) x Line1D(10,Absorb) product space.
    fn hex_line() -> ProductSpace {
        let hex = Hex2D::new(5, 5).unwrap();
        let line = Line1D::new(10, crate::EdgeBehavior::Absorb).unwrap();
        ProductSpace::new(vec![Box::new(hex), Box::new(line)]).unwrap()
    }

    // ── HLD Worked Examples ─────────────────────────────────────

    #[test]
    fn neighbours_hex_line() {
        // coord ((2,1), 5) → 6 hex + 2 line = 8 neighbours
        let s = hex_line();
        let coord: Coord = smallvec![2, 1, 5];
        let n = s.neighbours(&coord);
        assert_eq!(n.len(), 8);

        // 6 hex neighbours (vary hex, hold line=5)
        assert!(n.contains(&smallvec![3, 1, 5])); // E
        assert!(n.contains(&smallvec![3, 0, 5])); // NE
        assert!(n.contains(&smallvec![2, 0, 5])); // NW
        assert!(n.contains(&smallvec![1, 1, 5])); // W
        assert!(n.contains(&smallvec![1, 2, 5])); // SW
        assert!(n.contains(&smallvec![2, 2, 5])); // SE

        // 2 line neighbours (vary line, hold hex=(2,1))
        assert!(n.contains(&smallvec![2, 1, 4]));
        assert!(n.contains(&smallvec![2, 1, 6]));
    }

    #[test]
    fn distance_hex_line() {
        // ((2,1), 5) → ((4,0), 8): hex_dist=2, line_dist=3, total=5
        let s = hex_line();
        let a: Coord = smallvec![2, 1, 5];
        let b: Coord = smallvec![4, 0, 8];
        assert_eq!(s.distance(&a, &b), 5.0);
    }

    #[test]
    fn metric_distance_linf() {
        // Same pair, LInfinity: max(2, 3) = 3
        let s = hex_line();
        let a: Coord = smallvec![2, 1, 5];
        let b: Coord = smallvec![4, 0, 8];
        assert_eq!(s.metric_distance(&a, &b, &ProductMetric::LInfinity), 3.0);
    }

    #[test]
    fn metric_distance_weighted() {
        let s = hex_line();
        let a: Coord = smallvec![2, 1, 5];
        let b: Coord = smallvec![4, 0, 8];
        // hex_dist=2, line_dist=3, weights=[1.0, 2.0] → 2*1.0 + 3*2.0 = 8.0
        assert_eq!(
            s.metric_distance(&a, &b, &ProductMetric::Weighted(vec![1.0, 2.0])),
            8.0
        );
    }

    #[test]
    fn iteration_order_hex_line() {
        // R-SPACE-10: leftmost (hex) slowest, rightmost (line) fastest.
        // First few entries should be: hex(0,0)+line(0), hex(0,0)+line(1), ...
        let s = hex_line();
        let order = s.canonical_ordering();
        assert_eq!(order.len(), 250); // 25 * 10

        // First 10 entries: hex(0,0) with line 0..9
        for (i, coord) in order.iter().enumerate().take(10) {
            let expected: Coord = smallvec![0, 0, i as i32];
            assert_eq!(*coord, expected);
        }
        // Next 10: hex(1,0) with line 0..9
        for (j, coord) in order[10..20].iter().enumerate() {
            let expected: Coord = smallvec![1, 0, j as i32];
            assert_eq!(*coord, expected);
        }
    }

    #[test]
    fn region_rect_hex_line() {
        // Per-component Rect → Cartesian product.
        let s = hex_line();
        let plan = s
            .compile_region(&RegionSpec::Rect {
                min: smallvec![1, 1, 3],
                max: smallvec![2, 2, 5],
            })
            .unwrap();
        // hex rect: 2 cols * 2 rows = 4, line rect: 3 cells → 4 * 3 = 12
        assert_eq!(plan.cell_count, 12);
    }

    // ── Structural tests ────────────────────────────────────────

    #[test]
    fn ndim_sum() {
        let s = hex_line();
        assert_eq!(s.ndim(), 3); // Hex2D(2) + Line1D(1)
    }

    #[test]
    fn cell_count_product() {
        let s = hex_line();
        assert_eq!(s.cell_count(), 250); // 25 * 10
    }

    #[test]
    fn three_component() {
        let hex = Hex2D::new(3, 3).unwrap();
        let line = Line1D::new(5, crate::EdgeBehavior::Absorb).unwrap();
        let ring = Ring1D::new(4).unwrap();
        let s = ProductSpace::new(vec![Box::new(hex), Box::new(line), Box::new(ring)]).unwrap();
        assert_eq!(s.ndim(), 4); // 2 + 1 + 1
        assert_eq!(s.cell_count(), 180); // 9 * 5 * 4
    }

    #[test]
    fn single_component() {
        let line = Line1D::new(5, crate::EdgeBehavior::Absorb).unwrap();
        let s = ProductSpace::new(vec![Box::new(line)]).unwrap();
        assert_eq!(s.ndim(), 1);
        assert_eq!(s.cell_count(), 5);
        // Neighbours should match the underlying Line1D.
        let n = s.neighbours(&smallvec![2]);
        assert_eq!(n.len(), 2);
        assert!(n.contains(&smallvec![1]));
        assert!(n.contains(&smallvec![3]));
    }

    #[test]
    fn empty_components_error() {
        let result = ProductSpace::new(vec![]);
        assert!(matches!(result, Err(SpaceError::InvalidComposition { .. })));
    }

    // ── valid_ratio tests ───────────────────────────────────────

    #[test]
    fn valid_ratio_hex_disk_x_line_all() {
        let hex = Hex2D::new(10, 10).unwrap();
        let line = Line1D::new(5, crate::EdgeBehavior::Absorb).unwrap();
        let s = ProductSpace::new(vec![Box::new(hex), Box::new(line)]).unwrap();

        // All region: valid_ratio = 1.0
        let plan = s.compile_region(&RegionSpec::All).unwrap();
        assert_eq!(plan.valid_ratio(), 1.0);
    }

    // ── Compliance ──────────────────────────────────────────────

    #[test]
    fn compliance_hex_line_small() {
        let hex = Hex2D::new(3, 3).unwrap();
        let line = Line1D::new(3, crate::EdgeBehavior::Absorb).unwrap();
        let s = ProductSpace::new(vec![Box::new(hex), Box::new(line)]).unwrap();
        compliance::run_full_compliance(&s);
    }

    #[test]
    fn compliance_line_ring() {
        let line = Line1D::new(4, crate::EdgeBehavior::Absorb).unwrap();
        let ring = Ring1D::new(3).unwrap();
        let s = ProductSpace::new(vec![Box::new(line), Box::new(ring)]).unwrap();
        compliance::run_full_compliance(&s);
    }

    // ── Downcast test ───────────────────────────────────────────

    #[test]
    fn downcast_ref_product_space() {
        let line = Line1D::new(5, crate::EdgeBehavior::Absorb).unwrap();
        let s: Box<dyn Space> = Box::new(ProductSpace::new(vec![Box::new(line)]).unwrap());
        assert!(s.downcast_ref::<ProductSpace>().is_some());
        assert!(s.downcast_ref::<Hex2D>().is_none());
    }

    // ── Regression tests ─────────────────────────────────────────

    #[test]
    fn disk_coords_match_canonical_order() {
        // Verify BFS disk result is sorted by product canonical order,
        // not raw lexicographic order (which diverges for Hex2D r-then-q).
        let hex = Hex2D::new(5, 5).unwrap();
        let line = Line1D::new(5, crate::EdgeBehavior::Absorb).unwrap();
        let s = ProductSpace::new(vec![Box::new(hex), Box::new(line)]).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Disk {
                center: smallvec![2, 2, 2],
                radius: 1,
            })
            .unwrap();
        // Check that disk coords are a subsequence of canonical ordering.
        let canonical = s.canonical_ordering();
        let mut last_pos = None;
        for coord in &plan.coords {
            let pos = canonical
                .iter()
                .position(|c| c == coord)
                .expect("disk coord not in canonical ordering");
            if let Some(lp) = last_pos {
                assert!(pos > lp, "coords not in canonical order: {:?}", plan.coords);
            }
            last_pos = Some(pos);
        }
    }

    #[test]
    fn coords_region_matches_canonical_order() {
        // Verify Coords region is sorted by product canonical order.
        let hex = Hex2D::new(5, 5).unwrap();
        let line = Line1D::new(5, crate::EdgeBehavior::Absorb).unwrap();
        let s = ProductSpace::new(vec![Box::new(hex), Box::new(line)]).unwrap();
        let plan = s
            .compile_region(&RegionSpec::Coords(vec![
                smallvec![1, 0, 3],
                smallvec![0, 1, 2], // Hex canonical: (0,1) > (1,0) since r-then-q
                smallvec![2, 0, 0],
            ]))
            .unwrap();
        let canonical = s.canonical_ordering();
        let mut last_pos = None;
        for coord in &plan.coords {
            let pos = canonical.iter().position(|c| c == coord).unwrap();
            if let Some(lp) = last_pos {
                assert!(pos > lp, "coords not in canonical order: {:?}", plan.coords);
            }
            last_pos = Some(pos);
        }
    }

    #[test]
    fn disk_oob_center_rejected() {
        let hex = Hex2D::new(5, 5).unwrap();
        let line = Line1D::new(5, crate::EdgeBehavior::Absorb).unwrap();
        let s = ProductSpace::new(vec![Box::new(hex), Box::new(line)]).unwrap();
        // Center q=999 is out of bounds for Hex2D(5,5).
        let result = s.compile_region(&RegionSpec::Disk {
            center: smallvec![999, 0, 2],
            radius: 1,
        });
        assert!(result.is_err());
    }

    // ── Property tests ──────────────────────────────────────────

    proptest! {
        #[test]
        fn distance_is_metric(
            len_a in 2u32..5,
            len_b in 2u32..5,
            ai in 0i32..5, bi in 0i32..5,
            aj in 0i32..5, bj in 0i32..5,
            ci in 0i32..5, cj in 0i32..5,
        ) {
            let ai = ai % len_a as i32;
            let bi = bi % len_b as i32;
            let aj = aj % len_a as i32;
            let bj = bj % len_b as i32;
            let ci = ci % len_a as i32;
            let cj = cj % len_b as i32;

            let line_a = Line1D::new(len_a, crate::EdgeBehavior::Absorb).unwrap();
            let line_b = Line1D::new(len_b, crate::EdgeBehavior::Absorb).unwrap();
            let s = ProductSpace::new(vec![Box::new(line_a), Box::new(line_b)]).unwrap();

            let a: Coord = smallvec![ai, bi];
            let b: Coord = smallvec![aj, bj];
            let cv: Coord = smallvec![ci, cj];

            prop_assert!((s.distance(&a, &a) - 0.0).abs() < f64::EPSILON);
            prop_assert!((s.distance(&a, &b) - s.distance(&b, &a)).abs() < f64::EPSILON);
            prop_assert!(s.distance(&a, &cv) <= s.distance(&a, &b) + s.distance(&b, &cv) + f64::EPSILON);
        }

        #[test]
        fn neighbours_symmetric(
            len_a in 2u32..5,
            len_b in 2u32..5,
            i in 0i32..5, j in 0i32..5,
        ) {
            let i = i % len_a as i32;
            let j = j % len_b as i32;

            let line_a = Line1D::new(len_a, crate::EdgeBehavior::Absorb).unwrap();
            let line_b = Line1D::new(len_b, crate::EdgeBehavior::Absorb).unwrap();
            let s = ProductSpace::new(vec![Box::new(line_a), Box::new(line_b)]).unwrap();

            let coord: Coord = smallvec![i, j];
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
