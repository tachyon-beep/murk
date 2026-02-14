# FCC12 Lattice: Implementation Plan

**Bead:** murk-04q
**Status:** Design
**Author:** Claude Opus 4.6 + John Morrissey
**Date:** 2026-02-14

**Revision 2 (2026-02-14):** Four corrections applied after review:
1. Distance formula corrected from L∞ to `max(max_abs, half_L1)` (§2.3, §6)
2. Canonical rank fixed for odd-dimension grids: even/odd z-slices have
   different cell counts when w and h are both odd (§4.2)
3. Clamp edge behavior specified: degrades to Absorb to prevent parity
   violation from single-axis clamping (§2.4, §5, §8)
4. CFL stability constant clarified: depends on physical vs graph spacing
   convention (§12)

## 1. Motivation

Murk's existing 2D backends (Square4, Square8, Hex2D) serve grid-world and
hex-world use cases. For 3D environments — volumetric simulations, 3D pursuit
dynamics, atmospheric models — we need an isotropic 3D lattice.

**Why FCC12, not Cube6?** The face-centred cubic (FCC) lattice is the 3D
analogue of Hex2D: every cell has 12 equidistant neighbours, giving minimal
directional bias for diffusion stencils, foveation regions, and agent movement.
A naive cubic lattice (Cube6) suffers from strong axis bias, just as Square4
does in 2D.

**Why FCC12, not BCC8?** BCC has only 8 nearest neighbours (all diagonal),
which gives worse angular coverage than FCC's 12. BCC14 (adding 6 face
neighbours) reintroduces a two-distance problem, negating the "single step
length" benefit.

## 2. Geometry Reference

### 2.1 Coordinate System

Integer triples `(x, y, z)` with the **parity constraint**:

```
(x + y + z) % 2 == 0
```

This selects exactly half the integer lattice points. For a bounding box
of dimensions `W × H × D`, the number of valid cells is:

```
cell_count = ceil(W*H*D / 2)
```

More precisely, for each `(y, z)` row, valid `x` values start at
`(y + z) % 2` and step by 2.

### 2.2 Neighbour Offsets

All 12 permutations of `(±1, ±1, 0)`:

```
(+1, +1,  0)   (-1, +1,  0)   (+1, -1,  0)   (-1, -1,  0)
(+1,  0, +1)   (-1,  0, +1)   (+1,  0, -1)   (-1,  0, -1)
( 0, +1, +1)   ( 0, -1, +1)   ( 0, +1, -1)   ( 0, -1, -1)
```

All offsets satisfy the parity constraint: if `(x+y+z)` is even, then
`(x±1, y±1, z)` has parity `x+y+z ± 1 ± 1 + z` which preserves
evenness. More precisely: each offset changes exactly two coordinates
by ±1, so the parity sum changes by `(±1) + (±1) = 0 or ±2`, both even.

### 2.3 Distance Metric

The FCC graph distance between two valid points `a` and `b` is:

```
d(a, b) = max(max(|dx|, |dy|, |dz|), (|dx| + |dy| + |dz|) / 2)
    where dx = a.x - b.x, dy = a.y - b.y, dz = a.z - b.z
```

This is **not** L∞. L∞ gives the wrong answer because each FCC step
changes exactly two axes, not one. Two lower bounds combine:

- **Lower bound 1 (max-abs):** Each step changes any single axis by at
  most 1, so we need at least `max(|dx|, |dy|, |dz|)` steps.
- **Lower bound 2 (half-L1):** Each step reduces the L1 displacement by
  exactly 2 (changes two axes by ±1 each toward target), so we need at
  least `(|dx| + |dy| + |dz|) / 2` steps.

The geodesic is the tighter of the two: `max(max_abs, half_L1)`.

**Why L1 is always even between valid cells:** Both endpoints satisfy
`(x+y+z) % 2 == 0`, so `|dx| + |dy| + |dz|` has the same parity as
`(a.x+a.y+a.z) - (b.x+b.y+b.z)` which is even. The division by 2 is
exact.

**Counterexample showing L∞ is wrong:** From `(0,0,0)` to `(2,2,2)`:
- L∞ says 2
- But each step changes only 2 axes, so in 2 steps we change at most
  4 axis-increments, but need 6
- Correct: `max(2, 6/2) = max(2, 3) = 3`

**Constructive proof:** At each step, pick the two axes with the
largest remaining |displacement| and step toward the target on both.
This reduces max-abs by 1 whenever the two largest components are
equal, and reduces half-L1 by 1 always. The strategy achieves
`max(max_abs, half_L1)` steps.

**Metric properties:**
- Reflexive: `max(0, 0) = 0` ✓
- Symmetric: absolute values ✓
- Triangle inequality: both max-abs and half-L1 satisfy it individually,
  so their max does too ✓

### 2.4 Edge Behavior

FCC uses the same `EdgeBehavior` enum as Square4/Square8, but the
parity constraint creates a subtlety: **axis-independent clamping can
produce coordinates that violate the parity invariant**.

**The Clamp parity problem:**

Consider cell `(0, 0, 0)` (valid, sum=0 even) with offset `(-1, +1, 0)`:
1. Clamp x: `-1 → 0`, y stays `1`, z stays `0`
2. Result: `(0, 1, 0)` — sum=1, **odd parity, not in the lattice**

This happens because clamping "cancels" one of the two ±1 changes,
so only one axis actually changes, flipping parity. The neighbour
code would push an invalid coordinate, and `canonical_rank` would
return `None` (or panic on `.unwrap()`).

**Resolution: Clamp acts as Absorb at the FCC level.**

If resolving any axis of a neighbour offset produces a clamped value
(i.e., the raw coordinate was out of bounds and got pulled back), that
offset is **dropped entirely** — same as Absorb. This is semantically
correct: "stay near the boundary" is already achieved by the cell being
at the boundary and having some valid neighbours; we don't need to
invent synthetic self-loop neighbours with broken parity.

This is implemented by tracking whether any axis was clamped:

```rust
// In neighbours(), for each offset:
let (nx, x_clamped) = resolve_axis_fcc(x + dx, self.w, self.edge);
let (ny, y_clamped) = resolve_axis_fcc(y + dy, self.h, self.edge);
let (nz, z_clamped) = resolve_axis_fcc(z + dz, self.d, self.edge);
match (nx, ny, nz) {
    (Some(nx), Some(ny), Some(nz)) if !(x_clamped || y_clamped || z_clamped) => {
        result.push(smallvec![nx, ny, nz]);
    }
    _ => {} // Absorb or clamped — skip this offset
}
```

Where `resolve_axis_fcc` returns `(Option<i32>, bool)` — the resolved
value plus a flag indicating whether clamping occurred.

**Absorb:** Offsets that go out of bounds are dropped. No parity issue —
the whole move is rejected.

**Wrap:** Offsets that go out of bounds wrap to the opposite side. The
parity constraint is preserved because both axes change by ±1 (wrap
doesn't cancel either change, it redirects both). This requires even
dimensions to ensure the parity checkerboard tiles correctly across
the wrap boundary — `(dim-1) + 1 → 0` must produce the same parity
class. When `dim` is even, the checkerboard tiles cleanly. When `dim`
is odd, the cell at coordinate 0 and the cell at coordinate `dim-1`
have the same parity on that axis, creating a discontinuity. We handle
this by **requiring even dimensions when Wrap is used**, returning
`SpaceError::InvalidComposition` otherwise.

**Summary:**

| EdgeBehavior | FCC12 semantics |
|-------------|-----------------|
| Absorb | Offset dropped if any axis out of bounds |
| Clamp | Offset dropped if any axis would clamp (degrades to Absorb) |
| Wrap | Offset wraps; requires even dimensions |

Note: Clamp is still accepted as a constructor argument (no error), but
its runtime behaviour at FCC boundaries is identical to Absorb. This is
documented in the struct-level doc comment. Users who truly want
"boundary hugging" should use Absorb explicitly.

## 3. Data Structures

### 3.1 Fcc12 Struct

```rust
#[derive(Debug, Clone)]
pub struct Fcc12 {
    /// Extent along x-axis. Valid x: 0..w, filtered by parity.
    w: u32,
    /// Extent along y-axis.
    h: u32,
    /// Extent along z-axis.
    d: u32,
    /// Precomputed cell count (valid parity cells only).
    cell_count: usize,
    /// Per-row x-offset: x_offset[y][z] = (y + z) % 2.
    /// Stored as flat lookup: offset(y,z) = (y + z) & 1.
    /// Not stored — computed inline (trivial).
    edge: EdgeBehavior,
    instance_id: SpaceInstanceId,
}
```

### 3.2 Construction

```rust
impl Fcc12 {
    pub const MAX_DIM: u32 = i32::MAX as u32;

    pub fn new(w: u32, h: u32, d: u32, edge: EdgeBehavior) -> Result<Self, SpaceError> {
        // Validate dimensions
        if w == 0 || h == 0 || d == 0 { return Err(SpaceError::EmptySpace); }
        for (name, val) in [("w", w), ("h", h), ("d", d)] {
            if val > Self::MAX_DIM {
                return Err(SpaceError::DimensionTooLarge { name, value: val, max: Self::MAX_DIM });
            }
        }

        // Wrap requires even dimensions for parity consistency
        if edge == EdgeBehavior::Wrap && (w % 2 != 0 || h % 2 != 0 || d % 2 != 0) {
            return Err(SpaceError::InvalidComposition {
                reason: "FCC12 with Wrap requires even dimensions for parity consistency".into(),
            });
        }

        // Count valid cells: for each (y, z), count x values with correct parity
        let cell_count = count_fcc_cells(w, h, d);

        Ok(Self { w, h, d, cell_count, edge, instance_id: SpaceInstanceId::next() })
    }
}
```

### 3.3 Cell Count Algorithm

For each `(y, z)` pair, valid `x` values start at `(y + z) % 2` and
step by 2, ending before `w`:

```rust
fn count_fcc_cells(w: u32, h: u32, d: u32) -> usize {
    // For each (y,z), the number of valid x is:
    //   start = (y + z) % 2
    //   count = (w - start + 1) / 2   (integer division, rounding down)
    //
    // Summing over all y,z:
    //   Half the (y,z) pairs have start=0, half have start=1.
    //   When h*d is even, exactly h*d/2 rows start at 0 and h*d/2 start at 1.
    //   n_even = ceil(w / 2) cells per start=0 row
    //   n_odd  = floor(w / 2) cells per start=1 row
    //
    // Fast closed-form:
    let hd = h as usize * d as usize;
    let n_even_rows = (hd + 1) / 2;  // rows where (y+z) % 2 == 0
    let n_odd_rows = hd / 2;          // rows where (y+z) % 2 == 1
    let x_even = ((w as usize) + 1) / 2; // valid x count when start=0
    let x_odd = (w as usize) / 2;         // valid x count when start=1
    n_even_rows * x_even + n_odd_rows * x_odd
}
```

**Verification:** For `w=h=d=2`: `hd=4`, `n_even=2`, `n_odd=2`,
`x_even=1`, `x_odd=1`, total=4. Manual check: valid coords are
`(0,0,0)`, `(1,1,0)`, `(0,1,1)`, `(1,0,1)` — exactly 4. ✓

For `w=h=d=4`: `hd=16`, `n_even=8`, `n_odd=8`, `x_even=2`, `x_odd=2`,
total=32 = 64/2. ✓

## 4. Canonical Ordering & Rank

### 4.1 Canonical Ordering

Lexicographic: z outer, y middle, x inner, **skipping invalid parity**:

```rust
fn canonical_ordering(&self) -> Vec<Coord> {
    let mut out = Vec::with_capacity(self.cell_count);
    for z in 0..self.d as i32 {
        for y in 0..self.h as i32 {
            let x_start = ((y + z) % 2) as i32;  // always 0 or 1
            let mut x = x_start;
            while x < self.w as i32 {
                out.push(smallvec![x, y, z]);
                x += 2;
            }
        }
    }
    out
}
```

### 4.2 Canonical Rank (O(1))

The dense index for `(x, y, z)` packs valid cells contiguously.

**Critical subtlety:** When `w` and `h` are both odd, even-z slices
and odd-z slices have **different cell counts**. The parity of `z`
determines which y-rows get `ceil(w/2)` valid x-values vs `floor(w/2)`.
A naive `z * cells_per_slice` formula silently gives wrong indices for
odd-dimension grids.

```rust
fn canonical_rank(&self, coord: &Coord) -> Option<usize> {
    if coord.len() != 3 { return None; }
    let (x, y, z) = (coord[0], coord[1], coord[2]);

    // Bounds check
    if x < 0 || x >= self.w as i32 ||
       y < 0 || y >= self.h as i32 ||
       z < 0 || z >= self.d as i32 { return None; }

    // Parity check
    if (x + y + z) % 2 != 0 { return None; }

    let w = self.w as usize;
    let x_even = (w + 1) / 2;  // valid x count when row start = 0
    let x_odd  = w / 2;         // valid x count when row start = 1

    let h = self.h as usize;
    let y_even_rows = (h + 1) / 2;  // count of even-index y rows
    let y_odd_rows  = h / 2;         // count of odd-index y rows

    // Two slice sizes: slice cell count depends on z parity.
    // When z is even: even y-rows have start=0 (x_even cells),
    //                 odd y-rows have start=1 (x_odd cells).
    // When z is odd:  even y-rows have start=1 (x_odd cells),
    //                 odd y-rows have start=0 (x_even cells).
    let slice_even = y_even_rows * x_even + y_odd_rows * x_odd;
    let slice_odd  = y_even_rows * x_odd  + y_odd_rows * x_even;

    // Count cells in all complete z-slices before this one.
    let z_us = z as usize;
    let z_even_ct = (z_us + 1) / 2;  // even z values in [0, z): 0, 2, 4, ...
    let z_odd_ct  = z_us / 2;         // odd z values in [0, z): 1, 3, 5, ...
    let cells_before_z = z_even_ct * slice_even + z_odd_ct * slice_odd;

    // Count cells in complete y-rows within this z-slice.
    // Which row gets x_even vs x_odd depends on (y + z) parity.
    let y_us = y as usize;
    let z_parity = (z & 1) as usize;
    // Rows 0..y: even-index rows have (row+z)%2 == z%2.
    let y_even_ct = (y_us + 1) / 2;  // even y values in [0, y)
    let y_odd_ct  = y_us / 2;         // odd y values in [0, y)
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
```

**Verification for odd dimensions (5×5×5):**

Slice z=0 (even): rows y=0,2,4 get x_even=3, rows y=1,3 get x_odd=2.
Slice cells = 3*3 + 2*2 = 13.

Slice z=1 (odd): rows y=0,2,4 get x_odd=2, rows y=1,3 get x_even=3.
Slice cells = 3*2 + 2*3 = 12.

Total = 13+12+13+12+13 = 63. Manual check: 5³/2 = 62.5, ceil = 63. ✓

The alternating slice sizes (13, 12, 13, 12, 13) are handled correctly
by counting even and odd z-slices separately.

## 5. Neighbours

```rust
const FCC_OFFSETS: [(i32, i32, i32); 12] = [
    ( 1,  1,  0), (-1,  1,  0), ( 1, -1,  0), (-1, -1,  0),
    ( 1,  0,  1), (-1,  0,  1), ( 1,  0, -1), (-1,  0, -1),
    ( 0,  1,  1), ( 0, -1,  1), ( 0,  1, -1), ( 0, -1, -1),
];

fn neighbours(&self, coord: &Coord) -> SmallVec<[Coord; 8]> {
    let (x, y, z) = (coord[0], coord[1], coord[2]);
    let mut result = SmallVec::new();  // will heap-allocate for >8
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
```

The `resolve_axis_fcc` helper wraps the existing `grid2d::resolve_axis`
logic but also reports whether clamping occurred:

```rust
fn resolve_axis_fcc(val: i32, len: u32, edge: EdgeBehavior) -> (Option<i32>, bool) {
    let n = len as i32;
    if val >= 0 && val < n {
        return (Some(val), false);
    }
    match edge {
        EdgeBehavior::Absorb => (None, false),
        EdgeBehavior::Clamp => (Some(val.clamp(0, n - 1)), true),  // clamped!
        EdgeBehavior::Wrap => (Some(((val % n) + n) % n), false),
    }
}
```
```

**Note on SmallVec<[Coord; 8]> spill:** FCC returns up to 12 neighbours.
SmallVec will heap-allocate when count > 8. This is acceptable for v1
because:
- The spill allocates ~96 bytes (4 extra Coords × ~24 bytes each)
- SmallVec reuses the buffer if the caller reuses the value
- Propagators that need tight loops can downcast to `Fcc12` and call a
  specialized `neighbours_xyz` that returns `SmallVec<[(i32,i32,i32); 12]>`
  (12 tuples, ~144 bytes inline, no Coord overhead)

If profiling shows this matters, we can:
1. Add a `neighbours_into()` method to the Space trait (buffer reuse)
2. Change the SmallVec capacity to 12 at the trait level (wastes stack for 4-connected grids)
3. Provide `Fcc12::neighbours_raw()` for hot-path specialization via downcast

## 6. Distance

```rust
fn distance(&self, a: &Coord, b: &Coord) -> f64 {
    let dx = axis_distance_3d(a[0], b[0], self.w, self.edge);
    let dy = axis_distance_3d(a[1], b[1], self.h, self.edge);
    let dz = axis_distance_3d(a[2], b[2], self.d, self.edge);

    let max_abs = dx.max(dy).max(dz);
    let half_l1 = (dx + dy + dz) / 2.0;

    max_abs.max(half_l1)
}
```

Where `axis_distance_3d` is the same as `grid2d::axis_distance` (it's
axis-independent — returns the absolute or wrapped per-axis distance as
`f64`). We reuse it directly since it's `pub(crate)` in `grid2d.rs`.

**Note on integer vs float division:** The `axis_distance` helper returns
`f64`, so `half_l1` is computed in floating point. Since both endpoints
satisfy the parity constraint, `dx + dy + dz` is always an even integer
(as `f64`), so the division is exact. No rounding issues.

## 7. Region Compilation

### 7.1 All

Straightforward: iterate canonical ordering, dense tensor indices,
all-ones valid mask.

```rust
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
```

**Bounding shape decision:** For "All", we use a flat 1D shape
`[cell_count]` rather than `[w, h, d]` because FCC cells don't fill
the full W×H×D grid. This matches how Hex2D handles "All" for its
`compile_region` (flat or `[rows, cols]` — but Hex2D happens to fill
every grid cell, so it uses `[rows, cols]`). For FCC we must use flat
because a `[w, h, d]` tensor would have 50% invalid entries.

### 7.2 Disk

BFS-based, following the `grid2d::compile_disk_2d` pattern but
generalized to 3D:

```rust
fn compile_fcc_disk(&self, cx: i32, cy: i32, cz: i32, radius: u32) -> RegionPlan {
    let mut visited = vec![false; self.cell_count];
    let mut queue = VecDeque::new();
    let mut result: Vec<Coord> = Vec::new();

    let center_rank = self.canonical_rank(&smallvec![cx, cy, cz]).unwrap();
    visited[center_rank] = true;
    queue.push_back((cx, cy, cz, 0u32));
    result.push(smallvec![cx, cy, cz]);

    while let Some((x, y, z, dist)) = queue.pop_front() {
        if dist >= radius { continue; }
        for (dx, dy, dz) in FCC_OFFSETS {
            let nx = resolve_axis_3d(x + dx, self.w, self.edge);
            let ny = resolve_axis_3d(y + dy, self.h, self.edge);
            let nz = resolve_axis_3d(z + dz, self.d, self.edge);
            if let (Some(nx), Some(ny), Some(nz)) = (nx, ny, nz) {
                let coord: Coord = smallvec![nx, ny, nz];
                let rank = self.canonical_rank(&coord).unwrap();
                if !visited[rank] {
                    visited[rank] = true;
                    queue.push_back((nx, ny, nz, dist + 1));
                    result.push(coord);
                }
            }
        }
    }

    // Sort by canonical rank for deterministic order
    result.sort_by_key(|c| self.canonical_rank(c).unwrap());
    // ... build RegionPlan with flat bounding shape
}
```

### 7.3 Rect

Axis-aligned bounding box in integer coordinates, filtered by parity:

```rust
RegionSpec::Rect { min, max } => {
    let (x_lo, y_lo, z_lo) = self.check_bounds(min)?;
    let (x_hi, y_hi, z_hi) = self.check_bounds(max)?;
    // Validate ordering
    if x_lo > x_hi || y_lo > y_hi || z_lo > z_hi {
        return Err(SpaceError::InvalidRegion { ... });
    }
    let mut coords = Vec::new();
    for z in z_lo..=z_hi {
        for y in y_lo..=y_hi {
            let x_start = x_lo + ((x_lo + y + z) % 2);  // first valid x >= x_lo
            let mut x = x_start;
            while x <= x_hi {
                coords.push(smallvec![x, y, z]);
                x += 2;
            }
        }
    }
    // Flat bounding shape since not all rect cells are valid
    let cell_count = coords.len();
    // ...
}
```

### 7.4 Neighbours & Coords

- **Neighbours:** delegates to `compile_fcc_disk` (same as Hex2D pattern)
- **Coords:** validates bounds + parity, sorts by canonical order, dedup

## 8. Edge Axis Resolution

FCC requires a parity-aware axis resolver that reports clamping (see §5).

For **neighbours**, we use `resolve_axis_fcc` (defined in §5) which
returns `(Option<i32>, bool)` — the resolved value plus a clamped flag.
If any axis was clamped, the entire offset is dropped to preserve parity.

For **distance**, we reuse `grid2d::axis_distance` directly since it
only computes the per-axis absolute displacement (no parity concern).
It's already `pub(crate)` in `grid2d.rs`, same crate.

**Implementation location:** Both `resolve_axis_fcc` and the FCC-specific
distance formula live in `fcc12.rs` as private helpers. No changes to
`grid2d.rs` are needed.

## 9. FFI Wiring

### 9.1 Space Type Enum

Add `Fcc12 = 6` to both enum locations:

**`crates/murk-ffi/src/types.rs`:**
```rust
pub enum MurkSpaceType {
    // ... existing variants ...
    /// 3D FCC lattice, 12-connected (isotropic).
    Fcc12 = 6,
}
```

**`crates/murk-python/src/config.rs`:**
```rust
pub(crate) enum SpaceType {
    // ... existing variants ...
    /// 3D FCC lattice, 12-connected (isotropic).
    Fcc12 = 6,
}
```

### 9.2 Config Parser

**`crates/murk-ffi/src/config.rs`:**
```rust
x if x == MurkSpaceType::Fcc12 as i32 => {
    // params = [w, h, d, edge_behavior]
    if p.len() < 4 { return None; }
    let w = p[0] as u32;
    let h = p[1] as u32;
    let d = p[2] as u32;
    let edge = parse_edge_behavior(p[3] as i32)?;
    Fcc12::new(w, h, d, edge)
        .ok()
        .map(|s| Box::new(s) as Box<dyn Space>)
}
```

### 9.3 Documentation Update

Update the `murk_config_set_space` doc comment:
```
/// - Fcc12: [w, h, d, edge_behavior]
```

## 10. Testing Strategy

### 10.1 Compliance Suite

Run `compliance::run_full_compliance` on multiple configurations:

```rust
#[test] fn compliance_4x4x4_absorb() { ... }
#[test] fn compliance_4x4x4_clamp() { ... }
#[test] fn compliance_4x4x4_wrap() { ... }
#[test] fn compliance_6x4x8_absorb() { ... }
#[test] fn compliance_2x2x2_absorb() { ... }  // minimal
```

This validates all 8 invariants: distance reflexive/symmetric/triangle,
neighbour symmetry, canonical ordering deterministic/complete, region
all valid ratio and coverage.

### 10.2 Unit Tests (following Hex2D pattern)

| Test | Validates |
|------|-----------|
| `neighbours_interior` | 12 neighbours for interior cell |
| `neighbours_corner_origin` | Absorb reduces count at `(0,0,0)` |
| `neighbours_face` | Edge cell, 3D face |
| `neighbours_edge` | Edge cell, 3D edge (intersection of 2 faces) |
| `distance_same_cell` | d(a,a) = 0 |
| `distance_adjacent` | d = 1 for all 12 neighbours |
| `distance_two_steps` | d = 2 for two-hop cells |
| `distance_balanced_diagonal` | `(0,0,0)→(2,2,2)` = 3, not 2 (L∞ counterexample) |
| `distance_unbalanced` | `(0,0,0)→(4,0,0)` = max(4, 4/2) = 4 (max_abs dominates) |
| `distance_cross_grid` | Corner-to-corner |
| `distance_wrap` | Wrap shortcut |
| `compile_region_all` | cell_count matches, valid_ratio = 1.0 |
| `compile_region_disk_r1` | Center + 12 = 13 (interior) |
| `compile_region_disk_r2` | Count matches theoretical |
| `compile_region_disk_boundary` | Truncated at Absorb edge |
| `compile_region_disk_huge_radius` | No overflow, returns all cells |
| `compile_region_rect` | Parity filtering correct |
| `compile_region_rect_invalid` | Err for min > max |
| `compile_region_coords` | Sorts by canonical order |
| `compile_region_coords_oob` | Err for out-of-bounds |
| `compile_region_coords_bad_parity` | Err for invalid parity |
| `canonical_ordering_lex` | z-then-y-then-x, parity-filtered |
| `canonical_rank_roundtrip` | rank ∘ ordering[i] == i |
| `cell_count_small` | 2×2×2 → 4 cells |
| `cell_count_formula` | Matches manual enumeration |
| `new_zero_dim` | Err(EmptySpace) |
| `new_dim_too_large` | Err(DimensionTooLarge) |
| `new_wrap_odd_dim` | Err(InvalidComposition) |
| `single_cell` | 1×1×1 with parity 0 = 1 cell |
| `downcast_ref` | Fcc12 downcasts correctly |
| `neighbours_clamp_drops_invalid` | Clamp at boundary drops moves that break parity |
| `neighbours_clamp_interior` | Clamp doesn't affect interior cells (12 neighbours) |
| `canonical_rank_odd_dims` | 5×5×5 rank roundtrips correctly (alternating slices) |
| `cell_count_odd_dims` | 5×5×5 = 63 cells (not 62 or 64) |

### 10.3 Property Tests (proptest)

```rust
proptest! {
    #[test]
    fn distance_is_metric(
        w in 2u32..6, h in 2u32..6, d in 2u32..6,
        // Generate valid FCC coordinates
        ax in 0i32..6, ay in 0i32..6, az in 0i32..6,
        bx in 0i32..6, by in 0i32..6, bz in 0i32..6,
        cx in 0i32..6, cy in 0i32..6, cz in 0i32..6,
    ) {
        // Snap to valid parity: adjust x if needed
        // ...
        // Test reflexive, symmetric, triangle inequality
    }

    #[test]
    fn neighbours_symmetric(/* ... */) { /* ... */ }

    #[test]
    fn canonical_rank_matches_ordering(/* ... */) {
        // For all cells: canonical_ordering()[rank(cell)] == cell
    }
}
```

### 10.4 Cross-validation

- **vs ProductSpace:** `Fcc12(4,4,4)` disk results should match
  running BFS manually on the same graph. We can construct the
  adjacency list from `canonical_ordering` + `neighbours` and run
  Dijkstra independently as a reference.

## 11. Implementation Steps

### Phase 1: Core Backend (murk-space)

| Step | File | Description |
|------|------|-------------|
| 1.1 | `crates/murk-space/src/fcc12.rs` | Struct, `new()`, `cell_count`, `count_fcc_cells()` |
| 1.2 | same | `canonical_ordering()`, `canonical_rank()` (loop version) |
| 1.3 | same | `neighbours()` with `FCC_OFFSETS`, `resolve_axis` reuse |
| 1.4 | same | `distance()` using `max(max_abs, half_L1)` with `axis_distance` |
| 1.5 | same | `compile_region()` — All, Rect, Coords |
| 1.6 | same | `compile_region()` — Disk, Neighbours (BFS) |
| 1.7 | same | Unit tests + compliance suite |
| 1.8 | same | Property tests |
| 1.9 | `crates/murk-space/src/lib.rs` | Add `pub mod fcc12`, `pub use fcc12::Fcc12` |

### Phase 2: FFI + Python Wiring

| Step | File | Description |
|------|------|-------------|
| 2.1 | `crates/murk-ffi/src/types.rs` | Add `Fcc12 = 6` to `MurkSpaceType` |
| 2.2 | `crates/murk-ffi/src/config.rs` | Add import + `parse_space` match arm |
| 2.3 | `crates/murk-python/src/config.rs` | Add `Fcc12 = 6` to Python `SpaceType` |

### Phase 3: Integration Testing

| Step | Description |
|------|-------------|
| 3.1 | End-to-end test: create Fcc12 world via FFI, step, read fields |
| 3.2 | Python integration test: `Config.set_space(SpaceType.Fcc12, [w, h, d, edge])` |
| 3.3 | ObsPlan test: compile + execute obs on Fcc12 world |

### Phase 4: Polish

| Step | Description |
|------|-------------|
| 4.1 | Update README.md space table |
| 4.2 | Update `examples/heat_seeker/README.md` backend table |
| 4.3 | Update crate-level doc comment in `lib.rs` |

## 12. CFL Note for Users

The CFL stability condition for explicit Euler diffusion depends on the
graph Laplacian degree and the physical spacing between neighbours.

For the FCC graph Laplacian `Δu = Σ_nbr u - 12·u` with **unit graph
spacing** (each step = distance 1 in the lattice metric):

```
CFL = 12 * D * dt < 1
```

However, the FCC neighbour offsets `(±1, ±1, 0)` have Euclidean length
`√2`, not 1. If users treat the lattice as embedded in physical space
with unit coordinate spacing, then `h² = 2` and the condition becomes:

```
CFL = 12 * D * dt / 2 = 6 * D * dt < 1
```

**Recommendation:** Document the CFL condition in terms of the user's
choice of `h`. The struct doc comment should state:

> For a graph Laplacian with degree 12, the explicit Euler stability
> bound is `degree * D * dt / h² < 1`. With FCC's Euclidean spacing
> of `h = √2`, this gives `6 * D * dt < 1`. With unit graph spacing
> (`h = 1`), this gives `12 * D * dt < 1`. Choose the convention
> that matches your propagator's stencil weights.

This is comparable to Cube6's `6 * D * dt / h²` with `h = 1`, i.e.
`6 * D * dt < 1`. The physical-spacing CFL for FCC and Cube6 are
actually the same — the extra neighbours are compensated by the longer
spacing. The graph-spacing CFL is 2× stricter for FCC.

## 13. Future Work (not in this bead)

- **Cube6 / Cube26:** Simple 3D cubic backends, easier to implement
  than FCC. Could share a `grid3d.rs` helper module.
- **BCC8 / BCC14:** Body-centred cubic, useful for some applications.
- **SmallVec capacity:** If FCC12's heap spill causes measurable
  performance issues, consider bumping `SmallVec<[Coord; 8]>` to
  `SmallVec<[Coord; 12]>` or adding `neighbours_into()`.
- **3D observation regions:** AgentSphere (3D disk), AgentCube (3D rect)
  region types for the observation system.
- **3D visualization:** matplotlib voxel plots or three.js viewers for
  FCC grids.

## 14. Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Parity arithmetic bugs | Medium | High (silent wrong results) | Property tests on rank↔ordering roundtrip; odd-dim-specific tests |
| Slice count alternation (odd w×h) | Medium | High (rank disagrees with ordering) | O(1) closed-form with separate even/odd slice sizes; verified in §4.2 |
| SmallVec spill perf | Low | Medium | Profile first; add specialization if needed |
| Wrap parity mismatch | Medium | High (neighbour symmetry breaks) | Even-dim constraint + compliance suite |
| Clamp producing invalid parity | High if unchecked | High (crash or phantom cells) | Clamp degrades to Absorb; clamped flag tracked in `resolve_axis_fcc` |
| Cell count overflow | Low | High | `checked_mul` in constructor |
| Distance formula wrong | Was High | High | Corrected from L∞ to `max(max_abs, half_L1)`; counterexample verified |
