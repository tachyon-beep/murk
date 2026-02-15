# murk-engine

Simulation engine for the [Murk](https://github.com/tachyon-beep/murk) framework.

Provides two runtime modes:
- `LockstepWorld` — synchronous stepping (`&mut self`), ideal for RL training loops
- `RealtimeAsyncWorld` — autonomous background tick thread with epoch-based
  memory reclamation, for real-time applications

Also includes the tick engine, ingress command queue, and egress observation pool.

Available via the top-level [`murk`](https://crates.io/crates/murk) crate
as `murk::engine`.
