# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent (wave-5)
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-propagators

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`DiffusionPropagator::step_generic` computes heat gradient using raw coordinate deltas from `Space::neighbours()`, producing wrong gradient sign and magnitude at periodic (wrap) boundaries on non-`Square4` spaces.

## Steps to Reproduce

1. Run `DiffusionPropagator` on a non-`Square4` wrap topology (e.g. `Ring1D` with 4 cells) so `step_generic` is selected.
2. Set heat to `[0, 10, 20, 30]`, zero velocity, `diffusivity=0.0` (to isolate gradient), `dt=0.01`.
3. Inspect gradient at cell 0: the periodic central difference should yield a negative x-gradient (left neighbor via wrap has higher heat), but the actual value has the wrong sign.

## Expected Behavior

Gradient at wrap boundaries should use signed minimal displacement across periodic edges. For cell 0 on a length-4 ring, the left neighbor (cell 3, heat=30) should contribute a displacement of `-1`, giving `dh/dx = (30-0)/(-1) = -30` from that direction, which averages with the right neighbor contribution to produce a negative gradient.

## Actual Behavior

`step_generic` computes displacement as raw coordinate subtraction (`nb[0] - coord[0]`), so the wrapped neighbor at coordinate 3 relative to coordinate 0 produces a delta of `+3` instead of `-1`. This yields `dh/dx = (30-0)/(+3) = +10` -- wrong sign and magnitude.

Evidence in `crates/murk-propagators/src/diffusion.rs`:
- Line 179: `let dc = if nb.len() >= 2 { nb[1] - coord[1] } else { 0 };` -- raw delta
- Line 180: `let dr = nb[0] - coord[0];` -- raw delta
- Lines 239-248: These raw deltas are used as gradient denominators (`dh / dc`, `dh / dr`)
- The `Square4` fast path (lines 126-144) uses `resolve_axis()` which handles wrapping correctly, so this bug only affects the generic path used by `Ring1D`, `Line1D(Wrap)`, `Hex2D(Wrap)`, `Fcc12(Wrap)`, etc.

## Reproduction Rate

Always (on any non-Square4 space with periodic boundaries).

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
    // Expected: negative gradient at cell 0 (heat increases to the left via wrap)
    assert!((grad[1] - (-10.0)).abs() < 1e-6, "actual={}", grad[1]); // fails
}
```

## Additional Context

The `Square4` fast path at lines 126-144 uses `resolve_axis()` which correctly handles periodic wrapping. The generic path at lines 171-186 does not have equivalent logic -- it uses raw absolute coordinates from `Space::neighbours()`, which for wrapped spaces represent the canonical coordinate on the other side of the boundary, not the minimal signed displacement.

**Root cause:** Periodic boundary displacements must be signed minimal offsets (e.g. `-1` for wrapping from coordinate 0 to coordinate `N-1`), but the generic path uses absolute wrapped coordinates as deltas.

**Suggested fix:** Use topology-aware signed displacement for wrapped axes. Either add a `Space::signed_displacement(from, to) -> Vec<i32>` method, or compute the minimal signed offset inline: `let d = nb[k] - coord[k]; if d > dim/2 { d - dim } else if d < -dim/2 { d + dim } else { d }`.

(Source report: `docs/bugs/generated/crates/murk-propagators/src/diffusion.rs.md`)
