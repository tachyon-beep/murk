# Bug Report

**Date:** 2026-02-23
**Reporter:** static-analysis-agent
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [x] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [ ] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

Generic-path heat gradient uses raw wrapped coordinate deltas, producing incorrect gradient sign/magnitude at periodic boundaries on non-`Square4` spaces.

## Steps to Reproduce

1. Run `DiffusionPropagator` on a non-`Square4` wrap topology (e.g., `Ring1D`) so `step_generic` is selected.
2. Use heat `[0, 10, 20, 30]`, zero velocity, `diffusivity=0.0` (to isolate gradient), `dt=0.01`.
3. Inspect gradient at cell `0`: expected periodic central difference is `-10`, but computed value is `+10` (stored in component 1).

## Expected Behavior

Gradient near wrap boundaries should use signed minimal displacement across periodic edges (e.g., left neighbor of `0` is displacement `-1`, not `+3` on length-4 ring).

## Actual Behavior

`step_generic` computes displacement as raw coordinate subtraction (`nb - coord`), so wrapped neighbors produce large positive deltas (e.g., `3`), yielding wrong derivative sign/magnitude.

## Reproduction Rate

Always

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```text
N/A - found via static analysis
```

## Minimal Reproducer

```rust
#[test]
#[allow(deprecated)]
fn reproduces_wrap_gradient_sign_bug() {
    use murk_core::TickId;
    use murk_propagator::{context::StepContext, propagator::Propagator, scratch::ScratchRegion};
    use murk_propagators::{DiffusionPropagator, HEAT, HEAT_GRADIENT, VELOCITY};
    use murk_space::{Ring1D, Space};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    let space = Ring1D::new(4).unwrap();
    let n = space.cell_count();

    let mut reader = MockFieldReader::new();
    reader.set_field(HEAT, vec![0.0, 10.0, 20.0, 30.0]);
    reader.set_field(VELOCITY, vec![0.0; n * 2]);

    let mut writer = MockFieldWriter::new();
    writer.add_field(HEAT, n);
    writer.add_field(VELOCITY, n * 2);
    writer.add_field(HEAT_GRADIENT, n * 2);

    let mut scratch = ScratchRegion::new(0);
    let mut ctx = StepContext::new(
        &reader, &reader, &mut writer, &mut scratch, &space, TickId(1), 0.01
    );

    DiffusionPropagator::new(0.0).step(&mut ctx).unwrap();

    let grad = writer.get_field(HEAT_GRADIENT).unwrap();
    assert!((grad[1] - (-10.0)).abs() < 1e-6, "actual={}", grad[1]); // fails; actual is +10
}
```

## Additional Context

Evidence:
- `step()` dispatches to generic path for non-`Square4`: `/home/john/murk/crates/murk-propagators/src/diffusion.rs:318` and `/home/john/murk/crates/murk-propagators/src/diffusion.rs:326`
- Raw coordinate deltas are used directly: `/home/john/murk/crates/murk-propagators/src/diffusion.rs:179` and `/home/john/murk/crates/murk-propagators/src/diffusion.rs:180`
- Those deltas are used as derivative denominators: `/home/john/murk/crates/murk-propagators/src/diffusion.rs:242` and `/home/john/murk/crates/murk-propagators/src/diffusion.rs:246`
- Wrapped neighbor coordinates are represented as absolute wrapped indices (e.g., `left=((i-1)+n)%n`): `/home/john/murk/crates/murk-space/src/line1d.rs:89` and `/home/john/murk/crates/murk-space/src/line1d.rs:91`

Root cause hypothesis: periodic boundary displacements must be signed minimal offsets, but generic path uses absolute wrapped coordinates.

Suggested fix: use topology-aware signed displacement for wrapped axes (or restrict gradient computation to spaces where displacement semantics are explicit).

---

# Bug Report

**Date:** 2026-02-23
**Reporter:** static-analysis-agent
**Severity:** [x] Critical | [ ] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [x] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [ ] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`DiffusionPropagator` assumes fixed field arities (heat scalar, velocity vec2, gradient vec2) but performs unchecked indexing/copying, so arity mismatch causes runtime panic.

## Steps to Reproduce

1. Configure/read `VELOCITY` with length `cell_count` instead of `cell_count * 2`.
2. Run `DiffusionPropagator::step` on `Square4` (`1x1` is sufficient).
3. Execution panics on out-of-bounds access in velocity loop.

## Expected Behavior

Propagator should return `PropagatorError::ExecutionFailed` for incompatible field shapes instead of panicking.

## Actual Behavior

Unchecked indexing (`idx = i * 2 + comp`) and slice copy assumptions trigger panic (`index out of bounds` / `copy_from_slice` length mismatch), which can unwind through engine tick execution.

## Reproduction Rate

Always

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```text
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

Evidence:
- Fixed-arity indexing assumptions in velocity path: `/home/john/murk/crates/murk-propagators/src/diffusion.rs:103`, `/home/john/murk/crates/murk-propagators/src/diffusion.rs:105`, `/home/john/murk/crates/murk-propagators/src/diffusion.rs:108`, `/home/john/murk/crates/murk-propagators/src/diffusion.rs:110`
- Fixed-arity indexing assumptions in gradient outputs: `/home/john/murk/crates/murk-propagators/src/diffusion.rs:143` and `/home/john/murk/crates/murk-propagators/src/diffusion.rs:144`
- `copy_from_slice` without explicit length validation in generic path: `/home/john/murk/crates/murk-propagators/src/diffusion.rs:261`, `/home/john/murk/crates/murk-propagators/src/diffusion.rs:269`, `/home/john/murk/crates/murk-propagators/src/diffusion.rs:277`
- Startup pipeline validation checks field existence but not field component compatibility: `/home/john/murk/crates/murk-propagator/src/pipeline.rs:255` and `/home/john/murk/crates/murk-propagator/src/pipeline.rs:279`

Root cause hypothesis: `DiffusionPropagator` hardcodes per-cell component counts but never validates input/output slice lengths before indexing.

Suggested fix: preflight-check all required lengths (`heat == cells`, `velocity == cells*2`, `gradient == cells*2`) and return `PropagatorError::ExecutionFailed` instead of indexing/copying blindly.