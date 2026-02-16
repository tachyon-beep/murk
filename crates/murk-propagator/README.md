# murk-propagator

Propagator trait and pipeline validation for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

The `Propagator` trait is the main extension point for user-defined simulation
logic. Propagators are stateless per-tick operators that declare their field
reads and writes, enabling automatic write-conflict detection, Euler/Jacobi
read modes, and CFL validation.

Available via the top-level [`murk`](https://crates.io/crates/murk) crate
as `murk::propagator`.

## Installation

```bash
cargo add murk-propagator
```

Most users should depend on the top-level [`murk`](https://crates.io/crates/murk) crate,
which re-exports this as `murk::propagator`.

## Usage

```rust
use murk_propagator::{Propagator, StepContext, WriteMode};
use murk_core::{FieldId, FieldSet, PropagatorError};

struct ConstantFill {
    field: FieldId,
    value: f32,
}

impl Propagator for ConstantFill {
    fn name(&self) -> &str { "constant_fill" }

    fn reads(&self) -> FieldSet { FieldSet::empty() }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.field, WriteMode::Full)]
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let buf = ctx.writes().write(self.field).unwrap();
        buf.fill(self.value);
        Ok(())
    }
}
```

## Documentation

- [Murk Book](https://tachyon-beep.github.io/murk/) — concepts and guides
- [API Reference](https://docs.rs/murk-propagator) — rustdoc
- [Examples](https://github.com/tachyon-beep/murk/tree/main/examples) — complete working projects
