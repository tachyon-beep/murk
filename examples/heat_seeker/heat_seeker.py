"""Heat Seeker: a minimal Murk + PPO tech demo.

A 16x16 grid world with a heat source in one corner. Diffusion spreads
warmth each tick, creating a gradient. A PPO agent learns to navigate
toward the heat by observing the temperature field and its own position.

Usage:
    cd crates/murk-python && maturin develop --release
    pip install stable-baselines3
    python examples/heat_seeker/heat_seeker.py
"""

from __future__ import annotations

import time
from typing import Any

import numpy as np
from gymnasium import spaces
from stable_baselines3 import PPO
from stable_baselines3.common.vec_env import DummyVecEnv

import murk
from murk import Command, Config, ObsEntry, SpaceType, FieldMutability, FieldType

# ─── World parameters ───────────────────────────────────────────
#
# Decision: grid size.
#   8x8   → too small, agent reaches goal by luck
#   16x16 → 256 cells, enough that random walk rarely finds the source
#   64x64 → works but slower training, no benefit for a demo
#
GRID_W, GRID_H = 16, 16
CELL_COUNT = GRID_W * GRID_H

# Decision: heat source placement.
#   Corner placement maximises average distance from random starts,
#   giving the clearest learning signal.
#
SOURCE_X, SOURCE_Y = 14, 14

# Decision: diffusion coefficient.
#   D=0.1 with dt=1.0 gives CFL number = 4*D*dt = 0.4 (stable, < 1).
#   Higher D spreads heat faster → flatter gradient → harder to learn.
#   Lower D concentrates heat near source → sparse signal far away.
#
DIFFUSION_COEFF = 0.1
SOURCE_INTENSITY = 10.0

# Field IDs (assigned by add_field order).
HEAT_FIELD = 0
AGENT_FIELD = 1

# Episode length. 200 steps is enough to cross the grid diagonally
# (worst case ~30 steps) with room for exploration.
MAX_STEPS = 200

# Number of empty ticks to run on reset, letting heat diffuse to
# approximate steady state before the agent starts.
WARMUP_TICKS = 50

# Training budget.
TOTAL_TIMESTEPS = 50_000


# ─── Propagator: discrete diffusion ─────────────────────────────
#
# Decision: Rust propagator vs Python propagator.
#   Rust propagators are faster (no GIL round-trip), but require
#   recompilation when you change the logic. For prototyping and
#   demos, Python propagators let you iterate without rebuilding.
#
#   Murk's trampoline system copies field buffers to numpy arrays,
#   calls your function, then copies results back. This means you
#   can use any numpy operation inside a propagator.
#
# Decision: Euler (reads current tick) vs Jacobi (reads previous tick).
#   Diffusion should read the *frozen* previous-tick state so that
#   cell updates don't depend on the order cells are visited. This
#   is the Jacobi style. We declare reads_previous=[HEAT_FIELD].
#

def diffusion_step(reads, reads_prev, writes, tick_id, dt, cell_count):
    """Discrete Laplacian diffusion with a fixed heat source.

    reads_prev[0]: previous tick's heat field (flat, row-major)
    writes[0]:     output heat field (starts at zero for PerTick fields)
    """
    prev = reads_prev[0].reshape(GRID_H, GRID_W)
    out = writes[0]

    # Pad with edge values (absorb boundaries: zero flux at edges).
    padded = np.pad(prev, 1, mode="edge")

    # 4-connected discrete Laplacian.
    laplacian = (
        padded[:-2, 1:-1]   # north
        + padded[2:, 1:-1]  # south
        + padded[1:-1, :-2] # west
        + padded[1:-1, 2:]  # east
        - 4.0 * prev
    )

    new_heat = prev + DIFFUSION_COEFF * dt * laplacian
    new_heat[SOURCE_Y, SOURCE_X] = SOURCE_INTENSITY
    np.maximum(new_heat, 0.0, out=new_heat)

    out[:] = new_heat.ravel()


# ─── Environment ─────────────────────────────────────────────────
#
# Decision: MurkEnv subclass vs raw Gymnasium.
#   MurkEnv handles world lifecycle, observation extraction via
#   ObsPlan, and the Gymnasium protocol. We override four hooks:
#     _action_to_commands  → translate discrete action to Murk commands
#     _compute_reward      → reward = heat at agent position
#     _check_terminated    → true when agent reaches the source
#     _check_truncated     → true when MAX_STEPS exceeded
#
# Decision: observation space.
#   Option A: full 16x16 heat grid + 16x16 agent position = 512 floats.
#   Option B: local 5x5 patch around agent = 50 floats (needs AgentDisk).
#   Option C: just (agent_x, agent_y, heat_at_agent) = 3 floats.
#
#   We use option A. It's the simplest to wire (two ObsEntry with
#   region_type=All), and 512 floats is trivial for PPO's MLP.
#   Option B would be better for larger grids; Option C would need
#   a custom observation instead of ObsPlan.
#
# Decision: action space.
#   5 discrete actions: stay, north, south, west, east.
#   We track agent position in Python and stamp it into the
#   agent_pos field via a SetField command each tick.
#

class HeatSeekerEnv(murk.MurkEnv):
    """16x16 grid: learn to navigate toward a heat source."""

    def __init__(self, seed: int = 0):
        config = Config()

        # Space: 16x16 Square4 grid with absorb boundaries.
        # params = [width, height, edge_behavior] where 0=Absorb.
        config.set_space(SpaceType.Square4, [float(GRID_W), float(GRID_H), 0.0])

        # Fields: heat (the gradient signal) and agent_pos (binary mask).
        # Both are PerTick (fresh allocation each tick, starts at zero).
        config.add_field("heat", FieldType.Scalar, FieldMutability.PerTick)
        config.add_field("agent_pos", FieldType.Scalar, FieldMutability.PerTick)

        config.set_dt(1.0)
        config.set_seed(seed)

        # Register the diffusion propagator.
        #   reads_previous=[0] → Jacobi-style read of previous tick's heat
        #   writes=[(0, 0)]    → full-write to heat field (write_mode 0=Full)
        murk.add_propagator(
            config,
            name="diffusion",
            step_fn=diffusion_step,
            reads_previous=[HEAT_FIELD],
            writes=[(HEAT_FIELD, 0)],
        )

        # Observation plan: observe both fields in full (region_type=0 = All).
        obs_entries = [
            ObsEntry(HEAT_FIELD),
            ObsEntry(AGENT_FIELD),
        ]

        super().__init__(
            config=config,
            obs_entries=obs_entries,
            n_actions=5,
            seed=seed,
        )

        self._tick_limit = MAX_STEPS
        self._agent_x = 0
        self._agent_y = 0
        self._episode_count = 0

    def reset(
        self, *, seed: int | None = None, options: dict | None = None
    ) -> tuple[np.ndarray, dict]:
        if seed is not None:
            self._seed = seed

        self._world.reset(self._seed)

        # Warmup: run empty ticks so heat diffuses to near-steady-state.
        for _ in range(WARMUP_TICKS):
            self._world.step(None)

        # Place agent at a random position.
        rng = np.random.default_rng(self._seed + self._episode_count)
        self._episode_count += 1
        self._agent_x = int(rng.integers(0, GRID_W))
        self._agent_y = int(rng.integers(0, GRID_H))

        # Stamp agent position into the field and tick once more.
        cmd = Command.set_field(AGENT_FIELD, [self._agent_x, self._agent_y], 1.0)
        self._world.step([cmd])

        # Extract initial observation.
        tick_id, age_ticks = self._obs_plan.execute(
            self._world, self._obs_buf, self._mask_buf
        )
        obs = self._obs_buf.copy()
        return obs, {"tick_id": tick_id, "age_ticks": age_ticks}

    def _action_to_commands(self, action: Any) -> list[Command]:
        # 0=stay, 1=north(y-1), 2=south(y+1), 3=west(x-1), 4=east(x+1)
        deltas = [(0, 0), (0, -1), (0, 1), (-1, 0), (1, 0)]
        dx, dy = deltas[action]
        self._agent_x = max(0, min(GRID_W - 1, self._agent_x + dx))
        self._agent_y = max(0, min(GRID_H - 1, self._agent_y + dy))
        return [Command.set_field(AGENT_FIELD, [self._agent_x, self._agent_y], 1.0)]

    def _compute_reward(self, obs: np.ndarray, info: dict) -> float:
        # Reward = heat value at the agent's current position.
        # obs layout: [heat_field (256 floats), agent_field (256 floats)]
        heat = obs[:CELL_COUNT]
        agent_idx = self._agent_y * GRID_W + self._agent_x
        return float(heat[agent_idx])

    def _check_terminated(self, obs: np.ndarray, info: dict) -> bool:
        return self._agent_x == SOURCE_X and self._agent_y == SOURCE_Y


# ─── Evaluation ──────────────────────────────────────────────────

def evaluate(model, n_episodes: int = 10) -> tuple[float, float, float]:
    """Run n episodes and return (mean_reward, mean_length, reach_rate)."""
    env = HeatSeekerEnv(seed=9999)
    total_rewards = []
    total_lengths = []
    reached = 0

    for ep in range(n_episodes):
        obs, _ = env.reset(seed=9999 + ep)
        episode_reward = 0.0
        steps = 0

        while True:
            action, _ = model.predict(obs, deterministic=True)
            obs, reward, terminated, truncated, _ = env.step(int(action))
            episode_reward += reward
            steps += 1
            if terminated:
                reached += 1
            if terminated or truncated:
                break

        total_rewards.append(episode_reward)
        total_lengths.append(steps)

    env.close()
    return (
        float(np.mean(total_rewards)),
        float(np.mean(total_lengths)),
        reached / n_episodes,
    )


# ─── Main ────────────────────────────────────────────────────────

def main():
    print("=" * 60)
    print("  Heat Seeker: Murk + PPO tech demo")
    print("=" * 60)
    print()
    print(f"  Grid:        {GRID_W}x{GRID_H} ({CELL_COUNT} cells)")
    print(f"  Heat source: ({SOURCE_X}, {SOURCE_Y})")
    print(f"  Actions:     5 (stay, N, S, W, E)")
    print(f"  Obs size:    {CELL_COUNT * 2} (heat grid + agent position)")
    print(f"  Training:    {TOTAL_TIMESTEPS:,} timesteps")
    print()

    # ── Create vectorized environment for PPO ────────────────
    #
    # Decision: single env vs vectorized.
    #   PPO benefits from vectorized environments (more diverse
    #   experience per update). DummyVecEnv runs them sequentially
    #   in one process — simple and sufficient for a demo.
    #   For production, use SubprocVecEnv or Murk's MurkVecEnv.
    #
    env = DummyVecEnv([lambda: HeatSeekerEnv(seed=42)])

    # ── Create PPO model ─────────────────────────────────────
    #
    # Decision: hyperparameters.
    #   Defaults work well for this problem. The only tuning:
    #   - n_steps=512: collect half an episode before each update
    #   - Small network (64x64 MLP) is plenty for 512-dim input
    #
    model = PPO(
        "MlpPolicy",
        env,
        n_steps=512,
        verbose=0,
    )

    # ── Evaluate before training (random policy baseline) ────
    print("Evaluating random policy (before training)...")
    mean_r, mean_l, reach = evaluate(model, n_episodes=10)
    print(f"  Mean reward:  {mean_r:8.1f}")
    print(f"  Mean length:  {mean_l:8.1f} steps")
    print(f"  Reach rate:   {reach:8.0%}")
    print()

    # ── Train ────────────────────────────────────────────────
    print(f"Training PPO for {TOTAL_TIMESTEPS:,} timesteps...")
    t0 = time.time()
    model.learn(total_timesteps=TOTAL_TIMESTEPS, progress_bar=True)
    elapsed = time.time() - t0
    print(f"  Done in {elapsed:.1f}s ({TOTAL_TIMESTEPS / elapsed:.0f} steps/sec)")
    print()

    # ── Evaluate after training ──────────────────────────────
    print("Evaluating trained policy (after training)...")
    mean_r, mean_l, reach = evaluate(model, n_episodes=10)
    print(f"  Mean reward:  {mean_r:8.1f}")
    print(f"  Mean length:  {mean_l:8.1f} steps")
    print(f"  Reach rate:   {reach:8.0%}")
    print()

    # ── Show a sample trajectory ─────────────────────────────
    print("Sample trajectory (trained agent):")
    demo_env = HeatSeekerEnv(seed=1234)
    obs, _ = demo_env.reset(seed=1234)
    path = [(demo_env._agent_x, demo_env._agent_y)]
    action_names = ["stay", "N", "S", "W", "E"]

    for step in range(30):
        action, _ = model.predict(obs, deterministic=True)
        action = int(action)
        obs, reward, terminated, truncated, _ = demo_env.step(action)
        path.append((demo_env._agent_x, demo_env._agent_y))
        marker = " ***" if terminated else ""
        print(
            f"  t={step + 1:3d}  action={action_names[action]:4s}"
            f"  pos=({demo_env._agent_x:2d},{demo_env._agent_y:2d})"
            f"  reward={reward:6.2f}{marker}"
        )
        if terminated or truncated:
            break

    demo_env.close()
    env.close()
    print()
    print("Done.")


if __name__ == "__main__":
    main()
