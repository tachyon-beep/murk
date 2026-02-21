# murk-propagators

Reference propagator implementations for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

Includes ready-to-use propagators for common simulation patterns:
- `DiffusionPropagator` — graph-Laplacian heat diffusion
- `AgentMovementPropagator` — discrete agent movement with collision
- `RewardPropagator` — configurable reward computation

Available via the top-level [`murk`](https://crates.io/crates/murk) crate
as `murk::propagators`.

## Installation

```bash
cargo add murk-propagators
```

Most users should depend on the top-level [`murk`](https://crates.io/crates/murk) crate,
which re-exports this as `murk::propagators`.

## Usage

```rust
use murk_propagators::{
    DiffusionPropagator,
    RewardPropagator,
    reference_fields,
};

let fields = reference_fields();

let diffusion = DiffusionPropagator::new(0.1);
let reward = RewardPropagator::new(1.0, -0.01);
```

The `reference_fields()` function returns the five field definitions
(`heat`, `velocity`, `agent_presence`, `heat_gradient`, `reward`)
expected by the built-in propagators.

## Documentation

- [Murk Book](https://tachyon-beep.github.io/murk/) — concepts and guides
- [API Reference](https://docs.rs/murk-propagators) — rustdoc
- [Examples](https://github.com/tachyon-beep/murk/tree/main/examples) — complete working projects
