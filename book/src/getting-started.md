# Getting Started

## Prerequisites

**Rust** (for building from source or using the Rust API):
- Rust toolchain (stable, 1.87+): [rustup.rs](https://rustup.rs/)

**Python** (for the Gymnasium bindings):
- Python 3.9+
- [maturin](https://www.maturin.rs/) (`pip install maturin`)
- numpy >= 1.24, gymnasium >= 0.29 (installed automatically)

## Installation

Murk is not yet on PyPI or crates.io. Install from source:

```bash
git clone https://github.com/tachyon-beep/murk.git
cd murk

# Rust: build and test
cargo build --workspace
cargo test --workspace

# Python: build native extension in development mode
cd crates/murk-python
pip install maturin
maturin develop --release
```

## First Rust simulation

Run the built-in quickstart example:

```bash
cargo run --example quickstart -p murk-engine
```

See [`crates/murk-engine/examples/quickstart.rs`](https://github.com/tachyon-beep/murk/blob/main/crates/murk-engine/examples/quickstart.rs)
for the full source. The essential pattern:

```rust
let config = WorldConfig { space, fields, propagators, dt: 0.1, seed: 42, .. };
let mut world = LockstepWorld::new(config)?;
let result = world.step_sync(commands)?;
let heat = result.snapshot.read(FieldId(0)).unwrap();
```

## First Python simulation

```python
import murk
from murk import Config, FieldMutability, EdgeBehavior, WriteMode, ObsEntry, RegionType

config = Config()
config.set_space_square4(16, 16, EdgeBehavior.Absorb)
config.add_field("heat", mutability=FieldMutability.PerTick)
# ... add propagators ...

env = murk.MurkEnv(config, obs_entries=[ObsEntry(0, region_type=RegionType.All)], n_actions=5)
obs, info = env.reset()

for _ in range(1000):
    action = policy(obs)
    obs, reward, terminated, truncated, info = env.step(action)
```

## Scaling up: BatchedVecEnv

For RL training with many parallel environments, `BatchedVecEnv` steps
all worlds in a single Rust call with one GIL release — eliminating the
per-environment FFI overhead of `MurkVecEnv`.

```python
from murk import BatchedVecEnv, Config, ObsEntry, SpaceType, RegionType

def make_config(i: int) -> Config:
    cfg = Config()
    cfg.set_space(SpaceType.Square4, rows=16, cols=16)
    cfg.add_field("temperature", initial_value=0.0)
    return cfg

obs_entries = [ObsEntry("temperature", RegionType.Full, region_params=[])]
env = BatchedVecEnv(make_config, obs_entries, num_envs=64)

obs, info = env.reset(seed=42)           # (64, obs_len)
obs, rewards, terms, truncs, info = env.step(actions)
env.close()
```

Subclass `BatchedVecEnv` and override the hook methods to customise
rewards, termination, and action-to-command mapping for your RL task.
See the [Concepts guide](../docs/CONCEPTS.md) for details.

## Next steps

- [Concepts](concepts.md) — understand spaces, fields, propagators, commands,
  and observations
- [Examples](examples.md) — complete Python RL training examples
- [API Reference](https://tachyon-beep.github.io/murk/api/) — full rustdoc
