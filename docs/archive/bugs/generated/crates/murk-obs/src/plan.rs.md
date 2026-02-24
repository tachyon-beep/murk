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
- [ ] murk-propagators
- [x] murk-obs
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

`ObsPlan::compile` accepts `ObsRegion::AgentRect` with `half_extent.len() != space.ndim()`, and later `zip`-based coordinate math silently truncates dimensions, producing incorrect gathered observations instead of rejecting invalid specs.

## Steps to Reproduce

1. Create a 2D space (e.g. `Square4`) and a spec with `ObsRegion::AgentRect { half_extent: smallvec![1] }` (1D half-extent on 2D space).
2. Compile with `ObsPlan::compile(&spec, &space)`.
3. Execute via `execute_agents` for a valid 2D center (e.g. `[1, 1]`) and inspect output/mask.

## Expected Behavior

Compilation should fail with `ObsError::InvalidObsSpec` because `half_extent` dimensionality does not match `space.ndim()`.

## Actual Behavior

Compilation succeeds. Execution proceeds with truncated coordinate arithmetic (due `zip`), effectively ignoring one axis and returning wrong observation values (deterministic, silent incorrect results).

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

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_core::{FieldId, SnapshotAccess, TickId, WorldGenerationId, ParameterVersion};
use murk_obs::{ObsDtype, ObsEntry, ObsPlan, ObsRegion, ObsSpec, ObsTransform};
use murk_space::{EdgeBehavior, Square4};
use murk_test_utils::MockSnapshot;
use smallvec::smallvec;

let space = Square4::new(3, 3, EdgeBehavior::Wrap).unwrap();

// Canonical row-major values: [0,1,2,3,4,5,6,7,8]
let mut snap = MockSnapshot::new(TickId(0), WorldGenerationId(1), ParameterVersion(0));
snap.set_field(FieldId(0), (0..9).map(|x| x as f32).collect());

let spec = ObsSpec {
    entries: vec![ObsEntry {
        field_id: FieldId(0),
        region: ObsRegion::AgentRect { half_extent: smallvec![1] }, // BUG: 1D in 2D space
        pool: None,
        transform: ObsTransform::Identity,
        dtype: ObsDtype::F32,
    }],
};

// Should be InvalidObsSpec, but currently succeeds.
let compiled = ObsPlan::compile(&spec, &space).unwrap();
assert_eq!(compiled.output_len, 3); // silently treated as 1D

let center = smallvec![1, 1];
let mut out = vec![0.0; compiled.output_len];
let mut mask = vec![0u8; compiled.mask_len];
compiled.plan.execute_agents(&snap, &space, &[center], None, &mut out, &mut mask).unwrap();

// Wrong semantics: second axis dropped by zip truncation.
assert_eq!(out, vec![0.0, 3.0, 6.0]);
assert_eq!(mask, vec![1, 1, 1]);
```

## Additional Context

Evidence in target file:

- Missing dimensionality validation for `AgentRect` in compile path:  
  `/home/john/murk/crates/murk-obs/src/plan.rs:435`  
  `/home/john/murk/crates/murk-obs/src/plan.rs:440`
- Shape/template built directly from `half_extent.len()` (not `space.ndim()`):  
  `/home/john/murk/crates/murk-obs/src/plan.rs:489`  
  `/home/john/murk/crates/murk-obs/src/plan.rs:493`  
  `/home/john/murk/crates/murk-obs/src/plan.rs:1232`
- Runtime validates only agent center dimensionality, not entry template dimensionality:  
  `/home/john/murk/crates/murk-obs/src/plan.rs:801`  
  `/home/john/murk/crates/murk-obs/src/plan.rs:803`
- Truncation source (`zip` drops unmatched dimensions):  
  `/home/john/murk/crates/murk-obs/src/plan.rs:1298`  
  `/home/john/murk/crates/murk-obs/src/plan.rs:1309`  
  `/home/john/murk/crates/murk-obs/src/plan.rs:1322`

Root cause: no `half_extent.len() == space.ndim()` check before plan compilation/execution, combined with `zip`-based coordinate composition.

Suggested fix: reject invalid `AgentRect` specs during compile with explicit dimensionality check; optionally harden `resolve_field_index` to return `None` on length mismatch before any `zip` operations.