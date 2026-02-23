# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent (wave-5)
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-obs

## Engine Mode

- [x] Both / Unknown

## Summary

`ObsPlan::compile` accepts `ObsRegion::AgentRect` with `half_extent.len() != space.ndim()`, and later `zip`-based coordinate math silently truncates dimensions, producing incorrect gathered observations instead of rejecting the invalid spec.

In `compile_standard` (plan.rs:435-451), the `AgentRect` branch passes the user-provided `half_extent` directly to `compile_agent_entry` without checking that `half_extent.len() == space.ndim()`. In contrast, the `AgentDisk` branch (plan.rs:416-418) correctly constructs `half_ext` with exactly `ndim` elements.

The `generate_template_ops` function (plan.rs:1232) uses `half_extent.len()` as ndim for the template, so template ops have `half_extent.len()` relative-coordinate dimensions. At execution time, `resolve_field_index` (plan.rs:1293-1327) zips the agent center (which has `space.ndim()` elements) with the template's relative coords (which has `half_extent.len()` elements). The `zip` iterator silently drops unmatched trailing dimensions.

## Steps to Reproduce

1. Create a 2D space (e.g. `Square4::new(5, 5, Absorb)`).
2. Construct a spec with `ObsRegion::AgentRect { half_extent: smallvec![1] }` (1D half-extent on 2D space).
3. Call `ObsPlan::compile(&spec, &space)` -- succeeds (should fail).
4. Execute the plan -- observations are silently wrong (one axis dropped).

## Expected Behavior

`compile` should fail with `ObsError::InvalidObsSpec` because `half_extent` dimensionality does not match `space.ndim()`.

## Actual Behavior

Compilation succeeds. Execution produces silently incorrect observation values. The second spatial axis is dropped from the observation due to zip truncation, returning a 1D slice through the 2D field instead of the intended 2D rectangle.

## Reproduction Rate

Always (deterministic, for any `half_extent.len() != space.ndim()`).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.9)

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
use murk_core::FieldId;
use murk_obs::spec::{ObsSpec, ObsEntry, ObsRegion, ObsTransform, ObsDtype};
use murk_obs::plan::ObsPlan;
use murk_space::{Square4, EdgeBehavior};
use smallvec::smallvec;

let space = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
let spec = ObsSpec {
    entries: vec![ObsEntry {
        field_id: FieldId(0),
        region: ObsRegion::AgentRect { half_extent: smallvec![1] }, // 1D on 2D space
        pool: None,
        transform: ObsTransform::Identity,
        dtype: ObsDtype::F32,
    }],
};

// Should be Err(InvalidObsSpec), but currently succeeds:
let result = ObsPlan::compile(&spec, &space);
assert!(result.is_ok()); // BUG: should fail
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-obs/src/plan.rs.md`

**Affected lines:**
- Missing validation: `crates/murk-obs/src/plan.rs:435-451` (AgentRect compile path)
- Correct pattern for comparison: `crates/murk-obs/src/plan.rs:416-418` (AgentDisk creates half_ext with ndim elements)
- Template uses half_extent.len(): `crates/murk-obs/src/plan.rs:1232` (generate_template_ops)
- Zip truncation sites: `crates/murk-obs/src/plan.rs:1297-1310` (resolve_field_index)

**Root cause:** No `half_extent.len() == space.ndim()` check in the AgentRect compile path, combined with `zip`-based coordinate composition that silently truncates mismatched lengths.

**Suggested fix:** Add a validation check at the start of the AgentRect branch:
```rust
if half_extent.len() != ndim {
    return Err(ObsError::InvalidObsSpec {
        reason: format!(
            "entry {i}: AgentRect half_extent has {} dims, but space requires {ndim}",
            half_extent.len()
        ),
    });
}
```
