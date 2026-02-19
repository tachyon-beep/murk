# Feature Proposal: Standard Propagator Library

**Date:** 2026-02-19
**Reporter:** design-review
**Priority:** P3 (enhancement, not blocking release)

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
- [x] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Summary

Refactor `murk-propagators` from a hardcoded reference pipeline into a
composable library of field-parameterized propagators exposed through the
Python bindings. 95% of users should be able to build a complete world
simulation by picking propagators off the shelf and wiring field IDs.
Power users retain the full `Propagator` trait for custom logic.

## Motivation

All three Python examples (`heat_seeker`, `crystal_nav`, `hex_pursuit`)
reimplement the same physics in Python:

| Pattern               | heat_seeker | crystal_nav | hex_pursuit |
|-----------------------|:-----------:|:-----------:|:-----------:|
| Laplacian diffusion   | 30 lines    | 60 lines (x2 fields) | -- |
| Fixed source injection| Yes         | Yes (x2)    | --          |
| Exponential decay     | Yes         | Yes         | --          |
| Clamp-to-zero         | Yes         | Yes         | --          |
| Identity copy-forward | --          | --          | Yes         |

These Python stencils are:
- **The performance bottleneck** (O(cells x neighbors) per tick, 10-100x slower than Rust depending on grid size)
- **Mathematically identical** across examples (only parameters differ)
- **Easy to get wrong** (CFL stability, boundary handling, off-by-one strides)
- **Boilerplate** that obscures the actual environment design

Meanwhile, the Rust `DiffusionPropagator` already implements correct
Jacobi diffusion with CFL checking, boundary handling, and gradient
computation -- but it's hardcoded to `HEAT`/`VELOCITY`/`HEAT_GRADIENT`
field IDs, making it unusable outside the bench harness.

## Proposal

### 1. Parameterize existing propagators on FieldId

Replace hardcoded field constants with constructor parameters:

```rust
// Current (hardcoded):
impl Propagator for DiffusionPropagator {
    fn reads_previous(&self) -> FieldSet { [HEAT, VELOCITY].into() }
}

// Proposed (parameterized):
let diffusion = ScalarDiffusion::builder()
    .input_field(my_heat_field)
    .output_field(my_heat_field)     // same FieldId: reads previous tick, writes full this tick
    .gradient_field(my_gradient)     // optional: also compute gradient
    .coefficient(0.1)
    .decay(0.01)
    .sources(vec![(cell_42, 10.0f32)])  // fixed-value cells (f32 matches field storage)
    .clamp_min(0.0)
    .build();
```

### 2. Expose through Python bindings

```python
config.add_propagator(murk.ScalarDiffusion(
    field="heat",
    coefficient=0.1,
    decay=0.01,
    sources=[(source_cell, 10.0)],
    clamp_min=0.0,
))
config.add_propagator(murk.IdentityCopy(field="agent_presence"))
```

### 3. Keep Python propagator escape hatch

Custom Python propagators (`murk.add_propagator(name, step_fn, ...)`) remain
available for logic that doesn't fit the library. Reward stays in
Python, derived from compiled observations.

### 4. Migrate examples

Update `heat_seeker`, `crystal_nav`, and `hex_pursuit` to use library
propagators instead of Python stencil code. This simultaneously:
- Demonstrates the library API
- Removes 30-60 lines of numpy boilerplate per example
- Gives each example a ~100x tick speedup for free

## Core Library (P3)

These cover the patterns observed across all existing examples:

### ScalarDiffusion

Jacobi Laplacian diffusion on a scalar field. Configurable:
- `input_field` / `output_field` (FieldId)
- `coefficient` (f64) -- diffusion rate
- `decay` (f64) -- exponential decay per tick (0 = none)
- `sources` (Vec<(usize, f32)>) -- fixed-value cells reset each tick
- `clamp_min` / `clamp_max` (Option<f32>) -- value clamping
- `gradient_field` (Option<FieldId>) -- optional gradient output

Reads previous (Jacobi-style), writes full. Already implemented as
`DiffusionPropagator` -- needs parameterization and source/clamp support.

### GradientCompute

Finite-difference gradient of a scalar field into a vector field.
Configurable:
- `input_field` (FieldId, scalar)
- `output_field` (FieldId, vector with dims matching space)

Currently embedded inside `DiffusionPropagator` -- extract as standalone.

### IdentityCopy

Copy a field's previous-tick values into the current tick unchanged.
Configurable:
- `field` (FieldId)

Used for persistent state fields (agent position in `hex_pursuit`).
Trivial to implement but saves users from writing a no-op propagator.

## Additional Reference Implementations (P4)

These extend the library for common RL environment patterns beyond what
the current examples need. Each is independently useful; prioritize
based on user demand.

### AgentEmission

Agents emit a scalar value at their current position each tick.
Useful for pheromone trails, scent marking, communication signals.
- `presence_field` (FieldId) -- which field encodes agent positions
- `emission_field` (FieldId) -- field to write emission into
- `intensity` (f32) -- emission strength per agent
- `mode` (Additive | Set) -- add to existing values or overwrite

Combines naturally with ScalarDiffusion + decay for pheromone-trail
environments (emit → diffuse → decay → observe).

### ResourceField

Linear or logistic regrowth of a consumable scalar field. Agents
consume by presence; field regenerates over time.
- `field` (FieldId)
- `presence_field` (FieldId) -- agents consume where present
- `consumption_rate` (f32)
- `regrowth_rate` (f32)
- `capacity` (f32) -- logistic cap
- `regrowth_model` (Linear | Logistic)

Common in foraging/harvesting environments.

### WavePropagation

Second-order wave equation (not just diffusion) for richer dynamics.
Requires two fields (displacement + velocity) and produces
qualitatively different behavior (propagating wavefronts, reflection
off boundaries, interference patterns).
- `displacement_field` (FieldId)
- `velocity_field` (FieldId)
- `wave_speed` (f64)
- `damping` (f64) -- energy loss per tick

Useful for environments where agents need to reason about signal
timing and direction, not just steady-state gradients.

### NoiseInjection

Add configurable noise to a field each tick. Useful for stochastic
environments, partial observability, and robustness training.
- `field` (FieldId)
- `noise_type` (Gaussian | Uniform | SaltPepper)
- `scale` (f64)
- `seed_offset` (u64) -- for deterministic replay

Must respect the determinism contract (seeded RNG, reproducible with
same seed).

### MorphologicalOp

Erosion/dilation on a binary or scalar field. Useful for computing
reachability, expanding danger zones, shrinking safe zones.
- `input_field` (FieldId)
- `output_field` (FieldId)
- `op` (Erode | Dilate)
- `radius` (u32)
- `threshold` (f32) -- binarization threshold for scalar fields

### FlowField

Compute a unit-direction vector field from a scalar potential field
(normalized negative gradient). Agents can "follow the flow" without
learning gradient descent themselves.
- `potential_field` (FieldId)
- `flow_field` (FieldId, vector)
- `normalize` (bool) -- unit vectors vs raw gradient magnitude

Pairs with ScalarDiffusion: diffuse a "goal scent" → compute flow →
agents follow flow field.

## Design Constraints

1. **FieldId parameterization, not field-name strings.** Rust API uses
   FieldId; Python bindings resolve names to IDs at config time.

2. **Builder pattern with validation.** Invalid configs (e.g.
   coefficient < 0, vector field for scalar diffusion) fail at build
   time, not at tick 10,000.

3. **Determinism contract.** All library propagators must be
   deterministic (same inputs → same outputs). NoiseInjection uses
   seeded RNG.

4. **No reward propagators.** Reward stays in Python, computed from
   the already-gathered observation. The library covers world state
   only.

5. **Backward compatible.** Existing Python `add_propagator(fn, ...)`
   API remains unchanged. Library propagators are additive.

6. **CFL enforcement.** Propagators that have stability constraints
   (diffusion, wave) implement `max_dt()` and the pipeline validator
   checks it.

## Migration Path

1. Parameterize `DiffusionPropagator` → `ScalarDiffusion` (rename + builder)
2. Extract `GradientCompute` from diffusion internals
3. Add `IdentityCopy` (trivial)
4. Expose all three through `murk-python` bindings
5. Migrate `heat_seeker` example (simplest, single field)
6. Migrate `crystal_nav` example (dual field, tests composition)
7. Migrate `hex_pursuit` example (identity-only, tests minimal case)
8. Update bench `reference_profile` (in `murk-bench` crate) to use library propagators
9. Deprecate hardcoded field constants in `murk-propagators::fields`

## Non-Goals

- **Reward propagators.** Reward stays in Python, computed from
  already-gathered observations. The library covers world-state only.
- **Multi-agent coordination propagators.** Agent interaction logic
  belongs in user code or a future higher-level crate.
- **GPU dispatch.** All P3/P4 propagators target CPU. GPU offload is
  a separate initiative.
- **Dynamic topology changes.** Library propagators assume a fixed
  `Space` for the lifetime of a run.

## Acceptance Criteria (P3)

The P3 milestone is complete when:

1. `ScalarDiffusion`, `GradientCompute`, and `IdentityCopy` are
   implemented with builder-pattern constructors parameterized on
   `FieldId`.
2. The bench `reference_profile` (`murk-bench`) produces bit-identical
   output when switched from the old `DiffusionPropagator` to the new
   library propagators.
3. All three Python examples (`heat_seeker`, `crystal_nav`,
   `hex_pursuit`) are migrated to use library propagators and pass CI.
4. Python bindings expose `ScalarDiffusion`, `GradientCompute`, and
   `IdentityCopy` as constructable classes in the `murk` module.
5. No remaining imports of hardcoded field constants (`HEAT`,
   `VELOCITY`, `HEAT_GRADIENT`) outside `murk-propagators` tests.

## Open Questions

- **Dynamic sources:** Fixed `(cell, value)` sources cover 90% of
  cases. Agent-driven sources (e.g. agents place heat) are handled by
  the P4 `AgentEmission` propagator. Open question: is a separate
  propagator sufficient, or do some use cases need agent emission and
  diffusion fused in a single step for correctness?

- **Multi-field diffusion:** `crystal_nav` has two independent
  diffusion fields (beacon + radiation). The natural API is two
  `ScalarDiffusion` instances with different parameters. Open
  question: can the engine pipeline them with good data locality, or
  does a batched variant need to exist for performance?

- **Anisotropic diffusion:** All current examples use isotropic
  diffusion. Is there demand for direction-dependent coefficients
  (e.g. wind, terrain-influenced flow)?

- **Space-specific optimizations:** Hex grids have different neighbor
  stencils than square grids. Should library propagators specialize
  per-space, or use the generic `Space::neighbours()` API?
