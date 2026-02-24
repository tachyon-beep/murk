# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
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

- [x] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`DiffusionPropagator::max_dt()` returns `1.0 / (4.0 * self.diffusivity)`, hardcoding the CFL stability bound for a 4-neighbor Square4 stencil. However, `step_generic()` (used for all non-Square4 spaces) computes `alpha = diffusivity * dt * count` where `count` is the actual neighbor degree, which can be 6 (Hex2D) or 12 (Fcc12). This means pipeline validation approves timesteps that cause numerical instability on higher-degree spaces.

Concrete failure: with `diffusivity = 1.0`, `max_dt()` returns `0.25`, and `dt = 0.24` passes validation. On a Hex2D interior cell with 6 neighbors, `alpha = 1.0 * 0.24 * 6 = 1.44`, yielding `(1 - alpha) = -0.44` as the self-weight. This produces negative heat values -- a non-physical, unstable update.

## Steps to Reproduce

1. Create a `DiffusionPropagator` with `diffusivity = 1.0`.
2. Create a `Hex2D` space (6 neighbors per interior cell).
3. Set `dt = 0.24` (passes `max_dt()` check since `0.24 < 0.25`).
4. Place a hot spot in the center cell (e.g., `heat = 100.0`).
5. Run one tick.
6. Observe that the center cell's heat goes negative (unstable update).

## Expected Behavior

`max_dt()` should return a stability bound that is safe for ALL supported spaces, or the bound should depend on the space's maximum neighbor degree. For Hex2D: `1 / (6 * D)`. For Fcc12: `1 / (12 * D)`.

## Actual Behavior

`max_dt()` always returns `1 / (4 * D)` regardless of the space topology. `step_generic()` then uses the actual neighbor count, which can exceed 4, producing `alpha > 1.0` and causing negative self-weights in the diffusion update formula `heat_new = (1 - alpha) * heat_prev + alpha * mean_neighbors`.

## Reproduction Rate

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_propagators::DiffusionPropagator;
use murk_propagator::propagator::Propagator;

let prop = DiffusionPropagator::new(1.0);
assert_eq!(prop.max_dt(), Some(0.25)); // 1/(4*1.0) -- assumes 4 neighbors

// On Hex2D (6 neighbors), dt=0.24 passes validation but produces:
// alpha = 1.0 * 0.24 * 6 = 1.44
// self_weight = 1 - 1.44 = -0.44  <-- UNSTABLE
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-propagators/src/diffusion.rs.md
**Verified lines:** diffusion.rs:328-330 (hardcoded max_dt), diffusion.rs:236-241 and 248-253 (alpha uses actual count), hex2d.rs:10-17 (6 neighbors), fcc12.rs:38-51 (12 neighbors)
**Root cause:** `max_dt()` is derived from Square4's 4-neighbor stencil and reused globally, but `step_generic()` scales by the actual topology-dependent neighbor count. The stability contract is space-dependent while the guard is space-agnostic.
**Suggested fix:**
1. Option A: Make `max_dt()` conservative for the worst-case supported space. Use `1 / (max_degree * D)` where `max_degree = 12` (Fcc12).
2. Option B: Extend the `Propagator` trait so `max_dt()` can depend on the `Space` (breaking change).
3. Option C: Add a runtime guard in `step_generic()` that clamps `alpha` to `<= 1.0` per cell and logs a warning.
