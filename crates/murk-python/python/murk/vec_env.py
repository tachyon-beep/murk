"""MurkVecEnv: Vectorized environment with auto-reset for RL training."""

from __future__ import annotations

from typing import Any, Callable

import numpy as np
from gymnasium import spaces

from murk.env import MurkEnv


class MurkVecEnv:
    """Vectorized Murk environment with auto-reset.

    Wraps multiple MurkEnv instances. On termination/truncation, the
    environment is automatically reset and the final observation is stored
    in ``info["final_observation"]``.

    Follows **Gymnasium** vectorized-env conventions:

    - ``reset()`` returns ``(obs, infos)``
    - ``step()`` returns ``(obs, rewards, terminateds, truncateds, infos)``

    This is **not** directly compatible with stable-baselines3's ``VecEnv``,
    which expects a 4-tuple step return and obs-only reset.  Use SB3's
    ``DummyVecEnv`` wrapper or a compatibility shim if you need SB3 integration.

    Args:
        env_fns: List of callables, each returning a MurkEnv instance.
    """

    def __init__(self, env_fns: list[Callable[[], MurkEnv]]):
        self.envs = [fn() for fn in env_fns]
        assert len(self.envs) > 0, "Need at least one environment"

        self.num_envs = len(self.envs)
        self.observation_space = self.envs[0].observation_space
        self.action_space = self.envs[0].action_space

        obs_shape = self.observation_space.shape
        self._observations = np.zeros(
            (self.num_envs, *obs_shape), dtype=np.float32
        )
        self._rewards = np.zeros(self.num_envs, dtype=np.float64)
        self._terminateds = np.zeros(self.num_envs, dtype=bool)
        self._truncateds = np.zeros(self.num_envs, dtype=bool)

    def reset(
        self,
        *,
        seed: int | list[int] | None = None,
        options: dict | None = None,
    ) -> tuple[np.ndarray, dict]:
        """Reset all environments."""
        infos: dict[str, Any] = {}
        for i, env in enumerate(self.envs):
            env_seed = seed[i] if isinstance(seed, list) else seed
            obs, info = env.reset(seed=env_seed, options=options)
            self._observations[i] = obs
        return self._observations.copy(), infos

    def step(
        self, actions: np.ndarray
    ) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray, dict]:
        """Step all environments, auto-resetting on termination/truncation."""
        final_observations = [None] * self.num_envs
        final_infos = [None] * self.num_envs

        for i, env in enumerate(self.envs):
            obs, reward, terminated, truncated, info = env.step(actions[i])
            self._rewards[i] = reward
            self._terminateds[i] = terminated
            self._truncateds[i] = truncated

            if terminated or truncated:
                final_observations[i] = obs.copy()
                final_infos[i] = info.copy()
                obs, _ = env.reset()

            self._observations[i] = obs

        info_dict: dict[str, Any] = {
            "final_observation": final_observations,
            "final_info": final_infos,
        }

        return (
            self._observations.copy(),
            self._rewards.copy(),
            self._terminateds.copy(),
            self._truncateds.copy(),
            info_dict,
        )

    def close(self):
        """Close all environments."""
        for env in self.envs:
            env.close()

    def render(self):
        """Rendering not supported."""
        pass
