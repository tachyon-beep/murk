# Examples

Murk ships with four Python example projects demonstrating different
spatial backends and RL integration patterns.

| Example | Space | Demonstrates |
|---------|-------|-------------|
| [heat_seeker](https://github.com/tachyon-beep/murk/tree/main/examples/heat_seeker) | Square4 | PPO RL, diffusion physics, Python propagator |
| [hex_pursuit](https://github.com/tachyon-beep/murk/tree/main/examples/hex_pursuit) | Hex2D | Multi-agent, AgentDisk foveation |
| [crystal_nav](https://github.com/tachyon-beep/murk/tree/main/examples/crystal_nav) | Fcc12 | 3D lattice navigation |
| [batched_heat_seeker](https://github.com/tachyon-beep/murk/tree/main/examples/batched_heat_seeker) | Square4 | `BatchedVecEnv`, high-throughput parallel training |

There is also a Rust example:

| Example | Demonstrates |
|---------|-------------|
| [quickstart.rs](https://github.com/tachyon-beep/murk/tree/main/crates/murk-engine/examples/quickstart.rs) | Rust API: config, propagator, commands, snapshots |

The `BatchedVecEnv` adapter is demonstrated in the
[batched engine tests](https://github.com/tachyon-beep/murk/blob/main/crates/murk-python/tests/test_batched_vec_env.py),
which show config factory patterns, observation extraction, auto-reset,
and override hooks.

## Running the Python examples

```bash
# Install murk first
cd crates/murk-python && maturin develop --release && cd ../..

# Run an example
cd examples/heat_seeker
pip install -r requirements.txt
python heat_seeker.py
```
