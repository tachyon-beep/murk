# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent (wave-5)
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-propagators

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`DiffusionPropagator` assumes fixed field arities (heat: scalar, velocity: vec2, gradient: vec2) but never validates input/output slice lengths before indexing, causing a panic on arity mismatch instead of a graceful error.

## Steps to Reproduce

1. Configure/read `VELOCITY` with length `cell_count` instead of the expected `cell_count * 2`.
2. Run `DiffusionPropagator::step` on `Square4` (even a 1x1 grid is sufficient).
3. Execution panics on out-of-bounds access in the velocity loop.

## Expected Behavior

`step()` should return `Err(PropagatorError::ExecutionFailed { ... })` for any incompatible field shape, instead of panicking.

## Actual Behavior

Unchecked indexing (`idx = i * 2 + comp`) and slice copy assumptions trigger a panic (`index out of bounds` or `copy_from_slice` length mismatch), which can unwind through engine tick execution.

Evidence in `crates/murk-propagators/src/diffusion.rs`:

**Square4 path:**
- Line 103: `let idx = i * 2 + comp;` -- unchecked
- Line 105: `vel_prev[ni * 2 + comp]` -- unchecked
- Line 108: `vel_out[idx]` -- unchecked
- Line 110: `vel_out[idx]` -- unchecked
- Line 143: `grad_out[i * 2]` -- unchecked
- Line 144: `grad_out[i * 2 + 1]` -- unchecked

**Generic path:**
- Line 223: `let idx = i * 2 + comp;` -- unchecked
- Line 225: `vel_prev[r * 2 + comp]` -- unchecked
- Lines 261, 269, 277: `copy_from_slice` without prior length validation

The startup pipeline validation (`murk-propagator/src/pipeline.rs`) checks field existence but not field component count compatibility.

## Reproduction Rate

Always (given mismatched field arity).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** 0.1.8 / HEAD

## Determinism Impact

- [x] Bug is deterministic
- [ ] Bug is non-deterministic
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
#[test]
#[should_panic]
#[allow(deprecated)]
fn reproduces_velocity_arity_panic() {
    use murk_core::TickId;
    use murk_propagator::{context::StepContext, propagator::Propagator, scratch::ScratchRegion};
    use murk_propagators::{DiffusionPropagator, HEAT, HEAT_GRADIENT, VELOCITY};
    use murk_space::{EdgeBehavior, Space, Square4};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    let grid = Square4::new(1, 1, EdgeBehavior::Absorb).unwrap();
    let n = grid.cell_count();

    let mut reader = MockFieldReader::new();
    reader.set_field(HEAT, vec![1.0; n]);
    reader.set_field(VELOCITY, vec![0.0; n]); // wrong arity: should be n * 2

    let mut writer = MockFieldWriter::new();
    writer.add_field(HEAT, n);
    writer.add_field(VELOCITY, n * 2);
    writer.add_field(HEAT_GRADIENT, n * 2);

    let mut scratch = ScratchRegion::new(0);
    let mut ctx = StepContext::new(
        &reader, &reader, &mut writer, &mut scratch, &grid, TickId(1), 0.01
    );

    DiffusionPropagator::new(0.1).step(&mut ctx).unwrap(); // panics
}
```

## Additional Context

**Root cause:** `DiffusionPropagator` hardcodes per-cell component counts (heat=1, velocity=2, gradient=2) but never validates that the actual slice lengths match these assumptions before performing indexed access.

**Suggested fix:** Add preflight length checks at the start of both `step_square4` and `step_generic`:
```rust
let cells = rows as usize * cols as usize;
if heat_prev.len() != cells {
    return Err(PropagatorError::ExecutionFailed { reason: "..." });
}
if vel_prev.len() != cells * 2 {
    return Err(PropagatorError::ExecutionFailed { reason: "..." });
}
// ... same for output buffers
```

This converts a panic into a recoverable error, consistent with the defensive patterns used in other propagators.

(Source report: `docs/bugs/generated/crates/murk-propagators/src/diffusion.rs.md`)
