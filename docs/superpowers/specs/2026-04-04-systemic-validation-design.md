# Systemic Validation: Buffer Lengths and Coord Arity

**Date:** 2026-04-04
**Status:** Reviewed (revised after 4-reviewer panel)
**Branch:** TBD (new branch from main)
**Version:** 0.1.9 (pre-1.0 — no backward compatibility constraints)

## Problem

Two systemic validation gaps affect the murk simulation engine:

1. **Buffer length validation in propagators.** 6 of 12 propagators lack length
   checks before `copy_from_slice` or direct indexing. This causes panics on
   misconfigured output buffers and silent data corruption on short input buffers
   (the morphological_op Erode bug: `all_present` stays `true` when missing cells
   are skipped, producing plausible-looking wrong results with no signal).

2. **Coord arity validation in Space impls.** All 7 Space implementations
   (`Line1D`, `Ring1D`, `Square4`, `Square8`, `Hex2D`, `Fcc12`, `ProductSpace`)
   lack arity validation in `neighbours()` and `distance()`. Passing a
   wrong-dimensioned `Coord` causes index-out-of-bounds panics or silent
   wrong results (ProductSpace).

Both gaps follow the same pattern: validation is present in *some* code paths
(e.g., `canonical_rank()` checks arity, 6 propagators check lengths) but absent
in others, creating an inconsistent safety contract.

## Design Principles

- **Centralized over decentralized.** The decentralized approach (each impl
  validates itself) has a 50% success rate in this codebase. Centralized
  validation has 100% success rate by construction.
- **Single source of truth.** Buffer sizes come from `FieldMeta` in the arena
  descriptor, not from propagator self-reports. No duplicate authorities.
- **Validate at boundaries, trust internally.** External data (Python/FFI)
  gets hard validation. Internal data (murk-to-murk) gets `debug_assert!`.
- **Existing per-propagator checks stay.** Defense-in-depth. But no effort to
  add them to the 6 propagators that lack them -- the engine is the contract.

## Part 1: Engine-Level Buffer Length Validation

### Data Source: `FieldMeta.total_len`

The arena's `FieldDescriptor` already computes and stores the correct buffer
length for every field at world construction time:

```
FieldMeta.total_len = cell_count * FieldType::components()
```

This is computed in `crates/murk-arena/src/descriptor.rs:70-80` and is the
same value used to allocate buffers. It is the single authoritative source for
expected buffer sizes. No new trait methods are needed on `Propagator`.

**Why not `field_components` on Propagator (rejected):** The initial design
proposed a `field_components(FieldId) -> usize` method on the Propagator trait.
Four-reviewer panel unanimously rejected this: it creates a second source of
truth that can diverge from `FieldDef`. If a maintainer updates the field
registration in `WorldConfig` but forgets to update the propagator override,
the validation checks the wrong expected length. Using `FieldMeta` directly
eliminates this risk entirely and requires zero propagator changes.

### Cached Field Expectations

The engine must not call `reads()`/`writes()` per-tick -- they return
heap-allocated `Vec`/`FieldSet` and the documented contract says they are
called once at startup, not per-tick.

At `TickEngine::new()`, after building the arena and `ReadResolutionPlan`,
build a cached validation table:

```rust
/// Per-propagator expected field lengths, computed once at construction.
struct FieldExpectations {
    /// For each propagator index: Vec of (FieldId, expected_len) for reads.
    read_expectations: Vec<Vec<(FieldId, usize)>>,
    /// For each propagator index: Vec of (FieldId, expected_len) for writes.
    write_expectations: Vec<Vec<(FieldId, usize)>>,
}
```

Built by iterating each propagator's `reads()`, `reads_previous()`, and
`writes()` once at construction, looking up `FieldMeta.total_len` from the
`FieldDescriptor` for each field.

The `ReadResolutionPlan` already caches `routes` (read fields per propagator)
and `write_modes` (write fields per propagator) at
`crates/murk-propagator/src/pipeline.rs:38-42`. The validation table can be
built alongside or derived from these.

### Engine Validation

In `crates/murk-engine/src/tick.rs`, in the propagator dispatch loop
(currently step 4, around line 325), **before** step 4e
(`StepContext::new()` + `prop.step()`), add a validation step:

```rust
// 4x. Validate field buffer lengths before dispatch.
for &(field_id, expected_len) in &self.expectations.read_expectations[i] {
    match overlay.read(field_id) {
        Some(buf) if buf.len() != expected_len => {
            return Err(/* PropagatorError::ExecutionFailed with details */);
        }
        None => {
            return Err(PropagatorError::ExecutionFailed {
                reason: format!(
                    "propagator '{}': declared read field {:?} not present",
                    prop.name(), field_id,
                ),
            });
        }
        _ => {} // present and correct length
    }
}

for &(field_id, expected_len) in &self.expectations.write_expectations[i] {
    match guard.writer.read(field_id) {
        Some(buf) if buf.len() != expected_len => {
            return Err(/* PropagatorError::ExecutionFailed with details */);
        }
        None => {
            return Err(PropagatorError::ExecutionFailed {
                reason: format!(
                    "propagator '{}': declared write field {:?} not present",
                    prop.name(), field_id,
                ),
            });
        }
        _ => {}
    }
}
```

**Missing fields fail early.** If a propagator declares it reads/writes a
field that isn't present, the engine returns `ExecutionFailed` immediately
rather than deferring to the propagator. This is consistent with the
centralized principle -- the engine guarantees the contract, not individual
propagators.

**Write-side buffer inspection.** The `WriteArena` guard already has
`fn read(&self, field: FieldId) -> Option<&[f32]>` (used for staged-cache
population at step 4a). This returns `&[f32]` via `&self`, avoiding the
`&mut self` borrow conflict with `write()`. No new trait methods are needed
on `FieldWriter`.

### Error Mode

`PropagatorError::ExecutionFailed` with a descriptive message naming the
propagator, field ID, actual length, and expected length. This integrates
with the engine's existing error handling and rollback path at line 388-398.

### What Happens to the 6 Unvalidated Propagators

Nothing. The engine check is the contract. The 5 propagators that already have
per-propagator checks (`MorphologicalOp`, `NoiseInjection`, `AgentEmission`,
`ResourceField`, `GradientCompute`) keep them as defense-in-depth. The 6 that
lack them (`IdentityCopy`, `FlowField`, `ScalarDiffusion`, `WavePropagation`,
`Reward`, `Diffusion`) do not get individual checks added -- that's the
decentralized approach we're replacing.

## Part 2: Space Coord Arity Validation

### Primary Enforcement: Compliance Suite

The compliance suite (`crates/murk-space/src/compliance.rs`) is the structural
guarantee for arity validation. All 7 existing Space impls call
`run_full_compliance()` in their test modules. Adding wrong-arity tests to
`run_full_compliance()` makes them automatically enforced for any future impl
that follows the pattern.

Add to `run_full_compliance()`:

```rust
#[cfg(debug_assertions)]
pub fn assert_neighbours_rejects_wrong_arity(space: &dyn Space) {
    let wrong_coord: Coord = if space.ndim() == 1 {
        smallvec![0i32, 0i32]  // 2D coord for 1D space
    } else {
        smallvec![0i32]  // 1D coord for 2D+ space
    };
    let result = std::panic::catch_unwind(
        std::panic::AssertUnwindSafe(|| space.neighbours(&wrong_coord))
    );
    assert!(result.is_err(),
        "neighbours() with wrong-arity coord should panic in debug mode");
}

#[cfg(debug_assertions)]
pub fn assert_distance_rejects_wrong_arity(space: &dyn Space) {
    // Similar pattern for distance()
}
```

Additional compliance tests to add while here:
- `assert_ndim_consistent`: Verify `ndim()` matches coord length from
  `canonical_ordering()`. Directly validates the `debug_assert_eq!` contract.
- `assert_neighbours_returns_valid_coords`: Verify all coords returned by
  `neighbours()` have `canonical_rank() == Some(...)`.

### Secondary: `debug_assert!` in Hot Path

Add `debug_assert_eq!` at the top of every `neighbours()` and `distance()`
impl. These are defense-in-depth -- the compliance suite is primary.

```rust
fn neighbours(&self, coord: &Coord) -> SmallVec<[Coord; 8]> {
    debug_assert_eq!(coord.len(), self.ndim(),
        "coord arity {}, expected {}", coord.len(), self.ndim());
    // ... existing implementation
}

fn distance(&self, a: &Coord, b: &Coord) -> f64 {
    debug_assert_eq!(a.len(), self.ndim(),
        "coord a arity {}, expected {}", a.len(), self.ndim());
    debug_assert_eq!(b.len(), self.ndim(),
        "coord b arity {}, expected {}", b.len(), self.ndim());
    // ... existing implementation
}
```

**Scope:** 7 impls x 2 methods = 14 insertions (with `distance()` getting 2
assertions each = 21 total `debug_assert_eq!` lines).

**ProductSpace note:** `ProductSpace::ndim()` returns `self.total_ndim`, which
is the sum of component ndims (verified by existing test at
`product.rs:731-733`: `Hex2D(2) + Line1D(1) = 3`). The
`debug_assert_eq!(coord.len(), self.ndim())` check works correctly for
ProductSpace without special handling.

**Cost:** Zero in release builds. In debug builds, one `len()` comparison per
call -- negligible compared to the BFS/distance computation itself.

### External: Hard Validation at FFI/Python Boundary

Wherever Python or game logic passes a `Coord` into murk, validate arity and
return an error (not panic) on mismatch. This is the only entry point for
externally-constructed coordinates.

**Scope:** Identify all FFI/Python functions that accept `Coord` arguments and
add arity checks that return `PyErr` / error codes before forwarding to Space
methods. The exploration found no direct `neighbours()`/`distance()` exposure
in FFI today, but this should be verified during implementation -- if those
methods are exposed indirectly (e.g., through a wrapper), the wrapper needs
the check.

### Deferred: Type-Level Coord Arity

A higher-leverage intervention would make invalid arity unrepresentable at the
type level (e.g., `TypedCoord<const N: usize>` or a `ValidatedCoord` newtype).
This is blocked by `dyn Space` object safety constraints -- const generics
cannot appear in object-safe trait methods. This is explicitly deferred, not
forgotten. Log a filigree observation to track it.

## Testing

### Part 1 Tests

**Engine-level validation tests** (in `tick.rs` test module):

1. **Mismatched output buffer length.** Register a propagator with a write
   field whose buffer is the wrong size. Verify `execute_tick()` returns
   `PropagatorError::ExecutionFailed` with a message containing field ID and
   length details.

2. **Short input buffer.** Register a propagator whose read field buffer is
   shorter than `cell_count * components`. Verify same.

3. **Missing declared field.** Register a propagator that declares a read
   field not present in the arena. Verify `ExecutionFailed` with "not present".

4. **Step-not-called guarantee.** Use a propagator with an `AtomicUsize`
   counter incremented in `step()`. After a buffer-length rejection, verify
   the counter is 0 -- confirming the engine rejected before dispatch.
   (The existing `FailingPropagator` in test-utils uses this pattern.)

5. **`reads_previous` vs `reads` path.** Separate tests for mismatched
   buffers in each slot, since they come from different sources (base
   snapshot vs staged cache).

6. **Vector-field positive case.** A propagator writing a 2-component field
   with a correctly sized buffer (`cell_count * 2`) passes through
   `execute_tick()` without error.

7. **Off-by-component mismatch.** A buffer of `cell_count` (scalar) for a
   field registered as `Vector { dims: 2 }` (expects `cell_count * 2`).
   Verify rejection.

8. **Rollback after pre-dispatch failure.** If the second propagator in a
   two-propagator pipeline fails the buffer check, verify rollback behavior
   matches the existing `handle_rollback` path (check
   `consecutive_rollback_count`, `tick_disabled` after max rollbacks).

9. **Regression.** All existing propagator and engine tests continue to pass.

**Error assertion pattern:** Match on the enum variant and check the message:
```rust
match err {
    PropagatorError::ExecutionFailed { reason } => {
        assert!(reason.contains("length"), "error should mention length: {reason}");
    }
    other => panic!("expected ExecutionFailed, got {other:?}"),
}
```

### Part 2 Tests

**Compliance suite additions** (in `compliance.rs`):

1. `assert_neighbours_rejects_wrong_arity` -- gated with
   `#[cfg(debug_assertions)]`, uses `catch_unwind` to verify panic.
2. `assert_distance_rejects_wrong_arity` -- same pattern.
3. `assert_ndim_consistent` -- verify `ndim()` matches coord lengths from
   `canonical_ordering()`.
4. `assert_neighbours_returns_valid_coords` -- all returned coords have
   `canonical_rank() == Some(...)`.

Added to `run_full_compliance()` so all existing and future Space impls
automatically run them.

**ProductSpace-specific test:** Verify that passing a coord with arity
matching one sub-space but not the total (e.g., 2D coord for a
`Hex2D + Line1D` product with ndim=3) triggers the assertion.

**Test gating:** All `debug_assert!`-dependent tests MUST be gated with
`#[cfg(debug_assertions)]` to prevent failures under `cargo test --release`.
Use `#[should_panic(expected = "coord arity")]` with the `expected` substring
where `#[should_panic]` is used (prevents false passes from unrelated panics).

## Scope and Non-Goals

- **In scope:** Engine-level buffer validation using `FieldMeta`, cached field
  expectations at construction, Space arity `debug_assert!`s, compliance suite
  extension, FFI boundary validation.
- **Not in scope:** Adding per-propagator validation to the 6 that lack it.
  New trait methods on `Propagator` or `FieldWriter`. Runtime (release-mode)
  validation in Space hot paths. Type-level coord arity (deferred).

## Files Modified

| File | Change |
|---|---|
| `crates/murk-engine/src/tick.rs` | Add `FieldExpectations`, build at construction, validate before dispatch |
| `crates/murk-space/src/line1d.rs` | `debug_assert_eq!` in neighbours/distance |
| `crates/murk-space/src/ring1d.rs` | `debug_assert_eq!` in neighbours/distance |
| `crates/murk-space/src/square4.rs` | `debug_assert_eq!` in neighbours/distance |
| `crates/murk-space/src/square8.rs` | `debug_assert_eq!` in neighbours/distance |
| `crates/murk-space/src/hex2d.rs` | `debug_assert_eq!` in neighbours/distance |
| `crates/murk-space/src/fcc12.rs` | `debug_assert_eq!` in neighbours/distance |
| `crates/murk-space/src/product.rs` | `debug_assert_eq!` in neighbours/distance |
| `crates/murk-space/src/compliance.rs` | Wrong-arity + ndim-consistency compliance tests |
| `crates/murk-engine/src/tick.rs` (tests) | 9 engine validation tests |

## Review Panel Findings Incorporated

| Finding | Source | Resolution |
|---|---|---|
| `field_components` duplicates `FieldType::components()` | Rust engineer, Architect | Removed; use `FieldMeta.total_len` |
| `buf_len` on FieldWriter unnecessary | Architect | Removed; use `guard.writer.read()` for write-side inspection |
| `reads()`/`writes()` allocate per-tick | Rust engineer | Cache at construction in `FieldExpectations` |
| Missing fields should fail early | Architect | Engine returns `ExecutionFailed` on missing declared fields |
| Compliance suite is primary for Part 2 | Systems thinker | Repositioned: compliance = primary, debug_assert = secondary |
| `#[should_panic]` fails under `--release` | Quality engineer | Gate with `#[cfg(debug_assertions)]` |
| Step-not-called contract untested | Quality engineer | Added AtomicUsize side-effect test |
| Type-level coord fix deferred | Systems thinker | Logged as explicit deferred decision |
| `ProductSpace::ndim()` returns sum | Rust engineer | Verified: existing test confirms `Hex2D(2) + Line1D(1) = 3` |
| `PropagatorError::ExecutionFailed` exists | Rust engineer | Verified: `error.rs:68-71` |
