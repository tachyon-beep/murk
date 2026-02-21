# murk-engine

Simulation engine for the [Murk](https://github.com/tachyon-beep/murk) framework.

Provides two runtime modes:
- `LockstepWorld` — synchronous stepping (`&mut self`), ideal for RL training loops
- `RealtimeAsyncWorld` — autonomous background tick thread with epoch-based
  memory reclamation, for real-time applications

Also includes the tick engine, ingress command queue, and egress observation pool.

Available via the top-level [`murk`](https://crates.io/crates/murk) crate
as `murk::engine`.

## Installation

```bash
cargo add murk-engine
```

Most users should depend on the top-level [`murk`](https://crates.io/crates/murk) crate,
which re-exports this as `murk::engine`.

## Usage

```rust
use murk_engine::{LockstepWorld, WorldConfig, BackoffConfig};
use murk_core::{FieldDef, FieldType, FieldMutability, BoundaryBehavior};
use murk_space::{Square4, EdgeBehavior};

let space = Square4::new(16, 16, EdgeBehavior::Absorb).unwrap();
let config = WorldConfig {
    space: Box::new(space),
    fields: vec![FieldDef {
        name: "heat".into(),
        field_type: FieldType::Scalar,
        mutability: FieldMutability::PerTick,
        units: None,
        bounds: None,
        boundary_behavior: BoundaryBehavior::Clamp,
    }],
    propagators: vec![],
    dt: 0.1,
    seed: 42,
    ring_buffer_size: 8,
    max_ingress_queue: 64,
    tick_rate_hz: None,
    backoff: BackoffConfig::default(),
};

let mut world = LockstepWorld::new(config).unwrap();
let result = world.step_sync(vec![]).unwrap();
```

## Documentation

- [Murk Book](https://tachyon-beep.github.io/murk/) — concepts and guides
- [API Reference](https://docs.rs/murk-engine) — rustdoc
- [Examples](https://github.com/tachyon-beep/murk/tree/main/examples) — complete working projects
