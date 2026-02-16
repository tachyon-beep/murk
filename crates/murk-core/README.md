# murk-core

Core types crate for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

Defines the foundational types shared across all Murk crates: field definitions,
commands, receipts, IDs, error types, and the core traits (`FieldReader`,
`FieldWriter`, `SnapshotAccess`).

Most users should depend on the top-level [`murk`](https://crates.io/crates/murk)
crate instead, which re-exports these types via `murk::types`.

## Installation

```bash
cargo add murk-core
```

Most users should depend on the top-level [`murk`](https://crates.io/crates/murk) crate,
which re-exports this as `murk::types`.

## When to use directly

Depend on this crate directly only if building custom crates outside the Murk
workspace or need minimal dependencies.

## Usage

```rust
use murk_core::{FieldDef, FieldType, FieldMutability, BoundaryBehavior};

let heat = FieldDef {
    name: "heat".into(),
    field_type: FieldType::Scalar,
    mutability: FieldMutability::PerTick,
    units: Some("kelvin".into()),
    bounds: Some((0.0, 1000.0)),
    boundary_behavior: BoundaryBehavior::Clamp,
};

let velocity = FieldDef {
    name: "wind".into(),
    field_type: FieldType::Vector { dims: 3 },
    mutability: FieldMutability::Static,
    units: None,
    bounds: None,
    boundary_behavior: BoundaryBehavior::Clamp,
};
```

## Documentation

- [Murk Book](https://tachyon-beep.github.io/murk/) — concepts and guides
- [API Reference](https://docs.rs/murk-core) — rustdoc
- [Examples](https://github.com/tachyon-beep/murk/tree/main/examples) — complete working projects
