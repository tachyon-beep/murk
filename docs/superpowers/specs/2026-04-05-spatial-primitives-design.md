# Spatial Primitives: Raycasting, Swept Collision, Projectile Advance, Region Stamp

**Date:** 2026-04-05
**Status:** Draft
**Crate scope:** `murk-space` (spatial queries), `murk-propagators` (propagator wrappers)

## Overview

Four spatial primitives that Murk lacks today. All are general-purpose operations
useful to any consumer with physics, projectiles, line-of-sight, or area effects.

| Primitive | Layer | ~Lines | Purpose |
|-----------|-------|--------|---------|
| Raycasting (3D DDA) | `murk-space` | ~200 | Core LOS/ballistic query |
| Swept collision | `murk-space` | ~120 | Sphere-cast for wide projectiles |
| ProjectileAdvance | `murk-propagators` | ~100 | Cell-field projectile integration |
| RegionStamp | `murk-propagators` | ~80 | Bulk-write area effects |

**Architecture:** Two-layer design.

- **Layer 1 — Spatial queries** in `murk-space`: Pure functions that take a `GridSpace`
  and geometric parameters, return lazy iterators. No field access, no `Propagator`
  trait. Reusable from propagators, observation code, tests, or consumer applications.
- **Layer 2 — Propagator wrappers** in `murk-propagators`: `Propagator` trait impls
  that use the layer-1 queries to read/write fields each tick. Also hosts the
  convenience functions (`raycast_first_match`, `swept_first_match`) that combine
  spatial queries with field data. Fits into the standard `WorldConfig` pipeline.

This separation exists because raycasting and swept collision are **queries**
("which cells does this ray cross?"), not field transformations. Locking them
behind the propagator pipeline would prevent reuse from observation code, agent
logic, and tests.

### Design Decision: Non-Axis-Aligned Topologies

GridSpace-based spatial operations are **intentionally not supported** for Hex2D
and Fcc12. These topologies are non-axis-aligned; the DDA algorithm is not
geometrically valid on them. This is a permanent design boundary, not a temporary
gap. Consumers who need projectiles or raycasting on hex grids should use the
entity model directly with topology-specific traversal in a custom propagator.

## 1. GridSpace Trait

DDA requires axis-aligned grid structure that the current `Space` trait does not
expose. New trait in `murk-space`:

```rust
/// Extension trait for spaces with axis-aligned regular grids.
///
/// Not all spaces are grid-spaces. Hex2D and Fcc12 are NOT axis-aligned
/// and do not implement this trait. Raycasting requires GridSpace.
///
/// GridSpace is object-safe by design — all methods use `&self`, return
/// owned types or references, and have no generic parameters. This enables
/// `&dyn GridSpace` and `Box<dyn GridSpace>`.
pub trait GridSpace: Space {
    /// Dimensions along each axis, e.g. &[32, 32, 10].
    fn axis_dims(&self) -> &[usize];

    /// Edge behavior for a given axis.
    fn axis_edge(&self, axis: usize) -> EdgeBehavior;

    /// Convert discrete grid coordinate to continuous cell-center point.
    fn cell_center(&self, coord: &Coord) -> Option<SmallVec<[f64; 4]>>;

    /// Which discrete cell contains a continuous point.
    /// Returns None if the point is outside the grid AND the relevant axes
    /// use Absorb edge behavior. For Wrap axes, the coordinate wraps.
    fn containing_cell(&self, point: &[f64]) -> Option<Coord>;
}
```

### Bridge: `Space::as_grid_space()`

Propagators receive `ctx.space()` as `&dyn Space`. To avoid exhaustive
concrete-type downcasting in every propagator that needs raycasting, `Space`
gains a bridge method:

```rust
// Added to the Space trait with a default impl:
fn as_grid_space(&self) -> Option<&dyn GridSpace> { None }
```

Grid backends (`Line1D`, `Ring1D`, `Square4`, `Square8`) override this to return
`Some(self)`. This centralizes the dispatch in one place — propagators call
`ctx.space().as_grid_space()` instead of a chain of `downcast_ref` calls.

**Implementors:** `Line1D`, `Ring1D`, `Square4`, `Square8`, and `ProductSpace`
(with runtime enforcement — see below).

### ProductSpace GridSpace Implementation

`ProductSpace` stores components as `Vec<Box<dyn Space>>`. It implements
`GridSpace` only when all components are themselves `GridSpace`. This is enforced
at **runtime** via `as_grid_space()`: the method downcasts each component; if any
component returns `None` from `as_grid_space()`, the ProductSpace also returns
`None`.

`axis_dims()` concatenates per-component dims. `axis_edge(n)` maps the
product-space axis index back to the correct component. `cell_center()` and
`containing_cell()` split/rejoin continuous coordinates across component
boundaries. `ProductSpace` materializes the concatenated `axis_dims` `Vec<usize>`
at construction time so `axis_dims()` can return `&[usize]`.

A `GridSpace` compliance test suite (mirroring the existing
`compliance::run_full_compliance` for `Space`) validates all implementors,
with `ProductSpace` as the most complex case.

**Unit-cell convention.** Cell `[i, j, k]` occupies the continuous cube
`[i, i+1) × [j, j+1) × [k, k+1)`, with center at `[i+0.5, j+0.5, k+0.5]`.
Consumers with physical cell sizes (e.g., 0.5m per cell) scale coordinates before
calling spatial queries. This keeps the trait simple and avoids threading physical
units through every spatial function.

## 2. Shared Types

```rust
/// Continuous point in space (f64 for sub-cell precision).
pub type ContinuousPoint = SmallVec<[f64; 4]>;

/// Direction vector.
pub type Direction = SmallVec<[f64; 4]>;

/// Result of a ray hitting a cell.
pub struct RayHit {
    /// Grid coordinate of the hit cell.
    pub coord: Coord,
    /// Flat canonical index of the hit cell.
    pub index: usize,
    /// Parametric distance along ray where it enters this cell.
    pub t_enter: f64,
    /// Parametric distance along ray where it exits this cell.
    pub t_exit: f64,
    /// Which face the ray entered through. None for the origin cell.
    ///
    /// The face encodes both axis and sign, which is load-bearing for
    /// directional mechanics: damage direction, ricochet angle, debris
    /// spray, and visual hit-effect placement all depend on knowing
    /// which side of a voxel was struck.
    pub entry_face: Option<Face>,
}

pub struct Face {
    /// Which axis the face is perpendicular to.
    pub axis: usize,
    /// Which direction the face points (Positive = +axis, Negative = -axis).
    pub sign: Sign,
}

pub enum Sign { Positive, Negative }
```

## 3. Raycasting (3D DDA)

N-dimensional Amanatides & Woo DDA through any `GridSpace`.

```rust
/// Cast a ray through a grid space, yielding each cell in traversal order.
///
/// Stops when `max_distance` is reached or the ray exits the grid.
/// Edge behavior per axis: Wrap continues across the boundary,
/// Absorb/Clamp terminate the ray.
pub fn raycast<S: GridSpace>(
    space: &S,
    origin: &[f64],
    direction: &[f64],
    max_distance: f64,
) -> RaycastIter<'_, S>
```

**Algorithm (Amanatides & Woo):**
1. Find the cell containing `origin`.
2. For each axis: compute `t_delta` (parametric distance to cross one cell) and
   `t_max` (distance to the next cell boundary in that axis).
3. Each step: advance along the axis with smallest `t_max`, yield the entered cell.
4. On boundary: check `axis_edge()` — `Wrap` wraps the coordinate, `Absorb`/`Clamp`
   terminates.

**Degenerate cases:**
- Zero-length direction: yield only the origin cell (if inside grid).
- Direction component exactly 0.0: ray is parallel to that axis, never steps along it.
- Origin outside grid: empty iterator.

**Numerical precision:** When `t_max` values tie (e.g., a 45-degree diagonal ray
where `t_delta` is equal on two axes), the implementation must break ties
deterministically — e.g., always prefer the lowest axis index. This prevents
non-deterministic axis selection and ensures no cell is yielded twice or skipped.
Origins exactly on cell boundaries (`[i+0.0, j+0.0]`) must be handled by the
`containing_cell` contract: the cell is `[i, j]` (the half-open interval
`[i, i+1)` includes `i`).

**Wrap-edge semantics:** A wrapping ray never exits the grid — it cycles. It
terminates only when `max_distance` is reached. A wrapping ray **can** re-enter
previously visited cells; there is no deduplication for `raycast`. A ray on a
`Ring1D(10)` with `max_distance = 15.0` yields 15 cells, re-entering some.

**Return type** is a lazy iterator (`RaycastIter`). A LOS check stops at the first
opaque cell; a ballistic trace collects all cells. No heap allocation during
iteration — state lives in the iterator struct.

**Borrow pattern for propagator consumers:** The iterator borrows `&S` (the space).
If you need to write to fields while iterating, collect the iterator into a
`Vec<RayHit>` first, then take the mutable write borrow. The convenience function
`raycast_first_match` (in `murk-propagators`) avoids this by taking the field as
a separate `&[f32]` parameter and returning a single `Option<RayHit>`.

**Complexity:** O(cells traversed). Each DDA step is O(ndim) to find the minimum
`t_max` — effectively O(1) per cell for 2D/3D.

### Convenience: `raycast_first_match` (in `murk-propagators`)

The 90% use case is "cast a ray, find the first cell matching a predicate."
This lives in `murk-propagators` (not `murk-space`) because it takes a field
slice — `murk-space` has no knowledge of field data.

```rust
/// Cast a ray and return the first cell where `predicate(field[hit.index])` is true.
pub fn raycast_first_match<S: GridSpace>(
    space: &S,
    origin: &[f64],
    direction: &[f64],
    max_distance: f64,
    field: &[f32],
    predicate: impl Fn(f32) -> bool,
) -> Option<RayHit>
```

This is ~5 lines wrapping the iterator. The raw iterator remains the power-user
API for consumers who need full traversal (ballistic traces, damage along a path).

## 4. Swept Collision (Sphere-Cast)

Raycasting variant where the "ray" has a radius — finds all cells whose center is
within `radius` of the ray's centerline.

```rust
pub fn swept_cast<S: GridSpace>(
    space: &S,
    origin: &[f64],
    direction: &[f64],
    radius: f64,
    max_distance: f64,
) -> SweptCastIter<'_, S>
```

**Algorithm:** Internally runs DDA along the centerline. At each DDA step, expands
to check all cells within `ceil(radius)` of the current ray position. A cell is
yielded if its center-to-ray distance <= `radius`.

**Result type:**

```rust
pub struct SweptHit {
    /// Grid coordinate of the hit cell.
    pub coord: Coord,
    /// Flat canonical index.
    pub index: usize,
    /// Parametric distance along ray at closest approach.
    pub t_closest: f64,
    /// Perpendicular distance from ray centerline to cell center.
    pub distance: f64,
    /// Face of entry for the centerline cell; None for expanded neighbors.
    pub entry_face: Option<Face>,
}
```

**Deduplication:** A cell near the ray over multiple DDA steps is yielded only once
(first encounter). The iterator tracks visited cells via a **bitset**
(`Vec<u64>`, sized to `cell_count` bits at iterator construction). Bitset provides
O(1) lookup, known size, and cache-friendly access. For a 256×256×10 grid
(~655K cells), the bitset is ~80KB — allocated once per `swept_cast` call.

**`radius = 0.0`** degenerates to standard raycasting — same cells, same order.
Consumers don't need to branch.

**Complexity:** O(cells traversed × r^d) where r = `ceil(radius)` and d = ndim. For
typical radii (1–3 cells in 3D), the expansion is ~7–27 neighbor checks per step.

### Convenience: `swept_first_match` (in `murk-propagators`)

Same pattern as `raycast_first_match`, lives in `murk-propagators`:

```rust
pub fn swept_first_match<S: GridSpace>(
    space: &S,
    origin: &[f64],
    direction: &[f64],
    radius: f64,
    max_distance: f64,
    field: &[f32],
    predicate: impl Fn(f32) -> bool,
) -> Option<SweptHit>
```

## 5. ProjectileAdvance Propagator

Cell-field projectile integration with gravity and collision. Reads position/velocity
from dense per-cell fields, advances projectiles through the grid each tick.

> **For entity-based projectile systems** (e.g., systems where projectiles have
> identity, ownership, and per-entity properties), use the raycasting utilities
> directly from a custom propagator that reads/writes entity properties. This
> propagator is designed for anonymous cell-field projectiles — particle effects,
> fluid simulation, simple grid games.

```rust
pub struct ProjectileAdvance {
    /// Scalar field — 1.0 = active projectile in this cell.
    presence_field: FieldId,
    /// Vector(ndim) field — velocity in cells/sec.
    velocity_field: FieldId,
    /// Optional Vector(ndim) field — sub-cell position [0, 1) per axis.
    offset_field: Option<FieldId>,
    /// Optional Scalar field — written 1.0 at collision cell.
    impact_field: Option<FieldId>,
    /// Acceleration in cells/sec^2.
    gravity: SmallVec<[f64; 4]>,
    /// Optional field to test for obstacles (e.g., terrain solidity).
    collision_field: Option<FieldId>,
    /// Value above which a cell in collision_field blocks projectiles.
    collision_threshold: f32,
}
```

**Per-tick algorithm (Jacobi double-buffer):**

The propagator reads from `reads_previous()` (frozen tick-start snapshot) and
writes to fresh output buffers. This prevents write-order nondeterminism — two
projectiles passing through each other's cells in the same tick produce
deterministic results regardless of scan order.

1. Read presence from `reads_previous()`. Scan in **canonical ordering** for
   deterministic iteration.
2. For each cell with presence > 0:
   a. Read velocity from previous; compute `new_vel = vel + gravity * dt`.
   b. Compute displacement = `new_vel * dt`.
   c. If `collision_field` is set: `raycast_first_match` from current position in
      displacement direction with `max_distance = |displacement|`, testing obstacle
      field against threshold. The full displacement is raycast — no single-cell
      clamping — so fast projectiles cannot tunnel through thin walls.
   d. **No collision:** compute destination cell, write presence=1.0 at destination
      in output buffer, write updated velocity.
   e. **Collision:** write impact=1.0 at hit cell, do NOT write presence at
      destination (projectile is consumed).
3. If `offset_field` is set, maintain sub-cell precision; otherwise snap to cell centers.

**Write modes:**
- `presence_field`: Full (fresh buffer each tick — Jacobi pattern)
- `velocity_field`: Full
- `offset_field`: Full (if present)
- `impact_field`: Full (zeroed each tick, only collision cells get 1.0)

**Reads:**
- `reads_previous()`: presence, velocity, offset (frozen tick-start)
- `reads_previous()`: collision field (if set — reads terrain from frozen snapshot)

**One-projectile-per-cell limitation.** If two projectiles converge to the same cell,
the second overwrites the first. For dense projectile scenarios, use separate
`ProjectileAdvance` instances with distinct field sets, or use the raycasting
utilities directly in a custom propagator.

**Gravity dimensionality.** `SmallVec<[f64; 4]>` works for any ndim. A 2D space uses
`[0.0, -9.81]`, a 3D space uses `[0.0, 0.0, -9.81]`. Validated at build time
against the space's `ndim()`.

**Builder:**

```rust
ProjectileAdvance::builder()
    .presence_field(f_presence)
    .velocity_field(f_velocity)
    .gravity([0.0, 0.0, -9.81])
    .collision_field(f_terrain)
    .collision_threshold(0.5)
    .impact_field(f_impact)
    .build()
```

## 6. RegionStamp Propagator

Trigger-driven bulk write to all cells in a compiled region. The "explosion crater"
primitive.

```rust
pub struct RegionStamp {
    /// Scalar field — value > 0 marks a stamp center.
    trigger_field: FieldId,
    /// Scalar field — receives the stamped value.
    target_field: FieldId,
    /// Geometry of the stamp centered on each trigger cell.
    shape: StampShape,
    /// What value to write (or how to derive it from the trigger).
    value_source: ValueSource,
    /// How to combine the stamp with existing target values.
    mode: StampMode,
    /// Clear trigger cells after stamping? Default: true.
    /// When false, triggers persist and fire every tick — use with care.
    consume_trigger: bool,
    /// Pre-compiled region plan (template centered at origin).
    /// Compiled once at build time; translated per trigger at runtime.
    template_plan: RegionPlan,
}

pub enum StampShape {
    /// Graph-distance disk (uses existing RegionSpec::Disk).
    Disk { radius: u32 },
    /// Axis-aligned box (uses existing RegionSpec::Rect).
    Rect { half_extents: Coord },
}

pub enum ValueSource {
    /// Always stamp this fixed value.
    Fixed(f32),
    /// Stamp the trigger cell's field value.
    FromTrigger,
    /// Stamp trigger_value * scale.
    ScaledTrigger(f32),
}

pub enum StampMode {
    Set,  // target[i] = value
    Add,  // target[i] += value
    Max,  // target[i] = max(target[i], value)
    Min,  // target[i] = min(target[i], value)
}
```

**Pre-compiled region plan.** The builder compiles a `RegionPlan` template once
at construction time (centered at the coordinate origin). At runtime, for each
trigger cell, the template's coordinates are translated by the trigger cell's
offset — O(plan.coords.len()) per trigger, no BFS. This avoids the cost of
calling `compile_region()` per trigger per tick, which would be O(BFS) per
trigger. For 100 triggers at radius 3, this is the difference between 100 BFS
traversals and 100 coordinate translations.

**Per-tick algorithm:**
1. Scan trigger field for cells with value > 0.
2. For each trigger cell:
   a. Derive the stamp value from `ValueSource`.
   b. Translate the pre-compiled `template_plan` coordinates by the trigger cell's
      position.
   c. For each translated coordinate: if in bounds, apply `StampMode` to target
      field.
3. If `consume_trigger` (default: `true`): write 0.0 to all processed trigger cells.

**Overlap behavior.** When two trigger cells' stamp regions intersect, the overlap
cells receive both stamps applied sequentially in canonical scan order. For
`StampMode::Add`, this means overlap cells get 2× the value. For `StampMode::Set`,
the later trigger's value wins. This ordering is deterministic (canonical scan of
trigger cells).

**Write modes:**
- `target_field`: Incremental (stamp modifies only affected cells)
- `trigger_field`: Incremental (only if `consume_trigger`)

**Builder:**

```rust
RegionStamp::builder()
    .trigger_field(f_explosion)
    .target_field(f_terrain_damage)
    .shape(StampShape::Disk { radius: 3 })
    .value_source(ValueSource::FromTrigger)
    .mode(StampMode::Add)
    // consume_trigger defaults to true
    .build()
```

## 7. Testing Strategy

### GridSpace Compliance Suite

A `gridspace_compliance::run_full_compliance` harness (mirroring the existing
`compliance::run_full_compliance` for `Space`) that validates all `GridSpace`
implementors:

- `cell_center`/`containing_cell` round-trip: `containing_cell(cell_center(c)) == c`
  for all valid coords
- Converse: `cell_center(containing_cell(p))` is the center of the cell containing `p`
- `axis_dims` product equals `cell_count()`
- `axis_edge(n)` returns valid `EdgeBehavior` for all `n < ndim`
- `as_grid_space()` returns `Some` for all grid backends

**ProductSpace-specific tests:**
- `Square4(4,4) × Line1D(8)`: `axis_dims()` returns `[4, 4, 8]`
- `axis_edge(0)` and `axis_edge(2)` return correct per-component `EdgeBehavior`
- `cell_center([2, 1, 3])` returns `[2.5, 1.5, 3.5]`
- Mixed edge behavior: one Wrap component, one Absorb — verify coordinate
  split/rejoin respects each component independently
- Non-GridSpace component: `ProductSpace` with `Hex2D` component returns `None`
  from `as_grid_space()`

### Raycasting Tests

**Unit tests:**
- Axis-aligned ray through known cells (verify exact cell sequence)
- Diagonal ray at 45 degrees (tie-breaking: lowest axis index wins)
- Ray origin exactly at cell corner `[0.0, 0.0]` — enters cell `[0, 0]`
- Ray along cell edge (`origin [0.5, 1.0]`, direction `[1.0, 0.0]`) — boundary
  between rows 0 and 1
- Corner-grazing ray that passes through a cell corner
- Wrap edge: `Ring1D(10)`, ray crosses boundary, yields correct wrapped cells
- Wrap termination: `Ring1D(10)`, `max_distance = 15.0` — yields 15 cells,
  re-entering visited cells
- Absorb edge: `Line1D(10, Absorb)`, ray terminates at boundary
- Zero-length direction: yields only origin cell
- Origin outside grid: empty iterator

**Property tests:**
- `containing_cell(origin + direction * t_enter) == coord` for every hit
- `t_enter` is strictly monotonically increasing across the hit sequence
- `t_exit` of cell n equals `t_enter` of cell n+1 (no parametric gaps or overlaps)
- No duplicate coords in a single raycast (non-wrapping spaces)
- `entry_face` correctness: ray from +x direction yields `Face { axis: 0, sign: Negative }` on entry

**Negative tests:**
- Origin outside grid → empty iterator (no panic)
- `max_distance = 0.0` → yields at most the origin cell
- `max_distance < 0.0` → empty iterator

### Swept Collision Tests

**Unit tests:**
- `radius = 0.0` matches `raycast`: identical cells in identical order
- Known geometry: `radius = 0.5` in 2D, verify which cells are yielded
- Boundary: cell center at exactly `radius` distance is included (`<= radius`)

**Property tests:**
- All yielded cells are within `radius` of ray centerline (precision)
- No cell within `radius` of the centerline is missed (completeness / recall)
- No duplicate coords in output

**Negative tests:**
- `radius < 0.0` → error or empty, not panic
- `max_distance = 0.0` → yields at most cells within radius of origin

### ProjectileAdvance Tests

**Unit tests:**
- Straight-line motion: single projectile, no gravity, no collision — arrives at
  correct cell after one tick
- Gravity arc: verify `new_vel = vel + gravity * dt` is applied
- Collision at known wall: projectile hits wall, impact field written, presence cleared
- Multi-cell step (tunneling prevention): velocity = 5.0 cells/sec, dt = 0.5 —
  projectile traverses 2.5 cells, must hit wall at cell 2, not tunnel to cell 5
- One-per-cell overwrite: two projectiles converge to same cell — document which wins
- Empty field: all-zero presence — step succeeds, writes nothing, no panic

**Property tests:**
- Presence count is conserved (minus collisions) across ticks
- With a wall present: presence count decreases by exactly the number of collisions

**Negative tests:**
- Builder rejects gravity vector with wrong dimensionality
- Builder rejects missing required fields (presence, velocity)

### RegionStamp Tests

**Unit tests:**
- Single trigger: correct cells stamped, correct value
- Multiple triggers: both regions stamped
- Overlapping triggers with `StampMode::Add`: overlap cells get 2× value
- Overlapping triggers with `StampMode::Set`: later trigger (canonical order) wins
- Each `StampMode` variant tested individually
- `consume_trigger = true`: trigger cells zeroed after stamping
- `consume_trigger = false`: trigger cells retain value after stamping
- Empty field: all-zero trigger — step succeeds cleanly, no panic

**Property tests:**
- Stamped cells match translated `template_plan` coordinates for each trigger
- For `StampMode::Max`: `target[i]` after stamping >= pre-stamp value for all
  cells in any trigger's region

**Negative tests:**
- Builder rejects missing required fields (trigger, target)

### Integration Tests

**Golden path:** Build a `WorldConfig` with terrain, a `ProjectileAdvance`, and a
`RegionStamp`. Fire a projectile, let it hit terrain, trigger an explosion stamp
at the impact site. Verify:
- Exact cell coordinates of the impact
- Presence field is zero at the old projectile location
- Terrain damage field modified in exactly the cells within stamp radius
- Cells outside stamp radius are unchanged

**No-collision path:** Projectile in open space across multiple ticks — verify it
continues moving correctly through the `WorldConfig` pipeline, not just in
unit isolation.

**Gravity arc integration:** At least one tick with non-zero gravity to verify
velocity integration is computed and written correctly through the pipeline.

**Stamp-only integration:** `RegionStamp` alone with a manually set trigger field
(no `ProjectileAdvance`), to isolate stamp correctness from the projectile
pipeline.

**Persistent trigger:** `consume_trigger = false` — verify the stamp fires again
on subsequent ticks with the trigger still active.

## 8. Public API Surface

### `murk-space` additions

```
// Trait (object-safe)
pub trait GridSpace: Space { ... }

// Bridge method on Space trait
fn as_grid_space(&self) -> Option<&dyn GridSpace> { None }

// Types
pub type ContinuousPoint = SmallVec<[f64; 4]>;
pub type Direction = SmallVec<[f64; 4]>;
pub struct RayHit { ... }
pub struct SweptHit { ... }
pub struct Face { ... }
pub enum Sign { Positive, Negative }

// Spatial query functions (pure geometry, no field access)
pub fn raycast<S: GridSpace>(...) -> RaycastIter<'_, S>
pub fn swept_cast<S: GridSpace>(...) -> SweptCastIter<'_, S>

// GridSpace impls for: Line1D, Ring1D, Square4, Square8, ProductSpace
// GridSpace compliance test suite
```

### `murk-propagators` additions

```
// Convenience functions (combine spatial queries with field data)
pub fn raycast_first_match<S: GridSpace>(..., field: &[f32], ...) -> Option<RayHit>
pub fn swept_first_match<S: GridSpace>(..., field: &[f32], ...) -> Option<SweptHit>

// Propagators
pub struct ProjectileAdvance { ... }
pub struct ProjectileAdvanceBuilder { ... }

pub struct RegionStamp { ... }
pub struct RegionStampBuilder { ... }
pub enum StampShape { Disk { radius: u32 }, Rect { half_extents: Coord } }
pub enum ValueSource { Fixed(f32), FromTrigger, ScaledTrigger(f32) }
pub enum StampMode { Set, Add, Max, Min }
```
