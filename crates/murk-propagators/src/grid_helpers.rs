//! Shared grid-topology helpers for Square4 propagators.
//!
//! Provides axis resolution (absorb/clamp/wrap) and 4-connected neighbour
//! lookup used by multiple propagators. Centralised here to eliminate
//! copy-paste duplication.

use murk_space::EdgeBehavior;

/// Resolve a single axis value under the given edge behavior.
/// Returns `Some(resolved)` or `None` for Absorb out-of-bounds.
pub(crate) fn resolve_axis(val: i32, len: i32, edge: EdgeBehavior) -> Option<i32> {
    if val >= 0 && val < len {
        return Some(val);
    }
    match edge {
        EdgeBehavior::Absorb => None,
        EdgeBehavior::Clamp => Some(val.clamp(0, len - 1)),
        EdgeBehavior::Wrap => Some(((val % len) + len) % len),
    }
}

/// Collect the flat indices of the 4-connected neighbours for cell (r,c),
/// respecting the grid's edge behavior.
pub(crate) fn neighbours_flat(
    r: i32,
    c: i32,
    rows: i32,
    cols: i32,
    edge: EdgeBehavior,
) -> smallvec::SmallVec<[usize; 4]> {
    let offsets: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
    let mut result = smallvec::SmallVec::new();
    for (dr, dc) in offsets {
        let nr = resolve_axis(r + dr, rows, edge);
        let nc = resolve_axis(c + dc, cols, edge);
        if let (Some(nr), Some(nc)) = (nr, nc) {
            result.push(nr as usize * cols as usize + nc as usize);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_axis_in_bounds() {
        assert_eq!(resolve_axis(2, 5, EdgeBehavior::Absorb), Some(2));
        assert_eq!(resolve_axis(0, 5, EdgeBehavior::Wrap), Some(0));
    }

    #[test]
    fn resolve_axis_absorb_out_of_bounds() {
        assert_eq!(resolve_axis(-1, 5, EdgeBehavior::Absorb), None);
        assert_eq!(resolve_axis(5, 5, EdgeBehavior::Absorb), None);
    }

    #[test]
    fn resolve_axis_clamp() {
        assert_eq!(resolve_axis(-1, 5, EdgeBehavior::Clamp), Some(0));
        assert_eq!(resolve_axis(7, 5, EdgeBehavior::Clamp), Some(4));
    }

    #[test]
    fn resolve_axis_wrap() {
        assert_eq!(resolve_axis(-1, 5, EdgeBehavior::Wrap), Some(4));
        assert_eq!(resolve_axis(5, 5, EdgeBehavior::Wrap), Some(0));
        assert_eq!(resolve_axis(7, 5, EdgeBehavior::Wrap), Some(2));
    }

    #[test]
    fn neighbours_flat_center_absorb() {
        let nbs = neighbours_flat(1, 1, 3, 3, EdgeBehavior::Absorb);
        assert_eq!(nbs.len(), 4);
        // (0,1)=1, (2,1)=7, (1,0)=3, (1,2)=5
        assert!(nbs.contains(&1));
        assert!(nbs.contains(&7));
        assert!(nbs.contains(&3));
        assert!(nbs.contains(&5));
    }

    #[test]
    fn neighbours_flat_corner_absorb() {
        let nbs = neighbours_flat(0, 0, 3, 3, EdgeBehavior::Absorb);
        assert_eq!(nbs.len(), 2);
        // (1,0)=3, (0,1)=1
        assert!(nbs.contains(&3));
        assert!(nbs.contains(&1));
    }

    #[test]
    fn neighbours_flat_corner_wrap() {
        let nbs = neighbours_flat(0, 0, 3, 3, EdgeBehavior::Wrap);
        assert_eq!(nbs.len(), 4);
        // Wraps: north=(2,0)=6, south=(1,0)=3, west=(0,2)=2, east=(0,1)=1
        assert!(nbs.contains(&6));
        assert!(nbs.contains(&3));
        assert!(nbs.contains(&2));
        assert!(nbs.contains(&1));
    }
}
