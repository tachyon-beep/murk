# murk-space

Spatial backends for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

Provides the `Space` trait and concrete lattice backends: `Line1D`, `Ring1D`,
`Square4`, `Square8`, `Hex2D`, `Fcc12`, and composable `ProductSpace`.
Each backend defines topology, neighborhoods, edge behavior, and region planning.

Available via the top-level [`murk`](https://crates.io/crates/murk) crate
as `murk::space`.
