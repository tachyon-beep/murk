# murk-obs

Observation specification and tensor extraction for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

Build `ObsSpec` descriptions of what to observe (fields, regions, transforms,
pooling), compile them into `ObsPlan`s, and extract flat `f32` tensors with
validity masks. Supports foveation, multi-agent batching, and normalization.

Available via the top-level [`murk`](https://crates.io/crates/murk) crate
as `murk::obs`.

## Installation

```bash
cargo add murk-obs
```

Most users should depend on the top-level [`murk`](https://crates.io/crates/murk) crate,
which re-exports this as `murk::obs`.

## Usage

```rust
use murk_obs::{ObsSpec, ObsEntry, ObsDtype, ObsTransform, ObsRegion};
use murk_core::FieldId;
use murk_space::RegionSpec;

let spec = ObsSpec {
    entries: vec![
        ObsEntry {
            field_id: FieldId(0),
            region: ObsRegion::Fixed(RegionSpec::All),
            pool: None,
            transform: ObsTransform::Identity,
            dtype: ObsDtype::F32,
        },
        ObsEntry {
            field_id: FieldId(1),
            region: ObsRegion::AgentDisk { radius: 3 },
            pool: None,
            transform: ObsTransform::Normalize { min: 0.0, max: 100.0 },
            dtype: ObsDtype::F32,
        },
    ],
};
```

## Documentation

- [Murk Book](https://tachyon-beep.github.io/murk/) — concepts and guides
- [API Reference](https://docs.rs/murk-obs) — rustdoc
- [Examples](https://github.com/tachyon-beep/murk/tree/main/examples) — complete working projects
