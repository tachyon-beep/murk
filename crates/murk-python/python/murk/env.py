"""MurkEnv: Gymnasium-compatible single-environment adapter.

Designed for subclassing — override ``_action_to_commands``,
``_compute_reward``, ``_check_terminated``, and ``_check_truncated``
to define environment-specific behavior.
"""

from __future__ import annotations

from typing import Any, Optional

import gymnasium
import numpy as np
from gymnasium import spaces

from murk._murk import Command, Config, ObsEntry, ObsPlan, World


class MurkEnv(gymnasium.Env):
    """Base Gymnasium environment backed by a Murk simulation world.

    Subclass this and override the hook methods to create a custom
    environment. The base class handles world lifecycle, observation
    extraction, and the Gymnasium protocol.

    Args:
        config: A fully-configured Config (consumed by World creation).
        obs_entries: List of ObsEntry defining what to observe.
        n_actions: Number of discrete actions (for default Discrete space).
        action_space: Override the default Discrete action space.
        seed: Initial RNG seed.
    """

    metadata = {"render_modes": []}

    def __init__(
        self,
        config: Config,
        obs_entries: list[ObsEntry],
        n_actions: int = 1,
        action_space: spaces.Space | None = None,
        seed: int = 0,
    ):
        super().__init__()

        # Create world (consumes config).
        self._world = World(config)

        # Compile observation plan.
        self._obs_plan = ObsPlan(self._world, obs_entries)

        # Pre-allocate reusable numpy buffers.
        self._obs_buf = np.zeros(self._obs_plan.output_len, dtype=np.float32)
        self._mask_buf = np.zeros(self._obs_plan.mask_len, dtype=np.uint8)

        # Gymnasium spaces.
        self.observation_space = spaces.Box(
            low=-np.inf,
            high=np.inf,
            shape=(self._obs_plan.output_len,),
            dtype=np.float32,
        )
        self.action_space = action_space or spaces.Discrete(n_actions)

        self._seed = seed
        self._last_step_metrics = None
        self._tick_limit = 0  # 0 = no truncation by default

    def step(self, action: Any) -> tuple[np.ndarray, float, bool, bool, dict]:
        """Execute one environment step.

        Converts the action to commands, steps the world, extracts
        observations, and computes reward/termination signals.
        """
        commands = self._action_to_commands(action)

        receipts, metrics = self._world.step(commands)
        self._last_step_metrics = metrics

        tick_id, age_ticks = self._obs_plan.execute(
            self._world, self._obs_buf, self._mask_buf
        )

        obs = self._obs_buf.copy()
        info: dict[str, Any] = {
            "tick_id": tick_id,
            "age_ticks": age_ticks,
        }

        reward = float(self._compute_reward(obs, info))
        terminated = bool(self._check_terminated(obs, info))
        truncated = bool(self._check_truncated(obs, info))

        return obs, reward, terminated, truncated, info

    def reset(
        self,
        *,
        seed: int | None = None,
        options: dict | None = None,
    ) -> tuple[np.ndarray, dict]:
        """Reset the environment to initial state."""
        super().reset(seed=seed, options=options)
        if seed is not None:
            self._seed = seed
        self._world.reset(self._seed)

        # Step once to populate initial field data.
        self._world.step(None)

        tick_id, age_ticks = self._obs_plan.execute(
            self._world, self._obs_buf, self._mask_buf
        )
        obs = self._obs_buf.copy()
        info: dict[str, Any] = {"tick_id": tick_id, "age_ticks": age_ticks}
        return obs, info

    @property
    def last_step_metrics(self):
        """StepMetrics from the most recent step() call."""
        return self._last_step_metrics

    # ── Override hooks ────────────────────────────────────────

    def _action_to_commands(self, action: Any) -> list[Command] | None:
        """Convert an action to a list of Murk commands.

        Override this in your subclass. The default returns no commands.
        """
        return None

    def _compute_reward(self, obs: np.ndarray, info: dict) -> float:
        """Compute the reward for the current step.

        Override this in your subclass. The default returns 0.
        """
        return 0.0

    def _check_terminated(self, obs: np.ndarray, info: dict) -> bool:
        """Check if the episode has terminated (goal reached, failure, etc.).

        Override this in your subclass. The default returns False.
        """
        return False

    def _check_truncated(self, obs: np.ndarray, info: dict) -> bool:
        """Check if the episode should be truncated (time limit, etc.).

        Override this in your subclass. The default checks tick_limit.
        """
        if self._tick_limit > 0:
            return info.get("tick_id", 0) >= self._tick_limit
        return False

    def close(self):
        """Clean up resources."""
        if hasattr(self, "_world") and self._world is not None:
            self._world.destroy()
