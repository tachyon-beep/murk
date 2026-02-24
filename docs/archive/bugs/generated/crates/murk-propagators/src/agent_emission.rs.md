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

`AgentEmission::step` in `EmissionMode::Additive` can panic on mismatched previous-emission/output lengths because it calls `copy_from_slice` before validating lengths.

## Steps to Reproduce

1. Configure `AgentEmission` with `mode(EmissionMode::Additive)`.
2. Provide `reads_previous().read(emission_field)` with length `N`, and `writes().write(emission_field)` with length `M != N`.
3. Call `step()`.

## Expected Behavior

`step()` should return `Err(PropagatorError::ExecutionFailed { ... })` for any field length mismatch, without panicking.

## Actual Behavior

`step()` panics at `copy_from_slice` when lengths differ, before reaching explicit mismatch error handling.

Evidence:
- `out.copy_from_slice(&prev_emission);` at `crates/murk-propagators/src/agent_emission.rs:201`
- Length validation only checks `presence.len()` vs `out.len()` at `crates/murk-propagators/src/agent_emission.rs:204`
- No validation of `prev_emission.len()` vs `out.len()` before copy

## Reproduction Rate

Always (given mismatched lengths).

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

// Panics inside step() at copy_from_slice
let _ = prop.step(&mut ctx);
```

## Additional Context

Root cause: `prev_emission` is copied into `out` before any `prev_emission.len() == out.len()` guard.  
Suggested fix: validate both `prev_emission.len()` and `presence.len()` against `out.len()` before any indexing/copy operations, and return `PropagatorError::ExecutionFailed` instead of panicking.