# Getting Started

## Prerequisites

**Rust** (for building from source or using the Rust API):
- Rust toolchain (stable, 1.87+): [rustup.rs](https://rustup.rs/)

**Python** (for the Gymnasium bindings):
- Python 3.12+
- Install `murk` from PyPI (numpy >= 1.24 and gymnasium >= 0.29 are installed automatically)
- [maturin](https://www.maturin.rs/) only if you are developing Murk from source

## Installation

For normal use, install published packages:

```bash
cargo add murk
python -m pip install murk
```

## Working on Murk itself (source checkout)

If you are contributing to Murk internals, use a source build:

```bash
git clone https://github.com/tachyon-beep/murk.git
cd murk

# Rust: build and test
cargo build --workspace
cargo test --workspace

# Python: build native extension in development mode
cd crates/murk-python
python -m pip install maturin
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
use murk_core::{FieldDef, FieldId, FieldMutability, FieldType, SnapshotAccess};
use murk_engine::{BackoffConfig, LockstepWorld, WorldConfig};
use murk_space::{EdgeBehavior, Square4};

let space = Square4::new(8, 8, EdgeBehavior::Absorb)?;
let fields = vec![FieldDef {
    name: "heat".into(),
    field_type: FieldType::Scalar,
    mutability: FieldMutability::PerTick,
    ..Default::default()
}];
let config = WorldConfig {
    space: Box::new(space), fields,
    propagators: vec![Box::new(DiffusionPropagator)],
    dt: 1.0, seed: 42, ..Default::default()
};
let mut world = LockstepWorld::new(config)?;
let result = world.step_sync(vec![])?;
let heat = result.snapshot.read(FieldId(0)).unwrap();
```

## First Python simulation

```python
import murk
from murk import Config, FieldType, FieldMutability, EdgeBehavior, WriteMode, ObsEntry, RegionType

config = Config()
config.set_space_square4(16, 16, EdgeBehavior.Absorb)
config.add_field("heat", FieldType.Scalar, FieldMutability.PerTick)
murk.add_propagator(
    config, name="diffusion", step_fn=diffusion_step,
    reads_previous=[0], writes=[(0, WriteMode.Full)],
)

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
from murk import BatchedVecEnv, Config, ObsEntry, RegionType

def make_config(i: int) -> Config:
    cfg = Config()
    cfg.set_space_square4(rows=16, cols=16)
    cfg.add_field("temperature", initial_value=0.0)
    return cfg

obs_entries = [ObsEntry(field_id=0, region_type=RegionType.All)]
env = BatchedVecEnv(make_config, obs_entries, num_envs=64)

obs, info = env.reset(seed=42)           # (64, obs_len)
obs, rewards, terms, truncs, info = env.step(actions)
env.close()
```

Subclass `BatchedVecEnv` and override the hook methods to customise
rewards, termination, and action-to-command mapping for your RL task.
See the [batched_heat_seeker](https://github.com/tachyon-beep/murk/tree/main/examples/batched_heat_seeker)
example for a complete working project, and the
[Concepts guide](concepts.md) for the full API.

## Next steps

- [Concepts](concepts.md) — understand spaces, fields, propagators, commands,
  and observations
- [Examples](examples.md) — complete Python RL training examples
- [API Reference](https://tachyon-beep.github.io/murk/api/) — full rustdoc
