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

`serialize()` emits a valid wire payload for `RegionSpec::Coords(vec![])`, but `deserialize()` always rejects that payload, breaking round-trip serialization for an empty-coordinates region.

## Steps to Reproduce

1. Construct an `ObsSpec` with one entry whose region is `ObsRegion::Fixed(RegionSpec::Coords(vec![]))`.
2. Call `serialize(&spec)` and keep the returned bytes.
3. Call `deserialize(&bytes)`.

## Expected Behavior

Deserialization succeeds and returns the original `ObsSpec` (round-trip invariant holds).

## Actual Behavior

Deserialization returns `ObsError::InvalidObsSpec` (message path: `entry 0: Coords expected 0 values, got 0`), so serialized bytes from a legal in-memory spec cannot be read back.

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

```text
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_core::FieldId;
use murk_obs::{deserialize, serialize, ObsDtype, ObsEntry, ObsRegion, ObsSpec, ObsTransform};
use murk_space::RegionSpec;

fn main() {
    let spec = ObsSpec {
        entries: vec![ObsEntry {
            field_id: FieldId(0),
            region: ObsRegion::Fixed(RegionSpec::Coords(vec![])),
            pool: None,
            transform: ObsTransform::Identity,
            dtype: ObsDtype::F32,
        }],
    };

    let bytes = serialize(&spec).unwrap();
    let err = deserialize(&bytes).unwrap_err();
    println!("{err:?}");
}
```

## Additional Context

Evidence in `crates/murk-obs/src/flatbuf.rs`:

- `encode_region` writes `ndim = 0` and `n_coords = 0` for empty coords:
  - `/home/john/murk/crates/murk-obs/src/flatbuf.rs:169`
  - `/home/john/murk/crates/murk-obs/src/flatbuf.rs:170`
  - `/home/john/murk/crates/murk-obs/src/flatbuf.rs:171`
- `decode_region` rejects all `ndim == 0` payloads unconditionally:
  - `/home/john/murk/crates/murk-obs/src/flatbuf.rs:364`
  - `/home/john/murk/crates/murk-obs/src/flatbuf.rs:367`

Root cause is an encoder/decoder contract mismatch for the empty-coordinates case. A direct fix is to explicitly accept `(ndim=0, n_coords=0, data.len()==0)` as `RegionSpec::Coords(vec![])` in `decode_region`, or reject empty coords during serialization so both directions enforce the same rule.