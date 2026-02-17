# Propagator Triage Summary

**Date:** 2026-02-17
**Triaged by:** static-analysis-triage
**Reports reviewed:** 11
**Crates covered:** murk-propagator, murk-propagators

---

## Statistics

| Classification       | Count | Reports                                          |
| -------------------- | ----- | ------------------------------------------------ |
| CONFIRMED            | 6     | See tickets below                                |
| FALSE_POSITIVE       | 0     | --                                               |
| DESIGN_AS_INTENDED   | 1     | guard.rs (as_mut_slice bulk-write assumption)    |
| SKIPPED (no bug)     | 4     | context.rs, lib.rs (x2), fields.rs               |
| **Total**            | **11**|                                                  |

---

## Confirmed Bugs (6 tickets filed)

### High Severity (2)

| # | Ticket | Crate(s) | Summary |
|---|--------|----------|---------|
| 1 | [propagator-writemode-incremental-not-implemented.md](propagator-writemode-incremental-not-implemented.md) | murk-propagator, murk-propagators, murk-engine, murk-arena | `WriteMode::Incremental` is documented but never implemented. PerTick buffers are always zero-initialized. `AgentMovementPropagator` depends on state persistence that the engine does not provide. Tests mask the bug with manual buffer pre-fill. |
| 2 | [propagator-diffusion-cfl-hardcoded-degree.md](propagator-diffusion-cfl-hardcoded-degree.md) | murk-propagators | `DiffusionPropagator::max_dt()` hardcodes `1/(4*D)` for Square4's 4-neighbor stencil. On Hex2D (6 neighbors) or Fcc12 (12 neighbors), the approved timestep causes `alpha > 1.0`, producing numerically unstable/non-physical results (negative heat values). |

### Medium Severity (3)

| # | Ticket | Crate(s) | Summary |
|---|--------|----------|---------|
| 3 | [propagator-agent-movement-tick0-actions.md](propagator-agent-movement-tick0-actions.md) | murk-propagators | `AgentMovementPropagator` applies movement actions on the same tick it initializes positions, violating the documented "init on tick 0, move on subsequent ticks" contract. Missing early return after tick-0 initialization. |
| 4 | [propagator-pipeline-nan-maxdt-bypass.md](propagator-pipeline-nan-maxdt-bypass.md) | murk-propagator | `validate_pipeline()` does not validate per-propagator `max_dt()` for finiteness. A propagator returning `Some(NaN)` silently bypasses the timestep constraint via IEEE-754 comparison semantics. |
| 5 | [propagator-reward-stale-heat-gradient-dependency.md](propagator-reward-stale-heat-gradient-dependency.md) | murk-propagators | `RewardPropagator::reads()` declares `HEAT_GRADIENT` as a dependency but `step()` never reads it. Creates unnecessary coupling and forces users to define a field they do not need. |

### Low Severity (1)

| # | Ticket | Crate(s) | Summary |
|---|--------|----------|---------|
| 6 | [propagator-scratch-byte-capacity-rounds-down.md](propagator-scratch-byte-capacity-rounds-down.md) | murk-propagator, murk-engine | `ScratchRegion::with_byte_capacity()` uses floor division to convert bytes to f32 slots, potentially under-allocating. Latent: no current propagator requests non-zero scratch. |

---

## Design As Intended (1)

### guard.rs -- `as_mut_slice()` marks all cells as written

**Report claim:** `FullWriteGuard::as_mut_slice()` marks all cells as written before the caller actually writes, defeating the incomplete-write diagnostic.

**Verdict:** DESIGN_AS_INTENDED. The doc comment at guard.rs:57 explicitly states: "Marks ALL cells as written -- assumes the caller fills the entire slice." This is a deliberate trust-the-caller optimization for the bulk-write path. The guard is a debug-only diagnostic tool (gated on `#[cfg(debug_assertions)]`), not a safety mechanism. If a caller uses `as_mut_slice()` but does not fill the slice, the bug is in the propagator, not the guard. The test at guard.rs:146-152 (`as_mut_slice_marks_complete`) confirms this is the intended behavior.

---

## Skipped (No Bug) -- 4 reports

| File | Reason |
|------|--------|
| `murk-propagator/src/context.rs` | Thin context wrapper. No logic, no bugs. |
| `murk-propagator/src/lib.rs` | Module declarations and re-exports only. |
| `murk-propagators/src/lib.rs` | Module declarations and re-exports only. |
| `murk-propagators/src/fields.rs` | Constant definitions with correct sequential IDs. |

---

## Priority Ordering for Fixes

1. **propagator-writemode-incremental-not-implemented** -- Blocks any real use of `AgentMovementPropagator` with the live engine. The test suite masks the bug, so this will surface immediately in integration/E2E scenarios.
2. **propagator-diffusion-cfl-hardcoded-degree** -- Blocks correct diffusion on any non-Square4 space. Silent numerical corruption.
3. **propagator-agent-movement-tick0-actions** -- Semantic violation, but practically unlikely to trigger in normal RL loops (actions are typically empty on tick 0).
4. **propagator-pipeline-nan-maxdt-bypass** -- Defensive validation gap. Requires a propagator to actively return `Some(NaN)`, which is a programming error in the propagator.
5. **propagator-reward-stale-heat-gradient-dependency** -- Unnecessary coupling. Easy one-line fix.
6. **propagator-scratch-byte-capacity-rounds-down** -- Latent. No current propagator uses non-zero scratch bytes.

---

## Cross-Cutting Observations

- **WriteMode is load-bearing but unimplemented.** The `WriteMode` enum is used in the API surface (`Propagator::writes()`) and consumed in pipeline validation (`write-write conflict detection`), but its runtime semantics (`Full` vs `Incremental` buffer initialization) are completely missing. This is the most impactful finding.

- **Tests mask arena-level bugs.** The `AgentMovementPropagator` tests use `MockFieldWriter` with manual pre-fill, which simulates incremental seeding. This means the propagator's logic is correct, but the live engine path through `PingPongArena` + `WriteArena` does not honor the mode. Integration tests with the real `TickEngine` would have caught this.

- **`max_dt()` is too coarse for multi-topology support.** The current `Propagator::max_dt()` signature returns a single `Option<f64>` with no knowledge of the space topology. For propagators like `DiffusionPropagator` whose stability bound depends on neighbor degree, this interface is insufficient. Consider either (a) making `max_dt` depend on `&dyn Space`, or (b) having propagators report a per-degree formula.
