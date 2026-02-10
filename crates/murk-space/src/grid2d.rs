//! Shared helpers for 2D grid backends (Square4, Square8).

use crate::edge::EdgeBehavior;
use crate::error::SpaceError;
use crate::region::{BoundingShape, RegionPlan, RegionSpec};
use crate::space::Space;
use murk_core::Coord;
use smallvec::smallvec;
use std::collections::VecDeque;

/// Check that a 2D coordinate is in bounds and return `(row, col)`.
pub(crate) fn check_2d_bounds(
    coord: &Coord,
    rows: u32,
    cols: u32,
) -> Result<(i32, i32), SpaceError> {
    if coord.len() != 2 {
        return Err(SpaceError::CoordOutOfBounds {
            coord: coord.clone(),
            bounds: format!("expected 2D coordinate, got {}D", coord.len()),
        });
    }
    let r = coord[0];
    let c = coord[1];
    if r < 0 || r >= rows as i32 || c < 0 || c >= cols as i32 {
        return Err(SpaceError::CoordOutOfBounds {
            coord: coord.clone(),
            bounds: format!("[0, {}) x [0, {})", rows, cols),
        });
    }
    Ok((r, c))
}

/// Row-major canonical ordering: `[0,0], [0,1], ..., [rows-1, cols-1]`.
pub(crate) fn canonical_ordering_2d(rows: u32, cols: u32) -> Vec<Coord> {
    let mut out = Vec::with_capacity((rows as usize) * (cols as usize));
    for r in 0..rows as i32 {
        for c in 0..cols as i32 {
            out.push(smallvec![r, c]);
        }
    }
    out
}

/// Resolve a single axis value under the given edge behavior.
/// Returns `Some(clamped_value)` or `None` for Absorb out-of-bounds.
pub(crate) fn resolve_axis(val: i32, len: u32, edge: EdgeBehavior) -> Option<i32> {
    let n = len as i32;
    if val >= 0 && val < n {
        return Some(val);
    }
    match edge {
        EdgeBehavior::Absorb => None,
        EdgeBehavior::Clamp => Some(val.clamp(0, n - 1)),
        EdgeBehavior::Wrap => Some(((val % n) + n) % n),
    }
}

/// 1D distance along a single axis, accounting for wrap.
pub(crate) fn axis_distance(a: i32, b: i32, len: u32, edge: EdgeBehavior) -> f64 {
    let diff = (a - b).unsigned_abs();
    match edge {
        EdgeBehavior::Wrap => {
            let wrap = len - diff;
            diff.min(wrap) as f64
        }
        EdgeBehavior::Absorb | EdgeBehavior::Clamp => diff as f64,
    }
}

/// BFS-based disk compilation for 2D grids.
///
/// `get_neighbours` is called with `(row, col)` and should return the
/// neighbours for that cell according to the backend's connectivity.
pub(crate) fn compile_disk_2d(
    center_r: i32,
    center_c: i32,
    radius: u32,
    rows: u32,
    cols: u32,
    get_neighbours: impl Fn(i32, i32) -> Vec<(i32, i32)>,
) -> RegionPlan {
    let n = (rows as usize) * (cols as usize);
    let mut visited = vec![false; n];
    let mut queue = VecDeque::new();
    let mut result: Vec<(i32, i32)> = Vec::new();

    let idx = |r: i32, c: i32| (r as usize) * (cols as usize) + (c as usize);

    visited[idx(center_r, center_c)] = true;
    queue.push_back((center_r, center_c, 0u32));
    result.push((center_r, center_c));

    while let Some((r, c, dist)) = queue.pop_front() {
        if dist >= radius {
            continue;
        }
        for (nr, nc) in get_neighbours(r, c) {
            let i = idx(nr, nc);
            if !visited[i] {
                visited[i] = true;
                queue.push_back((nr, nc, dist + 1));
                result.push((nr, nc));
            }
        }
    }

    // Sort in row-major canonical order.
    result.sort();
    let coords: Vec<Coord> = result.iter().map(|&(r, c)| smallvec![r, c]).collect();
    let cell_count = coords.len();
    let tensor_indices: Vec<usize> = (0..cell_count).collect();
    let valid_mask = vec![1u8; cell_count];

    RegionPlan {
        cell_count,
        coords,
        tensor_indices,
        valid_mask,
        bounding_shape: BoundingShape::Rect(vec![cell_count]),
    }
}

/// Compile a region for a 2D grid space.
pub(crate) fn compile_region_2d(
    spec: &RegionSpec,
    rows: u32,
    cols: u32,
    _space: &dyn Space,
    get_neighbours: impl Fn(i32, i32) -> Vec<(i32, i32)>,
) -> Result<RegionPlan, SpaceError> {
    match spec {
        RegionSpec::All => {
            let coords = canonical_ordering_2d(rows, cols);
            let cell_count = coords.len();
            let tensor_indices: Vec<usize> = (0..cell_count).collect();
            let valid_mask = vec![1u8; cell_count];
            Ok(RegionPlan {
                cell_count,
                coords,
                tensor_indices,
                valid_mask,
                bounding_shape: BoundingShape::Rect(vec![rows as usize, cols as usize]),
            })
        }

        RegionSpec::Disk { center, radius } => {
            let (cr, cc) = check_2d_bounds(center, rows, cols)?;
            Ok(compile_disk_2d(cr, cc, *radius, rows, cols, &get_neighbours))
        }

        RegionSpec::Neighbours { center, depth } => {
            let (cr, cc) = check_2d_bounds(center, rows, cols)?;
            Ok(compile_disk_2d(cr, cc, *depth, rows, cols, &get_neighbours))
        }

        RegionSpec::Rect { min, max } => {
            let (r_lo, c_lo) = check_2d_bounds(min, rows, cols)?;
            let (r_hi, c_hi) = check_2d_bounds(max, rows, cols)?;
            if r_lo > r_hi || c_lo > c_hi {
                return Err(SpaceError::InvalidRegion {
                    reason: format!(
                        "Rect min ({r_lo},{c_lo}) > max ({r_hi},{c_hi}) on some axis"
                    ),
                });
            }
            let mut coords = Vec::new();
            for r in r_lo..=r_hi {
                for c in c_lo..=c_hi {
                    coords.push(smallvec![r, c]);
                }
            }
            let cell_count = coords.len();
            let tensor_indices: Vec<usize> = (0..cell_count).collect();
            let valid_mask = vec![1u8; cell_count];
            let shape_rows = (r_hi - r_lo + 1) as usize;
            let shape_cols = (c_hi - c_lo + 1) as usize;
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
                check_2d_bounds(coord, rows, cols)?;
            }
            let mut sorted: Vec<Coord> = coords.clone();
            sorted.sort();
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

