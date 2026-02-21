# murk-space

Spatial backends for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

Provides the `Space` trait and concrete lattice backends: `Line1D`, `Ring1D`,
`Square4`, `Square8`, `Hex2D`, `Fcc12`, and composable `ProductSpace`.
Each backend defines topology, neighborhoods, edge behavior, and region planning.

Available via the top-level [`murk`](https://crates.io/crates/murk) crate
as `murk::space`.

## Installation

```bash
cargo add murk-space
```

Most users should depend on the top-level [`murk`](https://crates.io/crates/murk) crate,
which re-exports this as `murk::space`.

## Usage

```rust
use murk_space::{Square4, EdgeBehavior, Space};

let grid = Square4::new(16, 16, EdgeBehavior::Absorb).unwrap();
assert_eq!(grid.cell_count(), 256);
assert_eq!(grid.ndim(), 2);

let coord = vec![2i32, 3].into();
let neighbors = grid.neighbours(&coord);
assert_eq!(neighbors.len(), 4);

let distance = grid.distance(&vec![0i32, 0].into(), &vec![3i32, 4].into());
assert_eq!(distance, 7.0);
```

## Documentation

- [Murk Book](https://tachyon-beep.github.io/murk/) — concepts and guides
- [API Reference](https://docs.rs/murk-space) — rustdoc
- [Examples](https://github.com/tachyon-beep/murk/tree/main/examples) — complete working projects
