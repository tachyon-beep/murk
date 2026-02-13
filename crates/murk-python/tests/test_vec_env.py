"""Tests for MurkVecEnv vectorized environment."""

import numpy as np
import pytest

from murk._murk import (
    Config,
    FieldMutability,
    ObsEntry,
    PropagatorDef,
    SpaceType,
)
from murk.env import MurkEnv
from murk.vec_env import MurkVecEnv


class CountEnv(MurkEnv):
    """Environment that terminates after 3 ticks."""

    def __init__(self, seed=42):
        cfg = Config()
        cfg.set_space(SpaceType.Line1D, [5.0, 0.0])
        cfg.add_field("x", mutability=FieldMutability.PerTick)
        cfg.set_dt(0.1)
        cfg.set_seed(seed)

        def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
            writes[0][:] = float(tick_id)

        prop = PropagatorDef("count", step_fn, writes=[(0, 0)])
        prop.register(cfg)

        super().__init__(cfg, [ObsEntry(0)], n_actions=2, seed=seed)

    def _check_terminated(self, obs, info):
        return info.get("tick_id", 0) >= 3


def test_vec_env_step_shapes():
    """VecEnv returns correct array shapes."""
    vec_env = MurkVecEnv([lambda: CountEnv(seed=i) for i in range(3)])
    obs, info = vec_env.reset()

    assert obs.shape == (3, 5)
    assert obs.dtype == np.float32

    obs, rewards, terminateds, truncateds, info = vec_env.step(np.array([0, 1, 0]))
    assert obs.shape == (3, 5)
    assert rewards.shape == (3,)
    assert terminateds.shape == (3,)
    assert truncateds.shape == (3,)
    vec_env.close()


def test_vec_env_auto_reset():
    """VecEnv auto-resets on termination, stores final_observation."""
    vec_env = MurkVecEnv([lambda: CountEnv(seed=42)])
    vec_env.reset()

    final_obs = None
    for step in range(10):
        obs, rewards, terminateds, truncateds, info = vec_env.step(np.array([0]))
        if terminateds[0]:
            final_obs = info["final_observation"][0]
            break

    assert final_obs is not None, "Should have terminated within 10 steps"
    assert isinstance(final_obs, np.ndarray)
    vec_env.close()


def test_vec_env_multiple_envs():
    """Multiple environments run independently."""
    n_envs = 4
    vec_env = MurkVecEnv([lambda i=i: CountEnv(seed=i) for i in range(n_envs)])
    obs, _ = vec_env.reset()
    assert obs.shape == (n_envs, 5)

    for _ in range(5):
        obs, _, _, _, _ = vec_env.step(np.zeros(n_envs, dtype=int))

    vec_env.close()


def test_vec_env_reset_with_seeds():
    """VecEnv reset with per-env seeds."""
    vec_env = MurkVecEnv([lambda: CountEnv(seed=0) for _ in range(2)])
    obs, _ = vec_env.reset(seed=[100, 200])
    assert obs.shape == (2, 5)
    vec_env.close()
