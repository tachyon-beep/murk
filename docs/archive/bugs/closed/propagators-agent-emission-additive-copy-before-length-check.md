# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent (wave-5)
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [x] murk-propagators

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`AgentEmission::step` in `EmissionMode::Additive` calls `out.copy_from_slice(&prev_emission)` before validating that `prev_emission.len() == out.len()`, causing a panic instead of a graceful error on length mismatch.

## Steps to Reproduce

1. Configure `AgentEmission` with `EmissionMode::Additive`.
2. Provide a mock/test setup where `reads_previous().read(emission_field)` returns a slice of length N and `writes().write(emission_field)` returns a buffer of length M where M != N.
3. Call `step()`.

## Expected Behavior

`step()` should return `Err(PropagatorError::ExecutionFailed { ... })` describing the length mismatch, consistent with the existing validation pattern at line 204 that checks `presence.len() != out.len()`.

## Actual Behavior

`step()` panics at `copy_from_slice` (line 201) when `prev_emission.len() != out.len()`, before reaching the existing length validation at line 204.

Evidence in `crates/murk-propagators/src/agent_emission.rs`:
- Line 201: `out.copy_from_slice(&prev_emission);` -- no prior length check on prev_emission vs out
- Line 204: `if presence.len() != out.len()` -- validates presence vs out, but NOT prev_emission vs out
- The `Set` mode path (lines 229-237) correctly validates before any indexing

## Reproduction Rate

Always (given mismatched lengths in a mock/test harness).

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
use murk_core::{FieldId, TickId};
use murk_propagators::{AgentEmission, EmissionMode};
use murk_propagator::{context::StepContext, propagator::Propagator, scratch::ScratchRegion};
use murk_space::{EdgeBehavior, Square4};
use murk_test_utils::{MockFieldReader, MockFieldWriter};

let f_pres = FieldId(100);
let f_emit = FieldId(101);

let prop = AgentEmission::builder()
    .presence_field(f_pres)
    .emission_field(f_emit)
    .intensity(1.0)
    .mode(EmissionMode::Additive)
    .build()
    .unwrap();

let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap(); // 9 cells
let mut reader = MockFieldReader::new();
reader.set_field(f_pres, vec![0.0; 9]);
reader.set_field(f_emit, vec![0.0; 8]); // mismatched previous emission len

let mut writer = MockFieldWriter::new();
writer.add_field(f_emit, 9); // output len 9

let mut scratch = ScratchRegion::new(0);
let mut ctx = StepContext::new(&reader, &reader, &mut writer, &mut scratch, &grid, TickId(1), 0.1);

// Panics inside step() at copy_from_slice instead of returning Err
let _ = prop.step(&mut ctx);
```

## Additional Context

In the real engine, `reads_previous` and `writes` for the same `FieldId` always yield slices of the same length (both backed by generations of the same `FieldDef`). This bug can only manifest via a mock/test harness or a corrupted arena state. Severity is Low because the precondition for triggering it is unlikely in production, but the fix is trivial and would make the code consistent with its own defensive validation pattern.

**Root cause:** `prev_emission` is copied into `out` before any `prev_emission.len() == out.len()` guard. The existing check at line 204 validates a different pair (presence vs out).

**Suggested fix:** Add `if prev_emission.len() != out.len() { return Err(PropagatorError::ExecutionFailed { ... }); }` before the `copy_from_slice` call at line 201, or reorder the existing checks to validate all lengths before any copy/indexing operations.

(Source report: `docs/bugs/generated/crates/murk-propagators/src/agent_emission.rs.md`)
