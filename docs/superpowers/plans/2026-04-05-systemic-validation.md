# Systemic Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close two systemic validation gaps: engine-level buffer length checks (Part 1) and Space coord arity assertions (Part 2).

**Architecture:** Part 1 caches expected buffer lengths (from `FieldDef` component counts) at `TickEngine::new()` and validates all read/write buffers before dispatching `step()`. Part 2 adds `debug_assert_eq!` to all Space impl hot paths, with the compliance suite as the primary structural enforcement.

**Tech Stack:** Rust, murk engine/propagator/space/arena crates

**Spec:** `docs/superpowers/specs/2026-04-04-systemic-validation-design.md`

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `crates/murk-engine/src/tick.rs` | Modify | `FieldExpectations` struct, construction, validation loop, tests |
| `crates/murk-space/src/compliance.rs` | Modify | Arity + ndim compliance tests |
| `crates/murk-space/src/line1d.rs` | Modify | `debug_assert_eq!` in neighbours/distance |
| `crates/murk-space/src/ring1d.rs` | Modify | `debug_assert_eq!` in neighbours/distance |
| `crates/murk-space/src/square4.rs` | Modify | `debug_assert_eq!` in neighbours/distance |
| `crates/murk-space/src/square8.rs` | Modify | `debug_assert_eq!` in neighbours/distance |
| `crates/murk-space/src/hex2d.rs` | Modify | `debug_assert_eq!` in neighbours/distance |
| `crates/murk-space/src/fcc12.rs` | Modify | `debug_assert_eq!` in neighbours/distance |
| `crates/murk-space/src/product.rs` | Modify | `debug_assert_eq!` in neighbours/distance |

---

## Task 1: Add `FieldExpectations` and Build at Construction

**Files:**
- Modify: `crates/murk-engine/src/tick.rs:91-206`

- [ ] **Step 1: Add `FieldExpectations` struct**

Insert above the `TickEngine` struct definition (before line 91). This holds
per-propagator expected buffer lengths, computed once from `FieldDef`:

```rust
/// Per-propagator expected field buffer lengths, computed once at construction.
///
/// Built from `FieldDef::field_type.components()` and `cell_count`. Used by
/// the pre-dispatch validation loop to check buffer sizes without calling
/// `reads()`/`writes()` per-tick (those return heap-allocated collections).
struct FieldExpectations {
    /// `read[propagator_index]` = Vec of (FieldId, expected_len).
    /// Covers both `reads()` and `reads_previous()` fields.
    read: Vec<Vec<(FieldId, usize)>>,
    /// `write[propagator_index]` = Vec of (FieldId, expected_len).
    write: Vec<Vec<(FieldId, usize)>>,
}
```

- [ ] **Step 2: Add `expectations` field to `TickEngine`**

In the `TickEngine` struct (line 96), add after `plan`:

```rust
    plan: ReadResolutionPlan,
    expectations: FieldExpectations,
```

- [ ] **Step 3: Build expectations in `TickEngine::new()`**

Insert after `arena_field_defs` is built (after line 148) and before the
`cell_count` computation (line 151). We need cell_count first, so move
the field_total_lens computation to after line 155:

```rust
        // Safety: validate() already checked cell_count fits in u32.
        let cell_count = u32::try_from(config.space.cell_count()).map_err(|_| {
            ConfigError::CellCountOverflow {
                value: config.space.cell_count(),
            }
        })?;

        // Build field_id -> expected total_len lookup from FieldDefs.
        // Single source of truth: cell_count * components (same formula the arena uses).
        let field_total_lens: std::collections::HashMap<FieldId, usize> = arena_field_defs
            .iter()
            .map(|(id, def)| {
                let total = cell_count as usize * def.field_type.components() as usize;
                (*id, total)
            })
            .collect();

        // Build per-propagator expected buffer lengths.
        // reads()/writes() are called once here, not per-tick.
        let expectations = {
            let mut read_exp = Vec::with_capacity(config.propagators.len());
            let mut write_exp = Vec::with_capacity(config.propagators.len());
            for prop in &config.propagators {
                let reads: Vec<(FieldId, usize)> = prop
                    .reads()
                    .iter()
                    .chain(prop.reads_previous().iter())
                    .filter_map(|fid| field_total_lens.get(&fid).map(|&len| (fid, len)))
                    .collect();
                let writes: Vec<(FieldId, usize)> = prop
                    .writes()
                    .iter()
                    .filter_map(|(fid, _mode)| field_total_lens.get(fid).map(|&len| (*fid, len)))
                    .collect();
                read_exp.push(reads);
                write_exp.push(writes);
            }
            FieldExpectations {
                read: read_exp,
                write: write_exp,
            }
        };
```

Note: `filter_map` is intentional — if a field ID isn't in `field_total_lens`,
it means the propagator declared a field that wasn't registered. Pipeline
validation (`validate_pipeline` at line 130) already catches this at
construction, so we silently skip it here. The pre-dispatch loop will catch
it at runtime via `None` from the reader/writer.

- [ ] **Step 4: Wire expectations into the return**

In the `Ok(Self { ... })` block (around line 189):

```rust
        Ok(Self {
            arena,
            propagators: config.propagators,
            plan,
            expectations,
            ingress,
            // ... rest unchanged
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p murk-engine`
Expected: compiles (unused field warning for `expectations` is fine).

- [ ] **Step 6: Commit**

```
feat(engine): add FieldExpectations cache for buffer length validation

Computes expected buffer lengths (cell_count * components) once at
TickEngine construction from FieldDefs. No per-tick allocation.
Validation loop will be added in the next commit.
```

---

## Task 2: Add Pre-Dispatch Validation Loop

**Files:**
- Modify: `crates/murk-engine/src/tick.rs:372-399` (between scratch reset and StepContext)

- [ ] **Step 1: Add the validation loop**

In the propagator dispatch loop, after `self.propagator_scratch.reset();`
(line 373) and before the StepContext block (line 376), insert:

```rust
            // 4dx. Validate field buffer lengths before dispatch.
            for &(field_id, expected_len) in &self.expectations.read[i] {
                match overlay.read(field_id) {
                    Some(buf) if buf.len() != expected_len => {
                        let prop_name = prop.name().to_string();
                        return self.handle_rollback(
                            prop_name,
                            PropagatorError::ExecutionFailed {
                                reason: format!(
                                    "read field {:?} buffer length {} != expected {}",
                                    field_id,
                                    buf.len(),
                                    expected_len,
                                ),
                            },
                            receipts,
                            accepted_receipt_start,
                        );
                    }
                    None => {
                        let prop_name = prop.name().to_string();
                        return self.handle_rollback(
                            prop_name,
                            PropagatorError::ExecutionFailed {
                                reason: format!(
                                    "declared read field {:?} not present",
                                    field_id,
                                ),
                            },
                            receipts,
                            accepted_receipt_start,
                        );
                    }
                    _ => {}
                }
            }
            for &(field_id, expected_len) in &self.expectations.write[i] {
                match guard.writer.read(field_id) {
                    Some(buf) if buf.len() != expected_len => {
                        let prop_name = prop.name().to_string();
                        return self.handle_rollback(
                            prop_name,
                            PropagatorError::ExecutionFailed {
                                reason: format!(
                                    "write field {:?} buffer length {} != expected {}",
                                    field_id,
                                    buf.len(),
                                    expected_len,
                                ),
                            },
                            receipts,
                            accepted_receipt_start,
                        );
                    }
                    None => {
                        let prop_name = prop.name().to_string();
                        return self.handle_rollback(
                            prop_name,
                            PropagatorError::ExecutionFailed {
                                reason: format!(
                                    "declared write field {:?} not present",
                                    field_id,
                                ),
                            },
                            receipts,
                            accepted_receipt_start,
                        );
                    }
                    _ => {}
                }
            }
```

**Key design notes:**
- Read validation uses `overlay.read()` — the `OverlayReader` implements
  `FieldReader` with `&self`, no borrow conflict.
- Write validation uses `guard.writer.read()` — `WriteArena` has
  `fn read(&self, field) -> Option<&[f32]>` (line 123 of write.rs),
  used elsewhere at step 4a for staged-cache population. No new methods needed.
- Errors go through `handle_rollback()` — same path as propagator `step()`
  failures, ensuring consistent rollback semantics.

- [ ] **Step 2: Add `PropagatorError` import if needed**

Check existing imports in tick.rs. `PropagatorError` may already be in scope
via `use murk_core::error::StepError` or `use murk_core::PropagatorError`.
If not, add to the import block:

```rust
use murk_core::PropagatorError;
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p murk-engine`
Expected: compiles with no errors.

- [ ] **Step 4: Run all existing tests**

Run: `cargo test -p murk-engine`
Expected: all existing tests pass. The validation loop runs on every tick
but all existing tests use correctly-sized buffers, so validation passes
silently.

- [ ] **Step 5: Commit**

```
feat(engine): validate field buffer lengths before propagator dispatch

Checks all read and write buffer lengths against FieldDef-derived
expectations before calling step(). Fails early via handle_rollback()
on length mismatch or missing declared fields. Uses overlay.read() for
read fields and guard.writer.read() for write fields — no new trait
methods needed.
```

---

## Task 3: Engine Validation Tests

**Files:**
- Modify: `crates/murk-engine/src/tick.rs` (test module, starting around line 609)

- [ ] **Step 1: Add test helpers**

Inside the `mod tests` block, add:

```rust
    fn vector_field(name: &str, dims: u32) -> FieldDef {
        FieldDef {
            name: name.to_string(),
            field_type: FieldType::Vector { dims },
            mutability: FieldMutability::PerTick,
            units: None,
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        }
    }
```

- [ ] **Step 2: Test — scalar engine passes validation**

```rust
    #[test]
    fn buffer_validation_passes_scalar_engine() {
        let mut engine = simple_engine();
        let result = engine.execute_tick();
        assert!(result.is_ok(), "correctly configured scalar engine should pass validation");
    }
```

Run: `cargo test -p murk-engine buffer_validation_passes_scalar`
Expected: PASS

- [ ] **Step 3: Test — vector field engine passes validation**

```rust
    #[test]
    fn buffer_validation_passes_vector_field() {
        struct VectorWriter;
        impl Propagator for VectorWriter {
            fn name(&self) -> &str {
                "vec_writer"
            }
            fn reads(&self) -> FieldSet {
                FieldSet::empty()
            }
            fn writes(&self) -> Vec<(FieldId, WriteMode)> {
                vec![(FieldId(0), WriteMode::Full)]
            }
            fn step(
                &self,
                ctx: &mut murk_propagator::StepContext<'_>,
            ) -> Result<(), PropagatorError> {
                let out = ctx.writes().write(FieldId(0)).unwrap();
                out.fill(0.0);
                Ok(())
            }
        }

        let config = WorldConfig::builder()
            .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
            .fields(vec![vector_field("velocity", 2)])
            .propagators(vec![Box::new(VectorWriter)])
            .dt(0.1)
            .seed(42)
            .build()
            .unwrap();
        let mut engine = TickEngine::new(config).unwrap();
        assert!(
            engine.execute_tick().is_ok(),
            "vector field with cell_count*2 buffer should pass validation"
        );
    }
```

Run: `cargo test -p murk-engine buffer_validation_passes_vector`
Expected: PASS

- [ ] **Step 4: Test — multi-propagator pipeline passes validation**

```rust
    #[test]
    fn buffer_validation_passes_multi_propagator() {
        let mut engine = three_field_engine();
        let result = engine.execute_tick();
        assert!(result.is_ok(), "three-propagator pipeline should pass validation");
    }
```

Run: `cargo test -p murk-engine buffer_validation_passes_multi`
Expected: PASS

- [ ] **Step 5: Test — reads_previous path works on tick 1**

Verify that reads_previous fields (routed through base_cache) are validated
correctly on the second tick (tick 1 reads tick 0's published snapshot):

```rust
    #[test]
    fn buffer_validation_passes_reads_previous_on_tick_1() {
        let mut engine = two_field_engine();
        // Tick 0: base cache populated from initial arena state.
        assert!(engine.execute_tick().is_ok());
        // Tick 1: reads_previous sees tick 0's published snapshot.
        assert!(engine.execute_tick().is_ok());
    }
```

Run: `cargo test -p murk-engine buffer_validation_passes_reads_previous`
Expected: PASS

- [ ] **Step 6: Test — rollback path from failing propagator still works**

Verify that the existing rollback path (propagator fails inside step())
still functions correctly after adding pre-dispatch validation:

```rust
    #[test]
    fn rollback_still_works_with_validation() {
        let mut engine = failing_engine(0); // fails immediately
        let result = engine.execute_tick();
        assert!(result.is_err());
        match result.unwrap_err().kind {
            StepError::PropagatorFailed { name, .. } => {
                assert_eq!(name, "fail");
            }
            other => panic!("expected PropagatorFailed, got {other:?}"),
        }
    }
```

Run: `cargo test -p murk-engine rollback_still_works_with_validation`
Expected: PASS

- [ ] **Step 7: Test — FieldExpectations computes correct lengths**

Unit test for the construction logic. Verify that scalar fields get
`cell_count * 1` and vector fields get `cell_count * dims`:

```rust
    #[test]
    fn field_expectations_computes_correct_lengths() {
        let config = WorldConfig::builder()
            .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
            .fields(vec![
                scalar_field("scalar_f"),       // FieldId(0), 10 * 1 = 10
                vector_field("vector_f", 3),    // FieldId(1), 10 * 3 = 30
            ])
            .propagators(vec![
                Box::new(ConstPropagator::new("write_scalar", FieldId(0), 1.0)),
            ])
            .dt(0.1)
            .seed(42)
            .build()
            .unwrap();
        let engine = TickEngine::new(config).unwrap();

        // ConstPropagator writes FieldId(0) which is scalar: expected 10.
        assert_eq!(engine.expectations.write[0], vec![(FieldId(0), 10)]);
    }
```

Note: This requires `expectations` to be accessible in tests. Since the
test module is `mod tests` inside `tick.rs`, it has access to private fields.

Run: `cargo test -p murk-engine field_expectations_computes`
Expected: PASS

- [ ] **Step 8: Run all engine tests**

Run: `cargo test -p murk-engine`
Expected: all tests pass (existing + new).

- [ ] **Step 9: Commit**

```
test(engine): add buffer validation regression tests

Verify validation passes for scalar, vector, multi-propagator, and
reads_previous configurations. Verify rollback path and
FieldExpectations construction.
```

---

## Task 4: Add `debug_assert_eq!` to All Space Implementations

**Files:**
- Modify: `crates/murk-space/src/line1d.rs:263,297`
- Modify: `crates/murk-space/src/ring1d.rs` (neighbours, distance methods)
- Modify: `crates/murk-space/src/square4.rs:121,146`
- Modify: `crates/murk-space/src/square8.rs` (neighbours, distance methods)
- Modify: `crates/murk-space/src/hex2d.rs` (neighbours, distance methods)
- Modify: `crates/murk-space/src/fcc12.rs` (neighbours, distance methods)
- Modify: `crates/murk-space/src/product.rs:346,377`

Mechanical insertions. Each `neighbours()` gets 1 assert, each `distance()`
gets 2 asserts. 7 impls x 3 asserts = 21 lines total.

- [ ] **Step 1: Line1D**

In `line1d.rs`, add at the top of `neighbours()` (line 263):
```rust
    fn neighbours(&self, coord: &Coord) -> SmallVec<[Coord; 8]> {
        debug_assert_eq!(
            coord.len(), self.ndim(),
            "coord arity {}, expected {}", coord.len(), self.ndim()
        );
        let i = coord[0];
        // ... rest unchanged
```

Add at the top of `distance()` (line 297):
```rust
    fn distance(&self, a: &Coord, b: &Coord) -> f64 {
        debug_assert_eq!(
            a.len(), self.ndim(),
            "coord a arity {}, expected {}", a.len(), self.ndim()
        );
        debug_assert_eq!(
            b.len(), self.ndim(),
            "coord b arity {}, expected {}", b.len(), self.ndim()
        );
        let ai = a[0];
        // ... rest unchanged
```

- [ ] **Step 2: Ring1D**

Same pattern as Line1D. Add at the top of `neighbours()` and `distance()`.
Ring1D delegates to `wrap_neighbours_1d(coord[0], self.len)` in neighbours
and directly indexes `a[0]`, `b[0]` in distance.

```rust
    fn neighbours(&self, coord: &Coord) -> SmallVec<[Coord; 8]> {
        debug_assert_eq!(
            coord.len(), self.ndim(),
            "coord arity {}, expected {}", coord.len(), self.ndim()
        );
        // ... existing: wrap_neighbours_1d(coord[0], self.len)
```

```rust
    fn distance(&self, a: &Coord, b: &Coord) -> f64 {
        debug_assert_eq!(
            a.len(), self.ndim(),
            "coord a arity {}, expected {}", a.len(), self.ndim()
        );
        debug_assert_eq!(
            b.len(), self.ndim(),
            "coord b arity {}, expected {}", b.len(), self.ndim()
        );
        // ... existing: wrap_distance_1d(a[0], b[0], self.len)
```

- [ ] **Step 3: Square4**

Add at the top of `neighbours()` (line 121) and `distance()` (line 146):

```rust
    fn neighbours(&self, coord: &Coord) -> SmallVec<[Coord; 8]> {
        debug_assert_eq!(
            coord.len(), self.ndim(),
            "coord arity {}, expected {}", coord.len(), self.ndim()
        );
        let r = coord[0];
        // ... rest unchanged
```

```rust
    fn distance(&self, a: &Coord, b: &Coord) -> f64 {
        debug_assert_eq!(
            a.len(), self.ndim(),
            "coord a arity {}, expected {}", a.len(), self.ndim()
        );
        debug_assert_eq!(
            b.len(), self.ndim(),
            "coord b arity {}, expected {}", b.len(), self.ndim()
        );
        let dr = grid2d::axis_distance(a[0], b[0], self.rows, self.edge);
        // ... rest unchanged
```

- [ ] **Step 4: Square8**

Same pattern as Square4. Add at the top of `neighbours()` and `distance()`.

- [ ] **Step 5: Hex2D**

Same pattern. Add at the top of `neighbours()` and `distance()`.

- [ ] **Step 6: Fcc12**

Same pattern. Fcc12 is 3D — `self.ndim()` returns 3.

- [ ] **Step 7: ProductSpace**

Same pattern. ProductSpace's `self.ndim()` returns `self.total_ndim` (the
sum of component ndims — verified by existing test at product.rs:731-733).

```rust
    fn neighbours(&self, coord: &Coord) -> SmallVec<[Coord; 8]> {
        debug_assert_eq!(
            coord.len(), self.ndim(),
            "coord arity {}, expected {}", coord.len(), self.ndim()
        );
        let parts: Vec<Coord> = (0..self.components.len())
        // ... rest unchanged
```

```rust
    fn distance(&self, a: &Coord, b: &Coord) -> f64 {
        debug_assert_eq!(
            a.len(), self.ndim(),
            "coord a arity {}, expected {}", a.len(), self.ndim()
        );
        debug_assert_eq!(
            b.len(), self.ndim(),
            "coord b arity {}, expected {}", b.len(), self.ndim()
        );
        (0..self.components.len())
        // ... rest unchanged
```

- [ ] **Step 8: Run all Space tests**

Run: `cargo test -p murk-space`
Expected: all tests pass. All existing tests use correctly-dimensioned coords,
so the debug_asserts never fire.

- [ ] **Step 9: Commit**

```
fix(space): add coord arity debug_assert to all Space impls

Adds debug_assert_eq!(coord.len(), self.ndim()) to neighbours() and
distance() in all 7 Space implementations. Zero cost in release builds.
Catches wrong-dimensioned coords during development/testing.
```

---

## Task 5: Compliance Suite Arity Tests

**Files:**
- Modify: `crates/murk-space/src/compliance.rs`

These are the primary structural enforcement for Part 2. Since all Space
impls call `run_full_compliance()`, adding tests here automatically covers
all existing and future impls.

- [ ] **Step 1: Add `assert_ndim_consistent`**

Add after the existing `assert_compile_region_all_covers_all` function
(around line 135):

```rust
/// Assert that `ndim()` matches the length of coords from `canonical_ordering()`.
pub fn assert_ndim_consistent(space: &dyn Space) {
    let ordering = space.canonical_ordering();
    assert!(!ordering.is_empty(), "canonical_ordering must be non-empty");
    for (i, coord) in ordering.iter().enumerate() {
        assert_eq!(
            coord.len(),
            space.ndim(),
            "coord at rank {} has length {}, but ndim() = {}",
            i,
            coord.len(),
            space.ndim(),
        );
    }
}
```

- [ ] **Step 2: Add `assert_neighbours_returns_valid_coords`**

```rust
/// Assert that all coords returned by `neighbours()` are valid (have a canonical rank).
pub fn assert_neighbours_returns_valid_coords(space: &dyn Space) {
    for coord in space.canonical_ordering() {
        for nb in space.neighbours(&coord) {
            assert!(
                space.canonical_rank(&nb).is_some(),
                "neighbours({:?}) returned {:?} which has no canonical rank",
                coord,
                nb,
            );
        }
    }
}
```

- [ ] **Step 3: Add arity rejection tests (debug-only)**

These use `catch_unwind` to verify that wrong-arity coords cause a panic
in debug mode. Gated with `#[cfg(debug_assertions)]` so they don't fail
under `cargo test --release`.

```rust
/// Assert that `neighbours()` panics on wrong-arity coord (debug builds only).
///
/// Gated with `#[cfg(debug_assertions)]` because `debug_assert!` is a no-op
/// in release mode — the panic would not fire and this test would fail.
#[cfg(debug_assertions)]
pub fn assert_neighbours_rejects_wrong_arity(space: &dyn Space) {
    use murk_core::Coord;
    use smallvec::smallvec;

    let wrong_coord: Coord = if space.ndim() == 1 {
        smallvec![0i32, 0i32] // 2D coord for a 1D space
    } else {
        smallvec![0i32] // 1D coord for a 2D+ space
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        space.neighbours(&wrong_coord)
    }));
    assert!(
        result.is_err(),
        "neighbours() with {}-arity coord should panic for {}-dim space",
        wrong_coord.len(),
        space.ndim(),
    );
}

/// Assert that `distance()` panics on wrong-arity coords (debug builds only).
#[cfg(debug_assertions)]
pub fn assert_distance_rejects_wrong_arity(space: &dyn Space) {
    use murk_core::Coord;
    use smallvec::smallvec;

    let wrong_coord: Coord = if space.ndim() == 1 {
        smallvec![0i32, 0i32]
    } else {
        smallvec![0i32]
    };
    let good_coord = &space.canonical_ordering()[0];

    // Wrong first arg.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        space.distance(&wrong_coord, good_coord)
    }));
    assert!(
        result.is_err(),
        "distance(wrong, good) should panic for wrong-arity coord",
    );

    // Wrong second arg.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        space.distance(good_coord, &wrong_coord)
    }));
    assert!(
        result.is_err(),
        "distance(good, wrong) should panic for wrong-arity coord",
    );
}
```

- [ ] **Step 4: Update `run_full_compliance`**

Update the doc comment and add the new checks:

```rust
/// Run all compliance checks on a space.
pub fn run_full_compliance(space: &dyn Space) {
    assert_distance_reflexive(space);
    assert_distance_symmetric(space);
    assert_distance_triangle_inequality(space);
    assert_neighbours_symmetric(space);
    assert_canonical_ordering_deterministic(space);
    assert_canonical_ordering_complete(space);
    assert_compile_region_all_valid_ratio(space);
    assert_compile_region_all_covers_all(space);
    assert_ndim_consistent(space);
    assert_neighbours_returns_valid_coords(space);
    #[cfg(debug_assertions)]
    {
        assert_neighbours_rejects_wrong_arity(space);
        assert_distance_rejects_wrong_arity(space);
    }
}
```

- [ ] **Step 5: Run all Space tests**

Run: `cargo test -p murk-space`
Expected: all tests pass. The new compliance checks run automatically for
every Space impl that calls `run_full_compliance()` (all 7 do).

- [ ] **Step 6: Verify under release mode**

Run: `cargo test -p murk-space --release` (or just `cargo check --release -p murk-space`)
Expected: compiles and passes. The `#[cfg(debug_assertions)]` blocks are
excluded, so the arity rejection tests don't exist in release mode.

- [ ] **Step 7: Commit**

```
test(space): add arity and ndim compliance tests

Adds assert_ndim_consistent and assert_neighbours_returns_valid_coords
to the compliance suite (always-on). Adds assert_neighbours_rejects_-
wrong_arity and assert_distance_rejects_wrong_arity (debug-only, using
catch_unwind). All tests are added to run_full_compliance() so every
Space impl is automatically covered.
```

---

## Task 6: Final Verification and Deferred Observation

**Files:** None (verification + tracking only)

- [ ] **Step 1: Run full workspace tests**

Run: `cargo test --workspace`
Expected: all tests pass across all crates.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace`
Expected: no new warnings from our changes.

- [ ] **Step 3: Log deferred observation for type-level coord arity**

Use the filigree MCP tool to record an observation about the deferred
type-level fix, so it doesn't get lost:

```
observe: "Space coord arity is validated via debug_assert! (zero-cost in release) + compliance suite. A higher-leverage fix would make invalid arity unrepresentable at the type level (e.g., TypedCoord<const N: usize> or a ValidatedCoord newtype). This is blocked by dyn Space object safety constraints. Tracked as deferred design decision, not forgotten."
file_path: crates/murk-space/src/space.rs
```

- [ ] **Step 4: Commit any remaining changes**

If clippy required fixes, commit them:

```
chore: address clippy warnings from systemic validation changes
```
