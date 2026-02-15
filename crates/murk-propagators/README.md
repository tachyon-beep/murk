# murk-propagators

Reference propagator implementations for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

Includes ready-to-use propagators for common simulation patterns:
- `DiffusionPropagator` — graph-Laplacian heat diffusion
- `AgentMovementPropagator` — discrete agent movement with collision
- `RewardPropagator` — configurable reward computation

Available via the top-level [`murk`](https://crates.io/crates/murk) crate
as `murk::propagators`.
