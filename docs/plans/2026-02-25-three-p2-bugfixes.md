# Three P2 Bugfix Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix three P2 bugs: unchecked f64→i32 cast in FFI parse_space, unchecked field arity in DiffusionPropagator, and redundant trait method calls in validate_pipeline.

**Architecture:** Each bug is independent. All are input validation / startup-time fixes with no runtime performance impact. TDD approach: write failing test first, then fix.

**Tech Stack:** Rust, murk-ffi, murk-propagators, murk-propagator

**Filigree IDs:** filigree-be3192, filigree-bab4b3, filigree-ea447d

---

## Task 1: `parse_space` unchecked `as i32` cast (filigree-be3192)

**Problem:** `crates/murk-ffi/src/config.rs` uses `p[x] as i32` for edge behavior and space type params. NaN becomes 0 (Absorb), 1.9 truncates to 1 (Clamp). Dimension params use validated `f64_to_u32` but enum params don't.

**Files:**
- Modify: `crates/murk-ffi/src/config.rs`

**Step 1: Write failing tests**

Add these tests to the existing `mod tests` block at the bottom of `config.rs`:

```rust
#[test]
fn nan_edge_behavior_returns_invalid_argument() {
    let mut h: u64 = 0;
    murk_config_create(&mut h);
    let params = [10.0f64, f64::NAN]; // Line1D, edge=NaN
    assert_eq!(
        murk_config_set_space(h, MurkSpaceType::Line1D as i32, params.as_ptr(), 2),
        MurkStatus::InvalidArgument as i32
    );
    murk_config_destroy(h);
}

#[test]
fn fractional_edge_behavior_returns_invalid_argument() {
    let mut h: u64 = 0;
    murk_config_create(&mut h);
    let params = [5.0f64, 5.0, 1.9]; // Square4, edge=1.9 (should not truncate to 1)
    assert_eq!(
        murk_config_set_space(h, MurkSpaceType::Square4 as i32, params.as_ptr(), 3),
        MurkStatus::InvalidArgument as i32
    );
    murk_config_destroy(h);
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p murk-ffi --lib -- config::tests::nan_edge config::tests::fractional_edge -v`
Expected: Both FAIL (NaN → 0 → Absorb accepted, 1.9 → 1 → Clamp accepted)

**Step 3: Add `f64_to_i32` helper and replace unchecked casts**

Add helper after `f64_to_usize` (~line 98):

```rust
/// Safely convert an f64 FFI parameter to i32.
/// Rejects non-finite, non-integer, and out-of-range values.
fn f64_to_i32(v: f64) -> Option<i32> {
    if !v.is_finite() || v > i32::MAX as f64 || v < i32::MIN as f64 || v != v.trunc() {
        return None;
    }
    Some(v as i32)
}
```

Replace these unchecked casts:
- Line 110: `parse_edge_behavior(p[1] as i32)?` → `parse_edge_behavior(f64_to_i32(p[1])?)?`
- Line 128: `parse_edge_behavior(p[2] as i32)?` → `parse_edge_behavior(f64_to_i32(p[2])?)?`
- Line 139: `parse_edge_behavior(p[2] as i32)?` → `parse_edge_behavior(f64_to_i32(p[2])?)?`
- Line 163: `parse_edge_behavior(p[3] as i32)?` → `parse_edge_behavior(f64_to_i32(p[3])?)?`
- Line 183: `let comp_type = p[offset] as i32;` → `let comp_type = f64_to_i32(p[offset])?;`

Update `parse_edge_behavior` signature to accept i32 directly (no change needed — it already does).

**Step 4: Run tests to verify they pass**

Run: `cargo test -p murk-ffi --lib -- config -v`
Expected: All 19 tests pass (17 existing + 2 new)

**Step 5: Commit**

```
fix: validate f64-to-i32 casts in FFI parse_space (filigree-be3192)
```

---

## Task 2: DiffusionPropagator unchecked field arity (filigree-bab4b3)

**Problem:** `crates/murk-propagators/src/diffusion.rs` assumes heat=scalar, velocity=vec2, gradient=vec2 but never validates slice lengths. Wrong arity → index-out-of-bounds panic instead of PropagatorError.

**Files:**
- Modify: `crates/murk-propagators/src/diffusion.rs`

**Step 1: Write failing test**

Add to the `mod tests` block at the bottom of `diffusion.rs`:

```rust
#[test]
fn wrong_velocity_arity_returns_error() {
    // Velocity should be vec2 (2 * cell_count) but we provide scalar (1 * cell_count).
    let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
    let n = grid.cell_count();
    let prop = DiffusionPropagator::new(0.1);

    let mut reader = MockFieldReader::new();
    reader.set_field(HEAT, vec![0.0; n]);
    reader.set_field(VELOCITY, vec![0.0; n]); // Wrong: should be n*2

    let mut writer = MockFieldWriter::new();
    writer.add_field(HEAT, n);
    writer.add_field(VELOCITY, n); // Wrong arity
    writer.add_field(HEAT_GRADIENT, n * 2);

    let mut scratch = ScratchRegion::new(0);
    let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

    let result = prop.step(&mut ctx);
    assert!(result.is_err(), "expected PropagatorError for wrong velocity arity, got Ok");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p murk-propagators --lib -- diffusion::tests::wrong_velocity_arity -v`
Expected: PANIC (index out of bounds), not a clean error

**Step 3: Add preflight length checks**

Add a helper function before the `impl Propagator for DiffusionPropagator` block:

```rust
/// Validate that field slices have the expected lengths for this propagator.
///
/// Returns `Ok(cell_count)` or a descriptive `PropagatorError`.
fn check_field_arity(
    heat: &[f32],
    velocity: &[f32],
    cell_count: usize,
) -> Result<(), PropagatorError> {
    if heat.len() != cell_count {
        return Err(PropagatorError::ExecutionFailed {
            reason: format!(
                "heat field length mismatch: expected {cell_count}, got {}",
                heat.len()
            ),
        });
    }
    if velocity.len() != cell_count * 2 {
        return Err(PropagatorError::ExecutionFailed {
            reason: format!(
                "velocity field length mismatch: expected {} (vec2), got {}",
                cell_count * 2,
                velocity.len()
            ),
        });
    }
    Ok(())
}
```

Insert the check at the top of `step_square4` (after reading both fields, before any indexing):

```rust
let cell_count = (rows * cols) as usize;
check_field_arity(&heat_prev, &vel_prev, cell_count)?;
```

Insert the check at the top of `step_generic` (after reading both fields, before any indexing):

```rust
check_field_arity(&heat_prev, &vel_prev, cell_count)?;
```

**Step 4: Run tests to verify they pass**

Run: `cargo test -p murk-propagators --lib -- diffusion -v`
Expected: All tests pass (existing + new), no panics

**Step 5: Commit**

```
fix: validate field arity in DiffusionPropagator before indexing (filigree-bab4b3)
```

---

## Task 3: validate_pipeline calls trait methods multiple times (filigree-ea447d)

**Problem:** `crates/murk-propagator/src/pipeline.rs` calls `writes()` 3×, `reads()` 2×, `reads_previous()` 1× per propagator across validation passes. With interior mutability, could return inconsistent declarations.

**Files:**
- Modify: `crates/murk-propagator/src/pipeline.rs`

**Step 1: Write failing test**

Add a test propagator that counts calls and a test that asserts single-call semantics:

```rust
#[test]
fn trait_methods_called_once_per_propagator() {
    use std::sync::atomic::{AtomicU32, Ordering};

    static READS_CALLS: AtomicU32 = AtomicU32::new(0);
    static WRITES_CALLS: AtomicU32 = AtomicU32::new(0);
    static READS_PREV_CALLS: AtomicU32 = AtomicU32::new(0);

    struct CountingProp;
    impl Propagator for CountingProp {
        fn name(&self) -> &str { "counting" }
        fn reads(&self) -> FieldSet {
            READS_CALLS.fetch_add(1, Ordering::Relaxed);
            [FieldId(0)].into_iter().collect()
        }
        fn reads_previous(&self) -> FieldSet {
            READS_PREV_CALLS.fetch_add(1, Ordering::Relaxed);
            FieldSet::empty()
        }
        fn writes(&self) -> Vec<(FieldId, WriteMode)> {
            WRITES_CALLS.fetch_add(1, Ordering::Relaxed);
            vec![(FieldId(1), WriteMode::Full)]
        }
        fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
            Ok(())
        }
    }

    READS_CALLS.store(0, Ordering::Relaxed);
    WRITES_CALLS.store(0, Ordering::Relaxed);
    READS_PREV_CALLS.store(0, Ordering::Relaxed);

    let props: Vec<Box<dyn Propagator>> = vec![Box::new(CountingProp)];
    let fields = [FieldId(0), FieldId(1)].into_iter().collect();
    let _ = validate_pipeline(&props, &fields, 0.1, &*test_space());

    assert_eq!(READS_CALLS.load(Ordering::Relaxed), 1, "reads() called more than once");
    assert_eq!(WRITES_CALLS.load(Ordering::Relaxed), 1, "writes() called more than once");
    assert_eq!(READS_PREV_CALLS.load(Ordering::Relaxed), 1, "reads_previous() called more than once");
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p murk-propagator --lib -- pipeline::tests::trait_methods_called_once -v`
Expected: FAIL — reads=2, writes=3, reads_previous=1

**Step 3: Snapshot declarations once into PropMeta**

At the top of `validate_pipeline`, after the empty-pipeline check, snapshot all declarations:

```rust
struct PropMeta {
    name: String,
    reads: FieldSet,
    reads_previous: FieldSet,
    writes: Vec<(FieldId, WriteMode)>,
    max_dt: Option<f64>,
}

let metas: Vec<PropMeta> = propagators
    .iter()
    .map(|p| PropMeta {
        name: p.name().to_string(),
        reads: p.reads(),
        reads_previous: p.reads_previous(),
        writes: p.writes(),
        max_dt: p.max_dt(space),
    })
    .collect();
```

Then rewrite all validation passes to use `metas[i]` instead of calling trait methods. Replace:
- Step 2 (write conflicts): `prop.writes()` → `meta.writes`
- Step 3 (field refs): `prop.reads()`, `prop.reads_previous()`, `prop.writes()` → `meta.reads`, `meta.reads_previous`, `meta.writes`
- Step 4 (dt validation): `prop.max_dt(space)` → `meta.max_dt`, `prop.name()` → `meta.name`
- Step 5 (build plan): `prop.reads()`, `prop.writes()` → `meta.reads`, `meta.writes`

**Step 4: Run tests to verify they pass**

Run: `cargo test -p murk-propagator --lib -- pipeline -v`
Expected: All 26 tests pass (25 existing + 1 new)

**Step 5: Commit**

```
fix: snapshot propagator declarations once in validate_pipeline (filigree-ea447d)
```

---

## Task 4: Final verification

**Step 1: Run full workspace tests**

Run: `cargo test --workspace`
Expected: All tests pass

**Step 2: Run all examples**

Run each of the 4 Rust examples to verify no regressions.

**Step 3: Close filigree issues**

```
filigree close filigree-be3192
filigree close filigree-bab4b3
filigree close filigree-ea447d
```
