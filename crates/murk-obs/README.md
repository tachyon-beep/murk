# murk-obs

Observation specification and tensor extraction for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

Build `ObsSpec` descriptions of what to observe (fields, regions, transforms,
pooling), compile them into `ObsPlan`s, and extract flat `f32` tensors with
validity masks. Supports foveation, multi-agent batching, and normalization.

Available via the top-level [`murk`](https://crates.io/crates/murk) crate
as `murk::obs`.
