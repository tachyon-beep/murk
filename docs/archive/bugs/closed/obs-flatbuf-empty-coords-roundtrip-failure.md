# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent (wave-5)
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-obs

## Engine Mode

- [x] Both / Unknown

## Summary

`serialize()` emits a valid wire payload for `RegionSpec::Coords(vec![])` (empty coordinate list), but `deserialize()` unconditionally rejects that payload because the decoder treats `ndim == 0` as invalid. This breaks the round-trip serialization invariant for empty-coordinates regions.

In `encode_region` (flatbuf.rs:168-175), when `coords` is empty:
- `ndim = coords.first().map(|c| c.len()).unwrap_or(0) as i32` produces `ndim = 0`
- `n_coords = coords.len() as i32` produces `n_coords = 0`
- The params `[0, 0]` are written successfully.

In `decode_region` (flatbuf.rs:364-375), the condition `if ndim == 0 || data.len() != ndim * n_coords` rejects `ndim == 0` regardless of `n_coords` and `data.len()`, so the valid serialized form is always rejected.

## Steps to Reproduce

1. Construct an `ObsSpec` with one entry whose region is `ObsRegion::Fixed(RegionSpec::Coords(vec![]))`.
2. Call `serialize(&spec)` -- succeeds, returns bytes.
3. Call `deserialize(&bytes)` -- fails with `InvalidObsSpec`.

## Expected Behavior

Deserialization succeeds and returns the original `ObsSpec` (round-trip invariant holds). Either both sides accept empty coords, or both sides reject them.

## Actual Behavior

Serialization succeeds, deserialization fails. The encoder/decoder contract is inconsistent.

## Reproduction Rate

Always (deterministic).

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
use murk_obs::flatbuf::{serialize, deserialize};
use murk_obs::spec::{ObsSpec, ObsEntry, ObsRegion, ObsTransform, ObsDtype};
use murk_core::FieldId;
use murk_space::RegionSpec;

let spec = ObsSpec {
    entries: vec![ObsEntry {
        field_id: FieldId(0),
        region: ObsRegion::Fixed(RegionSpec::Coords(vec![])),
        pool: None,
        transform: ObsTransform::Identity,
        dtype: ObsDtype::F32,
    }],
};

let bytes = serialize(&spec).unwrap();      // succeeds
let err = deserialize(&bytes).unwrap_err();  // fails: "Coords expected 0 values, got 0"
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-obs/src/flatbuf.rs.md`

**Affected lines:**
- Encoder: `crates/murk-obs/src/flatbuf.rs:168-175` (encode_region for Coords)
- Decoder: `crates/murk-obs/src/flatbuf.rs:364-375` (decode_region REGION_COORDS branch)

**Root cause:** Encoder/decoder contract mismatch for empty coordinates. The encoder allows `ndim == 0` (derived from an empty coords vec), but the decoder unconditionally rejects it.

**Suggested fix:** Either:
1. Accept `(ndim=0, n_coords=0, data.len()==0)` in decode_region as `RegionSpec::Coords(vec![])`, or
2. Reject empty coords during serialization so both directions enforce the same rule.

Option 1 preserves the most general semantics. Option 2 is simpler if empty coords are considered invalid by design.
