"""BatchedVecEnv: High-performance vectorized environment using the Rust batched engine."""

from __future__ import annotations

from typing import Any, Callable

import numpy as np

from murk._murk import BatchedWorld, Config, ObsEntry


class BatchedVecEnv:
    """Vectorized environment backed by a Rust BatchedEngine.

    Steps all worlds and extracts observations in a single Rust call with
    one GIL release, eliminating the per-world FFI overhead of MurkVecEnv.

    Subclass and override the hook methods to customize for your RL task:
    - ``_actions_to_commands``: convert action array to per-world command lists
    - ``_compute_rewards``: compute per-world rewards from observations
    - ``_check_terminated``: check per-world termination conditions
    - ``_check_truncated``: check per-world truncation conditions

    Args:
        config_factory: Callable taking a world index (int) and returning
            a Config. Called ``num_envs`` times to build independent configs.
        obs_entries: List of ObsEntry describing the observation spec.
        num_envs: Number of parallel environments.
    """

    def __init__(
        self,
        config_factory: Callable[[int], Config],
        obs_entries: list[ObsEntry],
        num_envs: int,
    ):
        assert num_envs > 0, "num_envs must be >= 1"

        configs = [config_factory(i) for i in range(num_envs)]
        self._engine = BatchedWorld(configs, obs_entries)

        self.num_envs = self._engine.num_worlds
        self._obs_per_world = self._engine.obs_output_len
        self._mask_per_world = self._engine.obs_mask_len

        total_obs = self.num_envs * self._obs_per_world
        total_mask = self.num_envs * self._mask_per_world

        # Pre-allocated buffers (reused every step).
        self._obs_flat = np.zeros(total_obs, dtype=np.float32)
        self._mask_flat = np.zeros(total_mask, dtype=np.uint8)
        self._rewards = np.zeros(self.num_envs, dtype=np.float64)
        self._terminateds = np.zeros(self.num_envs, dtype=bool)
        self._truncateds = np.zeros(self.num_envs, dtype=bool)
        self._tick_ids = np.zeros(self.num_envs, dtype=np.uint64)

    @property
    def obs_output_len(self) -> int:
        """Per-world observation output length (f32 elements)."""
        return self._obs_per_world

    @property
    def obs_mask_len(self) -> int:
        """Per-world observation mask length (bytes)."""
        return self._mask_per_world

    def reset(
        self,
        *,
        seed: int | list[int] | None = None,
    ) -> tuple[np.ndarray, dict]:
        """Reset all environments and return initial observations.

        Args:
            seed: Optional seed(s) for reset. If int, used for all worlds.
                If list, must have ``num_envs`` elements.

        Returns:
            Tuple of (observations, info_dict).
            Observations shape: (num_envs, obs_output_len).
        """
        if seed is None:
            seeds = list(range(self.num_envs))
        elif isinstance(seed, int):
            seeds = [seed + i for i in range(self.num_envs)]
        else:
            seeds = list(seed)

        self._engine.reset_all(seeds)

        # Extract initial observations.
        self._engine.observe_all(self._obs_flat, self._mask_flat)

        obs = self._obs_flat.reshape(self.num_envs, self._obs_per_world).copy()
        return obs, {}

    def step(
        self, actions: np.ndarray
    ) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray, dict]:
        """Step all environments with auto-reset.

        Args:
            actions: Action array, shape depends on ``_actions_to_commands``.

        Returns:
            Tuple of (obs, rewards, terminateds, truncateds, infos).
            obs shape: (num_envs, obs_output_len).
        """
        # Convert actions to per-world command lists.
        commands = self._actions_to_commands(actions)

        # Single Rust call: step all worlds + extract all observations.
        tick_ids = self._engine.step_and_observe(
            commands, self._obs_flat, self._mask_flat
        )
        for i, tid in enumerate(tick_ids):
            self._tick_ids[i] = tid

        obs = self._obs_flat.reshape(self.num_envs, self._obs_per_world)

        # Compute rewards, termination, truncation (user overrides).
        self._rewards[:] = self._compute_rewards(obs, self._tick_ids)
        self._terminateds[:] = self._check_terminated(obs, self._tick_ids)
        self._truncateds[:] = self._check_truncated(obs, self._tick_ids)

        # Auto-reset terminated/truncated worlds.
        final_observations: list[np.ndarray | None] = [None] * self.num_envs
        final_infos: list[dict | None] = [None] * self.num_envs

        needs_reset = self._terminateds | self._truncateds
        for i in range(self.num_envs):
            if needs_reset[i]:
                final_observations[i] = obs[i].copy()
                final_infos[i] = {}
                self._engine.reset_world(i, int(self._tick_ids[i]))
                # Re-observe this world (fills its slice in _obs_flat).
                # We do a full observe_all after the loop instead.

        if needs_reset.any():
            # Re-extract observations for reset worlds.
            self._engine.observe_all(self._obs_flat, self._mask_flat)
            obs = self._obs_flat.reshape(self.num_envs, self._obs_per_world)

        info_dict: dict[str, Any] = {
            "final_observation": final_observations,
            "final_info": final_infos,
            "tick_ids": self._tick_ids.copy(),
        }

        return (
            obs.copy(),
            self._rewards.copy(),
            self._terminateds.copy(),
            self._truncateds.copy(),
            info_dict,
        )

    def close(self) -> None:
        """Destroy the batched engine (idempotent)."""
        if self._engine is not None:
            self._engine.destroy()
            self._engine = None

    # ── Override hooks ───────────────────────────────────────────

    def _actions_to_commands(self, actions: np.ndarray) -> list[list]:
        """Convert an action array to per-world command lists.

        Default: empty commands for all worlds (no-op actions).
        Override this for your RL task.

        Args:
            actions: Action array from the RL agent.

        Returns:
            List of num_envs lists of Command objects.
        """
        return [[] for _ in range(self.num_envs)]

    def _compute_rewards(
        self, obs: np.ndarray, tick_ids: np.ndarray
    ) -> np.ndarray:
        """Compute per-world rewards from observations.

        Default: zero reward.
        Override this for your RL task.

        Args:
            obs: Observation array, shape (num_envs, obs_output_len).
            tick_ids: Per-world tick IDs, shape (num_envs,).

        Returns:
            Rewards array, shape (num_envs,).
        """
        return np.zeros(self.num_envs, dtype=np.float64)

    def _check_terminated(
        self, obs: np.ndarray, tick_ids: np.ndarray
    ) -> np.ndarray:
        """Check per-world termination conditions.

        Default: never terminated.
        Override this for your RL task.

        Args:
            obs: Observation array, shape (num_envs, obs_output_len).
            tick_ids: Per-world tick IDs, shape (num_envs,).

        Returns:
            Boolean array, shape (num_envs,).
        """
        return np.zeros(self.num_envs, dtype=bool)

    def _check_truncated(
        self, obs: np.ndarray, tick_ids: np.ndarray
    ) -> np.ndarray:
        """Check per-world truncation conditions.

        Default: never truncated.
        Override this for your RL task.

        Args:
            obs: Observation array, shape (num_envs, obs_output_len).
            tick_ids: Per-world tick IDs, shape (num_envs,).

        Returns:
            Boolean array, shape (num_envs,).
        """
        return np.zeros(self.num_envs, dtype=bool)
