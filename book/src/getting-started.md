# Getting Started

## Prerequisites

**Rust** (for building from source or using the Rust API):
- Rust toolchain (stable, 1.75+): [rustup.rs](https://rustup.rs/)

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

## Next steps

- [Concepts](concepts.md) — understand spaces, fields, propagators, commands,
  and observations
- [Examples](examples.md) — complete Python RL training examples
- [API Reference](https://tachyon-beep.github.io/murk/api/) — full rustdoc
