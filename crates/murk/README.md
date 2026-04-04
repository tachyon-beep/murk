# murk

Top-level facade crate for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

## Installation

```bash
cargo add murk
```

## Usage

Add this single dependency to access the full Murk API:

```rust
use murk::prelude::*;
use murk::space::Square4;

let space = Square4::new(16, 16, EdgeBehavior::Absorb).unwrap();
let config = WorldConfig::builder()
    .space(Box::new(space))
    .fields(vec![FieldDef {
        name: "heat".into(),
        field_type: FieldType::Scalar,
        mutability: FieldMutability::PerTick,
        units: None,
        bounds: None,
        boundary_behavior: BoundaryBehavior::Clamp,
    }])
    .propagators(vec![])
    .dt(0.1)
    .seed(42)
    .build()
    .unwrap();
let mut world = LockstepWorld::new(config).unwrap();
```

Sub-crates are re-exported as modules (`murk::space`, `murk::engine`, etc.).
See the [documentation](https://tachyon-beep.github.io/murk/) for the full guide.

## Documentation

- [Murk Book](https://tachyon-beep.github.io/murk/) — concepts and guides
- [API Reference](https://docs.rs/murk) — rustdoc
- [Examples](https://github.com/tachyon-beep/murk/tree/main/examples) — complete working projects
