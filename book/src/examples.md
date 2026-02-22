# Examples

Murk ships with seven Python example projects demonstrating different
spatial backends, RL integration patterns, and the batched engine.

| Example | Space | Demonstrates |
|---------|-------|-------------|
| [heat_seeker](https://github.com/tachyon-beep/murk/tree/main/examples/heat_seeker) | Square4 | PPO RL, diffusion physics, Python propagator |
| [hex_pursuit](https://github.com/tachyon-beep/murk/tree/main/examples/hex_pursuit) | Hex2D | Multi-agent, AgentDisk foveation |
| [crystal_nav](https://github.com/tachyon-beep/murk/tree/main/examples/crystal_nav) | Fcc12 | 3D lattice navigation |
| [layered_hex](https://github.com/tachyon-beep/murk/tree/main/examples/layered_hex) | ProductSpace (Hex2D × Line1D) | Multi-floor navigation |
| [batched_heat_seeker](https://github.com/tachyon-beep/murk/tree/main/examples/batched_heat_seeker) | Square4 | `BatchedVecEnv`, high-throughput parallel training |
| [batched_benchmark](https://github.com/tachyon-beep/murk/tree/main/examples/batched_benchmark) | Square4 | `BatchedVecEnv` vs `MurkVecEnv` vs raw `BatchedWorld` throughput comparison |
| [batched_cookbook](https://github.com/tachyon-beep/murk/tree/main/examples/batched_cookbook) | Square4 | Low-level `BatchedWorld` API: lifecycle, context manager, per-world commands, selective reset |

There are also three Rust examples:

| Example | Demonstrates |
|---------|-------------|
| [quickstart.rs](https://github.com/tachyon-beep/murk/tree/main/crates/murk-engine/examples/quickstart.rs) | Rust API: config, propagator, commands, snapshots |
| [realtime_async.rs](https://github.com/tachyon-beep/murk/tree/main/crates/murk-engine/examples/realtime_async.rs) | RealtimeAsyncWorld: background ticking, observe, shutdown |
| [replay.rs](https://github.com/tachyon-beep/murk/tree/main/crates/murk-engine/examples/replay.rs) | Deterministic replay: record, verify, prove determinism |

The `BatchedVecEnv` adapter is demonstrated in the
[batched engine tests](https://github.com/tachyon-beep/murk/blob/main/crates/murk-python/tests/test_batched_vec_env.py),
which show config factory patterns, observation extraction, auto-reset,
and override hooks.

## Running the Python examples

```bash
# Install published murk package (default)
python -m pip install murk

# If you are developing Murk internals from source instead:
# cd crates/murk-python && maturin develop --release && cd ../..

# Run an example
cd examples/heat_seeker
pip install -r requirements.txt
python heat_seeker.py
```
