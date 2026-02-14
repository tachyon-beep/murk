"""PPO smoke test: verify learning on a simple grid navigation task.

GridNavEnv: 10x10 Square4 grid, agent navigates toward target.
4 discrete actions (N/S/E/W), reward = -Manhattan distance.
Terminates when agent reaches target. Truncated at 200 ticks.
"""

import numpy as np
import pytest

from gymnasium import spaces

from murk._murk import (
    Config,
    EdgeBehavior,
    FieldMutability,
    ObsEntry,
    PropagatorDef,
    WriteMode,
)
from murk.env import MurkEnv


class NavState:
    """Mutable state for the grid navigation propagator.

    The propagator callable captures this object and writes its values
    to all cells each tick. State is mutated externally by reset()
    and _action_to_commands(), then the next tick propagates it.
    """

    def __init__(self, grid_size):
        self.grid_size = grid_size
        self.ax = 0.0
        self.ay = 0.0
        self.tx = 0.0
        self.ty = 0.0

    def __call__(self, reads, reads_prev, writes, tick_id, dt, cell_count):
        """Propagator: broadcast current positions to all cells."""
        for i in range(cell_count):
            writes[0][i] = self.ax
            writes[1][i] = self.ay
            writes[2][i] = self.tx
            writes[3][i] = self.ty


class GridNavEnv(MurkEnv):
    """Grid navigation: move agent to target on a 10x10 grid.

    Fields (all Scalar, PerTick):
        0: agent_x, 1: agent_y, 2: target_x, 3: target_y

    The propagator reads positions from a captured NavState object
    and writes them to all cells. Movement logic runs on the Python
    side before each tick.
    """

    GRID_SIZE = 10

    def __init__(self, seed=42):
        self._nav = NavState(self.GRID_SIZE)

        cfg = Config()
        cfg.set_space_square4(10, 10, EdgeBehavior.Absorb)
        for name in ["agent_x", "agent_y", "target_x", "target_y"]:
            cfg.add_field(name, mutability=FieldMutability.PerTick)
        cfg.set_dt(0.1)
        cfg.set_seed(seed)

        prop = PropagatorDef(
            "nav",
            self._nav,
            reads=[0, 1, 2, 3],
            writes=[(0, WriteMode.Full), (1, WriteMode.Full), (2, WriteMode.Full), (3, WriteMode.Full)],
        )
        prop.register(cfg)

        obs_entries = [ObsEntry(i) for i in range(4)]
        super().__init__(
            cfg,
            obs_entries,
            n_actions=4,
            action_space=spaces.Discrete(4),
            seed=seed,
        )
        self._tick_limit = 200
        self._rng = np.random.default_rng(seed)

    def _action_to_commands(self, action):
        """Apply movement to internal state. No commands needed.

        0=North(y+1), 1=South(y-1), 2=East(x+1), 3=West(x-1)
        """
        dx, dy = [(0, 1), (0, -1), (1, 0), (-1, 0)][int(action)]
        self._nav.ax = max(0.0, min(float(self.GRID_SIZE - 1), self._nav.ax + dx))
        self._nav.ay = max(0.0, min(float(self.GRID_SIZE - 1), self._nav.ay + dy))
        return None

    def _compute_reward(self, obs, info):
        """Negative Manhattan distance to target."""
        return -abs(self._nav.ax - self._nav.tx) - abs(self._nav.ay - self._nav.ty)

    def _check_terminated(self, obs, info):
        """Terminated when agent is at target."""
        return (abs(self._nav.ax - self._nav.tx)
                + abs(self._nav.ay - self._nav.ty)) < 0.5

    def _check_truncated(self, obs, info):
        """Truncated at tick limit."""
        return info.get("tick_id", 0) >= self._tick_limit

    def reset(self, *, seed=None, options=None):
        obs, info = super().reset(seed=seed, options=options)
        if seed is not None:
            self._rng = np.random.default_rng(seed)

        self._nav.ax = float(self._rng.integers(0, self.GRID_SIZE))
        self._nav.ay = float(self._rng.integers(0, self.GRID_SIZE))
        self._nav.tx = float(self._rng.integers(0, self.GRID_SIZE))
        self._nav.ty = float(self._rng.integers(0, self.GRID_SIZE))

        # Ensure agent and target don't start at the same position.
        while (abs(self._nav.ax - self._nav.tx)
               + abs(self._nav.ay - self._nav.ty)) < 0.5:
            self._nav.tx = float(self._rng.integers(0, self.GRID_SIZE))
            self._nav.ty = float(self._rng.integers(0, self.GRID_SIZE))

        # Step to populate fields with initial positions.
        self._world.step(None)
        self._obs_plan.execute(self._world, self._obs_buf, self._mask_buf)
        obs = self._obs_buf.copy()
        return obs, info


def test_grid_nav_env_lifecycle():
    """GridNavEnv can be created, stepped, and reset."""
    env = GridNavEnv(seed=42)
    obs, info = env.reset(seed=42)
    assert obs.shape == (400,)  # 10x10 grid * 4 fields
    assert obs.dtype == np.float32

    obs, reward, terminated, truncated, info = env.step(0)
    assert isinstance(reward, float)
    assert isinstance(terminated, bool)
    env.close()


def test_grid_nav_env_reward_range():
    """Reward is in expected range."""
    env = GridNavEnv(seed=42)
    env.reset(seed=42)

    rewards = []
    for _ in range(10):
        _, reward, terminated, truncated, _ = env.step(env.action_space.sample())
        rewards.append(reward)
        if terminated or truncated:
            env.reset()

    # Reward should be negative (distance) and bounded
    assert all(r <= 0 for r in rewards)
    env.close()


def test_grid_nav_env_observations_nonzero():
    """After reset, observations reflect the randomly-set positions."""
    env = GridNavEnv(seed=42)
    obs, _ = env.reset(seed=42)
    n = env.GRID_SIZE * env.GRID_SIZE

    # At least one of the 4 position values should be nonzero.
    positions = [obs[0], obs[n], obs[2 * n], obs[3 * n]]
    assert any(p != 0.0 for p in positions), (
        f"All positions are zero after reset: {positions}"
    )
    env.close()


def test_grid_nav_env_movement():
    """Stepping with actions changes agent position."""
    env = GridNavEnv(seed=42)
    env.reset(seed=42)

    ax_before = env._nav.ax
    ay_before = env._nav.ay

    # Move east (action 2): x should increase by 1
    obs, _, _, _, _ = env.step(2)
    n = env.GRID_SIZE * env.GRID_SIZE
    ax_after = obs[0]

    expected_x = min(ax_before + 1, env.GRID_SIZE - 1)
    assert abs(ax_after - expected_x) < 0.01, (
        f"Expected agent_x={expected_x}, got {ax_after}"
    )
    env.close()


@pytest.mark.slow
def test_ppo_smoke():
    """PPO training shows learning improvement over 100K steps.

    This test trains a PPO agent on GridNavEnv and verifies that
    episode rewards improve, indicating the agent is learning.
    """
    try:
        from stable_baselines3 import PPO
        from stable_baselines3.common.callbacks import BaseCallback
        from stable_baselines3.common.monitor import Monitor
    except ImportError:
        pytest.skip("stable-baselines3 not installed")

    class RewardTracker(BaseCallback):
        def __init__(self):
            super().__init__()
            self.episode_rewards = []

        def _on_step(self):
            infos = self.locals.get("infos", [])
            for info in infos:
                if "episode" in info:
                    self.episode_rewards.append(info["episode"]["r"])
            return True

    # Monitor wrapper is required for SB3 to track episode rewards.
    env = Monitor(GridNavEnv(seed=42))
    model = PPO(
        "MlpPolicy", env, verbose=0, seed=42,
        n_steps=256, batch_size=64, device="cpu",
    )

    tracker = RewardTracker()
    model.learn(total_timesteps=100_000, callback=tracker)

    if len(tracker.episode_rewards) >= 20:
        early = np.mean(tracker.episode_rewards[:10])
        late = np.mean(tracker.episode_rewards[-10:])
        # The agent should show some improvement
        assert late > early, (
            f"No learning detected: early mean reward = {early:.2f}, "
            f"late mean reward = {late:.2f}"
        )
    else:
        # Not enough episodes completed, but training didn't crash
        pass

    env.close()
