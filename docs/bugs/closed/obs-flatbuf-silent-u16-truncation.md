# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

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

`flatbuf::serialize` silently truncates `spec.entries.len()` via `as u16` (line 66), `region_params.len()` via `as u16` (line 81), and `cfg.kernel_size`/`cfg.stride` via `as u32` (lines 114-115). When values exceed the wire type's range, the header count is truncated but all entries are still serialized, producing a payload/header mismatch. The deserializer does not validate that all bytes were consumed, so trailing data is silently ignored.

## Steps to Reproduce

1. Create an `ObsSpec` with more than 65535 entries.
2. Call `flatbuf::serialize(&spec)`.
3. Call `flatbuf::deserialize(&bytes)`.
4. Observe only the first `(len as u16) as usize` entries are deserialized; remaining bytes are silently dropped.

## Expected Behavior

Serialization should fail with an error when values exceed wire type ranges. Deserialization should reject input with unconsumed trailing bytes.

## Actual Behavior

Serialization silently truncates via `as` cast. Deserialization returns `Ok(ObsSpec)` with fewer entries than were originally serialized, silently dropping the overflow entries.

## Reproduction Rate

- Deterministic for any input exceeding u16::MAX entries.

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
use murk_obs::flatbuf::{serialize, deserialize};
use murk_obs::spec::*;
use murk_core::FieldId;
use murk_space::RegionSpec;

// Create a spec with more entries than u16::MAX
let entries: Vec<ObsEntry> = (0..70_000u32)
    .map(|i| ObsEntry {
        field_id: FieldId(i),
        region: ObsRegion::Fixed(RegionSpec::All),
        pool: None,
        transform: ObsTransform::Identity,
        dtype: ObsDtype::F32,
    })
    .collect();
let spec = ObsSpec { entries };

let bytes = serialize(&spec);
let restored = deserialize(&bytes).unwrap();

// BUG: restored has 70000 % 65536 = 4464 entries, not 70000
assert_eq!(restored.entries.len(), 70000); // FAILS
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-obs/src/flatbuf.rs.md
**Verified lines:** flatbuf.rs:66 (entries.len() as u16), :81 (region_params.len() as u16), :114-115 (kernel_size/stride as u32), :185 (no trailing byte check)
**Root cause:** Unchecked `as` casts for wire-format narrowing, and no trailing-byte validation in deserializer.
**Suggested fix:** (1) Make `serialize` return `Result<Vec<u8>, ObsError>` and use `u16::try_from(spec.entries.len())` with error on overflow. (2) After deserializing all entries, verify `r.pos == bytes.len()` and return `InvalidObsSpec` if trailing bytes remain. (3) Apply the same pattern to `region_params.len()`, `kernel_size`, and `stride`.
