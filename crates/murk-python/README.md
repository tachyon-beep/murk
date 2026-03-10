# murk (Python)

[![PyPI](https://img.shields.io/pypi/v/murk.svg)](https://pypi.org/project/murk/)
[![Docs](https://github.com/tachyon-beep/murk/actions/workflows/docs.yml/badge.svg)](https://tachyon-beep.github.io/murk/)
[![CI](https://github.com/tachyon-beep/murk/actions/workflows/ci.yml/badge.svg)](https://github.com/tachyon-beep/murk/actions/workflows/ci.yml)

Python bindings for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

`murk` gives you:
- Native Rust performance via PyO3 bindings
- Gymnasium-compatible environment adapters
- Batched stepping for high-throughput RL training
- Typed Python API surface (`py.typed` + `.pyi` stubs)

## Install

```bash
python -m pip install murk
```

Requirements:
- Python 3.12+

## Quick Start (Gymnasium)

`MurkEnv` is designed for subclassing. Override hook methods to map actions,
reward, and episode boundaries for your task.

```python
import numpy as np
from murk import (
    Config,
    EdgeBehavior,
    FieldMutability,
    MurkEnv,
    ObsEntry,
    PropagatorDef,
    WriteMode,
)


class SimpleEnv(MurkEnv):
    def __init__(self, seed: int = 42):
        cfg = Config()
        cfg.set_space_line1d(10, EdgeBehavior.Absorb)
        cfg.add_field("value", mutability=FieldMutability.PerTick)
        cfg.set_dt(0.1)
        cfg.set_seed(seed)

        def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
            writes[0][:] = float(tick_id)

        PropagatorDef("inc", step_fn, writes=[(0, WriteMode.Full)]).register(cfg)
        super().__init__(cfg, [ObsEntry(0)], n_actions=2, seed=seed)
        self._tick_limit = 100

    def _compute_reward(self, obs, info):
        return -float(np.sum(obs))

    def _check_terminated(self, obs, info):
        return False

    def _check_truncated(self, obs, info):
        return info["tick_id"] >= self._tick_limit


env = SimpleEnv()
obs, info = env.reset(seed=0)
obs, reward, terminated, truncated, info = env.step(0)
env.close()
```

## High-Throughput Vectorized RL

`BatchedVecEnv` steps all worlds in one Rust call (single GIL release), which
removes per-world FFI overhead.

```python
import numpy as np
from murk import BatchedVecEnv, Config, EdgeBehavior, FieldMutability, ObsEntry


def make_config(i: int) -> Config:
    cfg = Config()
    cfg.set_space_line1d(64, EdgeBehavior.Absorb)
    cfg.add_field("energy", mutability=FieldMutability.PerTick)
    cfg.set_seed(i)
    return cfg


env = BatchedVecEnv(
    config_factory=make_config,
    obs_entries=[ObsEntry(0)],
    num_envs=32,
)
obs, infos = env.reset(seed=0)
obs, rewards, terminateds, truncateds, infos = env.step(np.zeros(32))
env.close()
```

## Low-Level API

If you want direct control of stepping and commands, use `World` and `ObsPlan`
from `murk._murk` / `murk`.

Core types:
- `Config`, `World`, `Command`
- `ObsEntry`, `ObsPlan`
- `StepMetrics` (timings, queue/realtime counters, sparse reuse counters)

## Package Links

- Repository: https://github.com/tachyon-beep/murk
- Docs: https://tachyon-beep.github.io/murk/
- Issues: https://github.com/tachyon-beep/murk/issues
