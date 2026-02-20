"""Batched Heat Seeker: migrating from single-env to BatchedVecEnv.

This example ports the original heat_seeker demo to the batched engine.
Instead of N independent Python-wrapped worlds stepped in a loop, all N
worlds are stepped in a single Rust call with one GIL release.

Key changes from the single-env version:
  - Config factory pattern: each world gets its own Config
  - Vectorized agent state: numpy arrays instead of scalar (x, y)
  - Vectorized rewards: fancy indexing across (N, obs_len) matrix
  - Single FFI call: step_and_observe replaces N step+observe cycles

Usage:
    cd crates/murk-python && maturin develop --release
    python examples/batched_heat_seeker/batched_heat_seeker.py
"""

from __future__ import annotations

import time
from typing import Any

import numpy as np

import murk
from murk import (
    BatchedVecEnv,
    Command,
    Config,
    EdgeBehavior,
    FieldMutability,
    FieldType,
    ObsEntry,
    RegionType,
    WriteMode,
)

# ─── World parameters (identical to heat_seeker) ─────────────────

GRID_W, GRID_H = 16, 16
CELL_COUNT = GRID_W * GRID_H

SOURCE_X, SOURCE_Y = 14, 14

DIFFUSION_COEFF = 0.1
SOURCE_INTENSITY = 10.0
HEAT_DECAY = 0.005

HEAT_FIELD = 0
AGENT_FIELD = 1

MAX_STEPS = 200
WARMUP_TICKS = 50

REWARD_SCALE = 0.1
TERMINAL_BONUS = 100.0
STEP_PENALTY = 1.0


# ─── Propagator (identical to heat_seeker) ────────────────────────

def diffusion_step(reads, reads_prev, writes, tick_id, dt, cell_count):
    """Discrete Laplacian diffusion with a fixed heat source."""
    prev = reads_prev[0].reshape(GRID_H, GRID_W)
    out = writes[0]

    padded = np.pad(prev, 1, mode="edge")
    laplacian = (
        padded[:-2, 1:-1]
        + padded[2:, 1:-1]
        + padded[1:-1, :-2]
        + padded[1:-1, 2:]
        - 4.0 * prev
    )

    new_heat = prev + DIFFUSION_COEFF * dt * laplacian - HEAT_DECAY * dt * prev
    new_heat[SOURCE_Y, SOURCE_X] = SOURCE_INTENSITY
    np.maximum(new_heat, 0.0, out=new_heat)
    out[:] = new_heat.ravel()


# ─── Movement deltas (precomputed) ───────────────────────────────
# 0=stay, 1=north(y-1), 2=south(y+1), 3=west(x-1), 4=east(x+1)

_DX = np.array([0, 0, 0, -1, 1], dtype=np.int32)
_DY = np.array([0, -1, 1, 0, 0], dtype=np.int32)
N_ACTIONS = 5


# ─── Batched environment ─────────────────────────────────────────
#
# Migration from MurkEnv (single world):
#   MurkEnv subclass        →  BatchedVecEnv subclass
#   self._agent_x: int      →  self._agent_x: np.ndarray (N,)
#   _action_to_commands()   →  vectorized in step()
#   _compute_reward()       →  vectorized numpy fancy-indexing
#   _check_terminated()     →  vectorized boolean comparison
#   DummyVecEnv([env]*N)    →  BatchedHeatSeekerEnv(num_envs=N)
#
# The hot path (step + observe) is now a single Rust call.
# Agent movement, reward computation, and termination checking
# stay in Python but operate on numpy arrays across all N worlds.
#

class BatchedHeatSeekerEnv(BatchedVecEnv):
    """N-world batched heat seeker: all worlds step in one Rust call."""

    def __init__(self, num_envs: int = 8, base_seed: int = 42):
        # ── Config factory ────────────────────────────────────
        #
        # Each world needs its own Config because Python propagators
        # (closures) can't be cloned. The factory is called N times.
        #
        def make_config(i: int) -> Config:
            config = Config()
            config.set_space_square4(GRID_W, GRID_H, EdgeBehavior.Absorb)
            config.add_field("heat", FieldType.Scalar, FieldMutability.PerTick)
            config.add_field("agent_pos", FieldType.Scalar, FieldMutability.PerTick)
            config.set_dt(1.0)
            config.set_seed(base_seed + i)
            murk.add_propagator(
                config,
                name="diffusion",
                step_fn=diffusion_step,
                reads_previous=[HEAT_FIELD],
                writes=[(HEAT_FIELD, WriteMode.Full)],
            )
            return config

        obs_entries = [
            ObsEntry(HEAT_FIELD, region_type=RegionType.All),
            ObsEntry(AGENT_FIELD, region_type=RegionType.All),
        ]

        super().__init__(
            config_factory=make_config,
            obs_entries=obs_entries,
            num_envs=num_envs,
        )

        self._base_seed = base_seed

        # ── Vectorized agent state ────────────────────────────
        # Instead of scalar self._agent_x, we track N positions.
        self._agent_x = np.zeros(num_envs, dtype=np.int32)
        self._agent_y = np.zeros(num_envs, dtype=np.int32)
        self._steps_in_episode = np.zeros(num_envs, dtype=np.int32)
        self._episode_counts = np.zeros(num_envs, dtype=np.int32)

    def reset(
        self, *, seed: int | list[int] | None = None
    ) -> tuple[np.ndarray, dict]:
        """Reset all worlds, run warmup, place agents randomly.

        The warmup phase steps all worlds with empty commands so the
        heat gradient reaches approximate steady state before agents
        start navigating.
        """
        if seed is None:
            seeds = [self._base_seed + i for i in range(self.num_envs)]
        elif isinstance(seed, int):
            seeds = [seed + i for i in range(self.num_envs)]
        else:
            seeds = list(seed)

        self._engine.reset_all(seeds)

        # ── Warmup: let heat diffuse to near-steady-state ─────
        #
        # After reset, all fields are zero. We step WARMUP_TICKS
        # times with no commands to build the heat gradient.
        # This is the batch version of the single-env warmup loop.
        #
        empty_cmds: list[list[Any]] = [[] for _ in range(self.num_envs)]
        for _ in range(WARMUP_TICKS):
            self._engine.step_and_observe(empty_cmds, self._obs_flat, self._mask_flat)

        # ── Place agents at random positions (vectorized) ─────
        rng = np.random.default_rng(seeds[0])
        self._agent_x[:] = rng.integers(0, GRID_W, size=self.num_envs)
        self._agent_y[:] = rng.integers(0, GRID_H, size=self.num_envs)
        self._steps_in_episode[:] = 0
        self._episode_counts[:] += 1

        # Stamp all agent positions in one batch step.
        stamp_cmds = self._make_agent_stamp_commands()
        self._engine.step_and_observe(stamp_cmds, self._obs_flat, self._mask_flat)

        obs = self._obs_flat.reshape(self.num_envs, self._obs_per_world).copy()
        return obs, {}

    def step(
        self, actions: np.ndarray
    ) -> tuple[np.ndarray, np.ndarray, np.ndarray, np.ndarray, dict]:
        """Step all worlds, compute vectorized rewards, auto-reset.

        This replaces the per-env step loop in MurkVecEnv. The key
        differences:
          - Agent positions updated with vectorized numpy ops
          - step_and_observe is a single Rust call (1 GIL release)
          - Rewards computed via fancy indexing across (N, obs_len)
          - Termination is a vectorized boolean comparison
        """
        acts = np.asarray(actions, dtype=np.int32).ravel()

        # ── Vectorized agent movement ─────────────────────────
        # In the single-env version this was:
        #   dx, dy = deltas[action]
        #   self._agent_x = max(0, min(W-1, self._agent_x + dx))
        #
        # Batched version: index into precomputed delta arrays.
        self._agent_x[:] = np.clip(self._agent_x + _DX[acts], 0, GRID_W - 1)
        self._agent_y[:] = np.clip(self._agent_y + _DY[acts], 0, GRID_H - 1)
        self._steps_in_episode += 1

        # ── Build per-world commands ──────────────────────────
        # Each world gets a SetField command stamping the agent.
        # This is the one part that can't be fully vectorized since
        # Command objects are Python objects.
        commands = self._make_agent_stamp_commands()

        # ── Single Rust call: step all + observe all ──────────
        tick_ids = self._engine.step_and_observe(
            commands, self._obs_flat, self._mask_flat
        )
        obs = self._obs_flat.reshape(self.num_envs, self._obs_per_world)

        # ── Vectorized reward computation ─────────────────────
        #
        # In the single-env version:
        #   heat = obs[:CELL_COUNT]
        #   agent_idx = self._agent_y * GRID_W + self._agent_x
        #   reward = REWARD_SCALE * heat[agent_idx] - STEP_PENALTY
        #
        # Batched version: fancy-index into (N, 256) heat matrix.
        heat = obs[:, :CELL_COUNT]                              # (N, 256)
        agent_indices = self._agent_y * GRID_W + self._agent_x  # (N,)
        heat_at_agent = heat[np.arange(self.num_envs), agent_indices]

        # ── Vectorized termination ────────────────────────────
        terminated = (self._agent_x == SOURCE_X) & (self._agent_y == SOURCE_Y)
        truncated = self._steps_in_episode >= MAX_STEPS

        rewards = REWARD_SCALE * heat_at_agent - STEP_PENALTY
        rewards[terminated] += TERMINAL_BONUS

        # ── Auto-reset terminated/truncated worlds ────────────
        #
        # Design choice: we skip warmup on auto-reset. The heat
        # gradient rebuilds within ~50 ticks, so the agent sees a
        # weak gradient briefly. This is a practical trade-off:
        # full warmup would require stepping ALL worlds (the batch
        # engine doesn't support per-world stepping).
        #
        final_observations: list[np.ndarray | None] = [None] * self.num_envs
        needs_reset = terminated | truncated

        if needs_reset.any():
            for i in np.where(needs_reset)[0]:
                final_observations[i] = obs[i].copy()

                new_seed = (
                    self._base_seed
                    + int(self._episode_counts[i]) * self.num_envs
                    + i
                )
                self._engine.reset_world(i, new_seed)

                # Re-place agent for next episode.
                rng = np.random.default_rng(new_seed)
                self._agent_x[i] = int(rng.integers(0, GRID_W))
                self._agent_y[i] = int(rng.integers(0, GRID_H))
                self._steps_in_episode[i] = 0
                self._episode_counts[i] += 1

            # Re-observe to get fresh obs for reset worlds.
            self._engine.observe_all(self._obs_flat, self._mask_flat)
            obs = self._obs_flat.reshape(self.num_envs, self._obs_per_world)

            # Patch agent_pos for reset worlds: observe_all sees zeroed
            # fields because reset_world clears everything and there is
            # no engine API to apply commands without stepping.  The
            # next step() call will stamp the correct position via
            # _make_agent_stamp_commands before step_and_observe.
            for i in np.where(needs_reset)[0]:
                cell_idx = int(self._agent_y[i]) * GRID_W + int(self._agent_x[i])
                obs[i, CELL_COUNT + cell_idx] = 1.0

        infos: dict[str, Any] = {
            "final_observation": final_observations,
            "tick_ids": list(tick_ids),
        }

        return (
            obs.copy(),
            rewards.astype(np.float64),
            terminated.copy(),
            truncated.copy(),
            infos,
        )

    def _make_agent_stamp_commands(self) -> list[list[Command]]:
        """Build SetField commands to stamp agent positions into each world."""
        cmds: list[list[Command]] = []
        for i in range(self.num_envs):
            cmds.append([
                Command.set_field(
                    AGENT_FIELD,
                    [int(self._agent_y[i]), int(self._agent_x[i])],
                    1.0,
                )
            ])
        return cmds


# ─── Performance comparison ──────────────────────────────────────
#
# The original heat_seeker uses DummyVecEnv (Python loop over N envs).
# The batched version uses BatchedVecEnv (single Rust call for N envs).
# This comparison measures the raw step throughput difference.
#

def benchmark_batched(num_envs: int, num_steps: int) -> float:
    """Benchmark BatchedHeatSeekerEnv. Returns steps/sec."""
    env = BatchedHeatSeekerEnv(num_envs=num_envs, base_seed=42)
    env.reset(seed=42)

    rng = np.random.default_rng(0)
    t0 = time.perf_counter()
    for _ in range(num_steps):
        actions = rng.integers(0, N_ACTIONS, size=num_envs)
        env.step(actions)
    elapsed = time.perf_counter() - t0

    env.close()
    total_world_steps = num_envs * num_steps
    return total_world_steps / elapsed


def benchmark_vecenv(num_envs: int, num_steps: int) -> float:
    """Benchmark MurkVecEnv (Python loop). Returns steps/sec."""
    # Import here to avoid import order issues.
    from murk import MurkVecEnv

    # Import HeatSeekerEnv from the original example.
    import sys
    import os
    example_dir = os.path.join(
        os.path.dirname(__file__), "..", "heat_seeker"
    )
    sys.path.insert(0, example_dir)
    from heat_seeker import HeatSeekerEnv
    sys.path.pop(0)

    env = MurkVecEnv([lambda i=i: HeatSeekerEnv(seed=42 + i) for i in range(num_envs)])
    env.reset(seed=[42 + i for i in range(num_envs)])

    rng = np.random.default_rng(0)
    t0 = time.perf_counter()
    for _ in range(num_steps):
        actions = rng.integers(0, N_ACTIONS, size=num_envs)
        env.step(actions)
    elapsed = time.perf_counter() - t0

    env.close()
    total_world_steps = num_envs * num_steps
    return total_world_steps / elapsed


# ─── Sample rollout ──────────────────────────────────────────────

def demo_rollout(num_envs: int = 4, num_steps: int = 30):
    """Run a short rollout and print per-step info."""
    env = BatchedHeatSeekerEnv(num_envs=num_envs, base_seed=1234)
    obs, _ = env.reset(seed=1234)

    print(f"  Initial agent positions:")
    for i in range(num_envs):
        print(f"    world {i}: ({env._agent_x[i]:2d}, {env._agent_y[i]:2d})")
    print()

    action_names = ["stay", "N", "S", "W", "E"]
    rng = np.random.default_rng(1234)

    for step in range(num_steps):
        actions = rng.integers(0, N_ACTIONS, size=num_envs)
        obs, rewards, terminateds, truncateds, infos = env.step(actions)

        # Print compact per-step summary.
        acts_str = " ".join(f"{action_names[a]:4s}" for a in actions)
        rew_str = " ".join(f"{r:6.2f}" for r in rewards)
        flags = ""
        if terminateds.any():
            flags += f" TERM={list(np.where(terminateds)[0])}"
        if truncateds.any():
            flags += f" TRUNC={list(np.where(truncateds)[0])}"

        print(f"  t={step + 1:3d}  actions=[{acts_str}]  rewards=[{rew_str}]{flags}")

    env.close()


# ─── Main ────────────────────────────────────────────────────────

def main():
    print("=" * 64)
    print("  Batched Heat Seeker: BatchedVecEnv migration demo")
    print("=" * 64)
    print()
    print(f"  Grid:        {GRID_W}x{GRID_H} ({CELL_COUNT} cells)")
    print(f"  Heat source: ({SOURCE_X}, {SOURCE_Y})")
    print(f"  Obs size:    {CELL_COUNT * 2} floats per world")
    print()

    # ── Sample rollout ────────────────────────────────────────
    print("Sample rollout (4 worlds, random actions):")
    demo_rollout(num_envs=4, num_steps=15)
    print()

    # ── Performance comparison ────────────────────────────────
    print("Performance comparison:")
    print("-" * 64)

    for num_envs in [4, 8, 16, 32]:
        num_steps = 200

        batched_sps = benchmark_batched(num_envs, num_steps)
        vecenv_sps = benchmark_vecenv(num_envs, num_steps)
        speedup = batched_sps / vecenv_sps if vecenv_sps > 0 else float("inf")

        print(
            f"  N={num_envs:3d}  "
            f"BatchedVecEnv: {batched_sps:8.0f} world-steps/s  "
            f"MurkVecEnv: {vecenv_sps:8.0f} world-steps/s  "
            f"speedup: {speedup:.2f}x"
        )

    print()
    print("Done.")


if __name__ == "__main__":
    main()
