# Examples

Murk ships with four Python example projects demonstrating different
spatial backends and RL integration patterns.

| Example | Space | Demonstrates |
|---------|-------|-------------|
| [heat_seeker](https://github.com/tachyon-beep/murk/tree/main/examples/heat_seeker) | Square4 | PPO RL, diffusion physics, Python propagator |
| [hex_pursuit](https://github.com/tachyon-beep/murk/tree/main/examples/hex_pursuit) | Hex2D | Multi-agent, AgentDisk foveation |
| [crystal_nav](https://github.com/tachyon-beep/murk/tree/main/examples/crystal_nav) | Fcc12 | 3D lattice navigation |
| [layered_hex](https://github.com/tachyon-beep/murk/tree/main/examples/layered_hex) | Hex2D × Line1D | ProductSpace composition, multi-floor navigation |

There are also Rust examples:

| Example | Demonstrates |
|---------|-------------|
| [quickstart.rs](https://github.com/tachyon-beep/murk/tree/main/crates/murk-engine/examples/quickstart.rs) | Rust API: config, propagator, commands, snapshots |
| [realtime_async.rs](https://github.com/tachyon-beep/murk/tree/main/crates/murk-engine/examples/realtime_async.rs) | RealtimeAsyncWorld: background ticking, observe, shutdown |
| [replay.rs](https://github.com/tachyon-beep/murk/tree/main/crates/murk-engine/examples/replay.rs) | Deterministic replay: record, verify, prove determinism |

## Running the Python examples

```bash
# Install murk
pip install murk

# Clone and run an example
git clone https://github.com/tachyon-beep/murk.git
cd murk/examples/heat_seeker
pip install -r requirements.txt
python heat_seeker.py
```
