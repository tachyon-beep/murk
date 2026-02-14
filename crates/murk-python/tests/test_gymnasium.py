"""Tests for MurkEnv Gymnasium adapter."""

import numpy as np
import pytest

import gymnasium
from gymnasium import spaces

from murk._murk import (
    Config,
    Command,
    FieldMutability,
    ObsEntry,
    PropagatorDef,
    SpaceType,
    WriteMode,
)
from murk.env import MurkEnv


class SimpleEnv(MurkEnv):
    """Minimal MurkEnv subclass for testing."""

    def __init__(self, seed=42):
        cfg = Config()
        cfg.set_space(SpaceType.Line1D, [10.0, 0.0])
        cfg.add_field("value", mutability=FieldMutability.PerTick)
        cfg.set_dt(0.1)
        cfg.set_seed(seed)

        def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
            writes[0][:] = float(tick_id)

        prop = PropagatorDef("inc", step_fn, writes=[(0, WriteMode.Full)])
        prop.register(cfg)

        obs_entries = [ObsEntry(0)]
        super().__init__(cfg, obs_entries, n_actions=2, seed=seed)
        self._tick_limit = 10

    def _compute_reward(self, obs, info):
        return -float(np.sum(obs))

    def _check_terminated(self, obs, info):
        return info.get("tick_id", 0) >= 5

    def _check_truncated(self, obs, info):
        return info.get("tick_id", 0) >= self._tick_limit


def test_gymnasium_step_returns_correct_types():
    """step() returns (obs, reward, terminated, truncated, info)."""
    env = SimpleEnv()
    obs, info = env.reset()

    assert isinstance(obs, np.ndarray)
    assert obs.dtype == np.float32
    assert obs.shape == (10,)

    obs, reward, terminated, truncated, info = env.step(0)
    assert isinstance(obs, np.ndarray)
    assert isinstance(reward, float)
    assert isinstance(terminated, bool)
    assert isinstance(truncated, bool)
    assert isinstance(info, dict)
    assert "tick_id" in info

    env.close()


def test_gymnasium_reset_returns_obs_info():
    """reset() returns (obs, info)."""
    env = SimpleEnv()
    result = env.reset()
    assert len(result) == 2
    obs, info = result
    assert isinstance(obs, np.ndarray)
    assert isinstance(info, dict)
    env.close()


def test_gymnasium_observation_space():
    """observation_space has correct shape and dtype."""
    env = SimpleEnv()
    assert isinstance(env.observation_space, spaces.Box)
    assert env.observation_space.shape == (10,)
    assert env.observation_space.dtype == np.float32
    env.close()


def test_gymnasium_action_space():
    """action_space defaults to Discrete."""
    env = SimpleEnv()
    assert isinstance(env.action_space, spaces.Discrete)
    assert env.action_space.n == 2
    env.close()


def test_gymnasium_termination():
    """Episode terminates when _check_terminated returns True."""
    env = SimpleEnv()
    env.reset()

    terminated = False
    for _ in range(20):
        _, _, terminated, _, info = env.step(0)
        if terminated:
            break

    assert terminated
    assert info["tick_id"] >= 5
    env.close()


def test_gymnasium_truncation():
    """Episode truncates at tick_limit."""
    env = SimpleEnv()
    env.reset()

    # Override terminated to never fire
    env._check_terminated = lambda obs, info: False

    truncated = False
    for _ in range(20):
        _, _, _, truncated, info = env.step(0)
        if truncated:
            break

    assert truncated
    env.close()


def test_gymnasium_reset_with_seed():
    """reset(seed=N) changes the RNG seed."""
    env = SimpleEnv()
    obs1, _ = env.reset(seed=100)
    obs2, _ = env.reset(seed=200)
    # Both should have valid observations (tick 1 after reset+step)
    assert obs1.shape == (10,)
    assert obs2.shape == (10,)
    env.close()


def test_gymnasium_last_step_metrics():
    """last_step_metrics is populated after step."""
    env = SimpleEnv()
    env.reset()
    env.step(0)
    metrics = env.last_step_metrics
    assert metrics is not None
    assert metrics.total_us >= 0
    env.close()


def test_gymnasium_custom_action_space():
    """Custom action_space is respected."""
    cfg = Config()
    cfg.set_space(SpaceType.Line1D, [5.0, 0.0])
    cfg.add_field("x", mutability=FieldMutability.PerTick)
    cfg.set_dt(0.1)
    cfg.set_seed(0)

    def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
        writes[0][:] = 0.0

    prop = PropagatorDef("noop", step_fn, writes=[(0, WriteMode.Full)])
    prop.register(cfg)

    custom_space = spaces.Box(low=-1.0, high=1.0, shape=(3,), dtype=np.float32)
    env = MurkEnv(cfg, [ObsEntry(0)], action_space=custom_space)
    assert isinstance(env.action_space, spaces.Box)
    assert env.action_space.shape == (3,)
    env.close()
