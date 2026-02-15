# murk-replay

Deterministic replay recording and verification for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

Record simulation runs with `ReplayWriter`, replay them with `ReplayReader`,
and verify determinism with per-tick snapshot hashing and divergence reports.

Available via the top-level [`murk`](https://crates.io/crates/murk) crate
as `murk::replay`.
