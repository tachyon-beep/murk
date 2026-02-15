"""Crystal Navigator: 3D FCC lattice navigation with dual diffusion fields.

An agent navigates an 8x8x8 FCC12 lattice with two competing diffusion
fields: a beacon scent (attractive) and a radiation hazard (repulsive).
The agent must follow the beacon gradient while avoiding radiation zones.

This demonstrates:
- 3D FCC12 topology (12-connected, isotropic)
- Graph Laplacian diffusion (topology-agnostic, sentinel trick)
- Dual competing reward signals
- 13-action discrete control (stay + 12 FCC offsets)

Usage:
    cd crates/murk-python && maturin develop --release
    pip install stable-baselines3 gymnasium numpy
    python examples/crystal_nav/crystal_nav.py
"""

from __future__ import annotations

import time
from typing import Any

import numpy as np
import torch
from stable_baselines3 import PPO
from stable_baselines3.common.vec_env import DummyVecEnv
from stable_baselines3.common.utils import set_random_seed

import murk
from murk import Command, Config, ObsEntry, FieldMutability, FieldType, EdgeBehavior, WriteMode

# ─── World parameters ───────────────────────────────────────────

GRID_W, GRID_H, GRID_D = 8, 8, 8

# All 12 FCC neighbour offsets: permutations of (+-1, +-1, 0).
# Matches crates/murk-space/src/fcc12.rs FCC_OFFSETS exactly.
FCC_OFFSETS = [
    (1, 1, 0),
    (-1, 1, 0),
    (1, -1, 0),
    (-1, -1, 0),
    (1, 0, 1),
    (-1, 0, 1),
    (1, 0, -1),
    (-1, 0, -1),
    (0, 1, 1),
    (0, -1, 1),
    (0, 1, -1),
    (0, -1, -1),
]

# Beacon: attractive target the agent seeks.
BEACON_X, BEACON_Y, BEACON_Z = 6, 6, 6

# Radiation: hazard source the agent should avoid.
RADIATION_X, RADIATION_Y, RADIATION_Z = 2, 2, 2

# Diffusion coefficients (with dt=1.0).
# Beacon spreads wider (longer-range signal), radiation is sharper (local hazard).
# Non-negative weights condition: 12*D*dt + decay < 1.
BEACON_D = 0.06    # 12*0.06 + 0.01 = 0.73
RADIATION_D = 0.04  # 12*0.04 + 0.03 = 0.51

# Decay rates: without decay, diffusion saturates to SOURCE_INTENSITY everywhere
# (no gradient). Decay creates an exponential gradient around each source:
#   steady-state ~ S * exp(-d * sqrt(lambda/D))
# where d = graph distance from source in hops.
#
# Beacon:   sqrt(0.01/0.06) ≈ 0.41 → half-value at ~1.7 hops, detectable at ~6 hops.
# Radiation: sqrt(0.03/0.04) ≈ 0.87 → half-value at ~0.8 hops, sharp local hazard.
BEACON_DECAY = 0.01
RADIATION_DECAY = 0.03

SOURCE_INTENSITY = 10.0

# Field IDs (assigned by add_field order).
BEACON_FIELD = 0
RADIATION_FIELD = 1
AGENT_FIELD = 2

MAX_STEPS = 300
WARMUP_TICKS = 80
TOTAL_TIMESTEPS = 1_000_000

# Reward shaping constants.
#
# Decision: gradient scale.
#   With decay, the beacon field ranges ~0.2 to 10.0, creating a spatial gradient.
#   The gradient term provides directional shaping; the terminal bonus and
#   step penalty drive goal-reaching:
#
#   Per-step at beacon:      0.1*9.9 - 0.5 = +0.49  (only positive position)
#   Per-step 1 hop away:     0.1*4.2 - 0.5 = -0.08  (slightly negative)
#   Per-step at radiation:   0.1*(-9.6) - 0.5 = -1.46  (strongly negative)
#   Episode reaching in 4:   100 + 4*0.49 ≈ 102
#   Episode camping 300:     300 * (-0.08) = -24
#
GRADIENT_SCALE = 0.1
TERMINAL_BONUS = 100.0
STEP_PENALTY = 0.5


# ─── FCC coordinate utilities ───────────────────────────────────
#
# Decision: precompute at module load vs compute per-step.
#   FCC adjacency is static for a given (w, h, d). Computing it once
#   at import time avoids per-step dictionary lookups and lets the
#   diffusion propagator use pure numpy fancy indexing.
#
# Decision: canonical ordering.
#   z-then-y-then-x, skipping invalid parity, matching
#   crates/murk-space/src/fcc12.rs canonical_ordering().
#   This ensures our Python rank indices match the Rust engine's
#   field buffer layout.
#

def _build_fcc_adjacency(w, h, d):
    """Build FCC adjacency structures for an w x h x d lattice (Absorb edges).

    Returns:
        cells:         list of (x, y, z) tuples in canonical order
        coord_to_rank: dict mapping (x, y, z) -> rank index
        nbr_idx:       int32 array (cell_count, 12) of neighbour ranks,
                       with sentinel = cell_count for missing neighbours
        degree:        int32 array (cell_count,) of actual neighbour counts
    """
    # Enumerate valid cells in canonical order (z-then-y-then-x, even parity).
    cells = []
    coord_to_rank = {}
    for z in range(d):
        for y in range(h):
            x_start = (y + z) % 2
            for x in range(x_start, w, 2):
                coord_to_rank[(x, y, z)] = len(cells)
                cells.append((x, y, z))

    cell_count = len(cells)

    # Build neighbour index array with sentinel for missing neighbours.
    sentinel = cell_count
    nbr_idx = np.full((cell_count, 12), sentinel, dtype=np.int32)
    degree = np.zeros(cell_count, dtype=np.int32)

    for rank, (x, y, z) in enumerate(cells):
        d_count = 0
        for oi, (dx, dy, dz) in enumerate(FCC_OFFSETS):
            nx, ny, nz = x + dx, y + dy, z + dz
            nb_rank = coord_to_rank.get((nx, ny, nz))
            if nb_rank is not None:
                nbr_idx[rank, oi] = nb_rank
                d_count += 1
        degree[rank] = d_count

    return cells, coord_to_rank, nbr_idx, degree


# Precompute at module load.
CELLS, COORD_TO_RANK, NBR_IDX, DEGREE = _build_fcc_adjacency(GRID_W, GRID_H, GRID_D)
CELL_COUNT = len(CELLS)


def _build_transition_table(cells, coord_to_rank, cell_count):
    """Precompute NEXT_RANK[rank, action] → rank (invalid moves map to self).

    This turns movement into a single array lookup — no per-step dict
    membership tests, no risk of rank mismatch bugs. The agent operates
    on a graph, and this table IS the graph from the control perspective.

    Also builds VALID_MASK[rank, action] for optional action-masking.
    """
    # 13 actions: 0=stay, 1-12=FCC offsets.
    next_rank = np.zeros((cell_count, 13), dtype=np.int32)
    valid_mask = np.zeros((cell_count, 13), dtype=np.bool_)

    for rank, (x, y, z) in enumerate(cells):
        # Action 0: stay (always valid).
        next_rank[rank, 0] = rank
        valid_mask[rank, 0] = True
        # Actions 1-12: FCC offsets.
        for ai, (dx, dy, dz) in enumerate(FCC_OFFSETS):
            nb = coord_to_rank.get((x + dx, y + dy, z + dz))
            if nb is not None:
                next_rank[rank, ai + 1] = nb
                valid_mask[rank, ai + 1] = True
            else:
                next_rank[rank, ai + 1] = rank  # absorb → stay
                valid_mask[rank, ai + 1] = False

    return next_rank, valid_mask


NEXT_RANK, VALID_MASK = _build_transition_table(CELLS, COORD_TO_RANK, CELL_COUNT)

# ─── Debug assertions ───────────────────────────────────────────

if __debug__:
    # All cells have even parity.
    for x, y, z in CELLS:
        assert (x + y + z) % 2 == 0, f"bad parity: ({x},{y},{z})"

    # Cell count matches expectation for 8x8x8 FCC.
    assert CELL_COUNT == 256, f"expected 256 cells, got {CELL_COUNT}"

    # Interior cells (not touching any face) should have degree 12.
    for rank, (x, y, z) in enumerate(CELLS):
        if 1 <= x < GRID_W - 1 and 1 <= y < GRID_H - 1 and 1 <= z < GRID_D - 1:
            assert DEGREE[rank] == 12, (
                f"interior cell ({x},{y},{z}) has degree {DEGREE[rank]}, expected 12"
            )

    # Non-negative weights check: 12*D + decay < 1 ensures the explicit
    # update weights stay non-negative (monotonicity / no overshoot).
    assert 12 * BEACON_D * 1.0 + BEACON_DECAY < 1.0, "beacon weights go negative"
    assert 12 * RADIATION_D * 1.0 + RADIATION_DECAY < 1.0, "radiation weights go negative"

    # Source cells exist in the lattice.
    assert (BEACON_X, BEACON_Y, BEACON_Z) in COORD_TO_RANK, "beacon not a valid FCC cell"
    assert (RADIATION_X, RADIATION_Y, RADIATION_Z) in COORD_TO_RANK, "radiation not a valid FCC cell"


# ─── Propagator: graph Laplacian diffusion ──────────────────────
#
# Decision: np.pad trick vs graph Laplacian.
#   np.pad only works for rectangular grids (Square4, Square8). FCC's
#   irregular connectivity requires a topology-agnostic approach.
#
# Decision: sentinel trick for vectorized numpy.
#   Pad the field array with one extra element (sentinel = 0.0) at
#   index CELL_COUNT. Missing neighbours in NBR_IDX point to this
#   sentinel, so fancy indexing gathers 0.0 for them.
#
#   This implements the combinatorial graph Laplacian on the induced
#   subgraph: L*u = sum_nbr(u_nbr) - deg(v)*u_v.  The sentinel is
#   a vectorisation convenience — missing neighbours are absent edges,
#   not "outside world is zero".  DEGREE corrects the self-term
#   (-deg * u), not a denominator.
#
#   This avoids Python loops over cells and runs at numpy speed.
#

def dual_diffusion_step(reads, reads_prev, writes, tick_id, dt, cell_count):
    """Graph Laplacian diffusion for beacon and radiation fields.

    Both fields use Jacobi-style reads (reads_previous) and full writes.
    Different diffusion coefficients give different spread characteristics.
    """
    prev_beacon = reads_prev[0]
    prev_radiation = reads_prev[1]
    out_beacon = writes[0]
    out_radiation = writes[1]

    # --- Beacon scent diffusion (D=0.06, decay=0.01) ---
    padded = np.zeros(cell_count + 1, dtype=np.float32)
    padded[:cell_count] = prev_beacon
    nbr_sum = padded[NBR_IDX].sum(axis=1)
    laplacian = nbr_sum - DEGREE * prev_beacon
    new_beacon = prev_beacon + BEACON_D * dt * laplacian - BEACON_DECAY * dt * prev_beacon
    beacon_rank = COORD_TO_RANK[(BEACON_X, BEACON_Y, BEACON_Z)]
    new_beacon[beacon_rank] = SOURCE_INTENSITY
    np.maximum(new_beacon, 0.0, out=new_beacon)
    out_beacon[:] = new_beacon

    # --- Radiation hazard diffusion (D=0.04, decay=0.03) ---
    padded[:cell_count] = prev_radiation
    padded[cell_count] = 0.0  # re-zero the sentinel
    nbr_sum = padded[NBR_IDX].sum(axis=1)
    laplacian = nbr_sum - DEGREE * prev_radiation
    new_radiation = prev_radiation + RADIATION_D * dt * laplacian - RADIATION_DECAY * dt * prev_radiation
    radiation_rank = COORD_TO_RANK[(RADIATION_X, RADIATION_Y, RADIATION_Z)]
    new_radiation[radiation_rank] = SOURCE_INTENSITY
    np.maximum(new_radiation, 0.0, out=new_radiation)
    out_radiation[:] = new_radiation


# ─── Environment ─────────────────────────────────────────────────
#
# Decision: observation space.
#   3 fields x 256 cells = 768 floats. Still trivial for PPO's MLP.
#   The agent sees the full beacon gradient, full radiation map, and
#   its own position — enough information to plan a safe path.
#
# Decision: reward shaping.
#   reward = beacon_scent[agent] - radiation[agent] - STEP_PENALTY
#   Plus a TERMINAL_BONUS when the agent reaches the beacon.
#
#   The gradient term provides directional signal (go toward beacon,
#   away from radiation). The step penalty discourages camping at a
#   "good enough" position. The terminal bonus makes reaching the
#   actual beacon cell worth far more than just hovering nearby.
#
# Decision: action space.
#   13 discrete actions: 0 = stay, 1-12 = FCC offsets.
#   Invalid moves (out of bounds) result in staying put (absorb).
#

class CrystalNavEnv(murk.MurkEnv):
    """8x8x8 FCC lattice: navigate toward beacon, avoid radiation."""

    def __init__(self, seed: int = 0):
        config = Config()

        # Space: 8x8x8 FCC12 lattice with absorb boundaries.
        config.set_space_fcc12(GRID_W, GRID_H, GRID_D, EdgeBehavior.Absorb)

        # Fields: beacon scent, radiation hazard, agent position.
        config.add_field("beacon_scent", FieldType.Scalar, FieldMutability.PerTick)
        config.add_field("radiation", FieldType.Scalar, FieldMutability.PerTick)
        config.add_field("agent_pos", FieldType.Scalar, FieldMutability.PerTick)

        config.set_dt(1.0)
        config.set_seed(seed)

        # Register dual diffusion propagator.
        # Reads previous tick of both beacon and radiation (Jacobi style).
        # Writes full to both fields.
        murk.add_propagator(
            config,
            name="dual_diffusion",
            step_fn=dual_diffusion_step,
            reads_previous=[BEACON_FIELD, RADIATION_FIELD],
            writes=[(BEACON_FIELD, WriteMode.Full), (RADIATION_FIELD, WriteMode.Full)],
        )

        # Observation plan: all three fields in full.
        obs_entries = [
            ObsEntry(BEACON_FIELD),
            ObsEntry(RADIATION_FIELD),
            ObsEntry(AGENT_FIELD),
        ]

        super().__init__(
            config=config,
            obs_entries=obs_entries,
            n_actions=13,
            seed=seed,
        )

        self._tick_limit = MAX_STEPS
        self._agent_x = 0
        self._agent_y = 0
        self._agent_z = 0
        self._episode_count = 0

    def reset(
        self, *, seed: int | None = None, options: dict | None = None
    ) -> tuple[np.ndarray, dict]:
        if seed is not None:
            self._seed = seed

        self._world.reset(self._seed)

        # Warmup: run empty ticks so diffusion fields reach near-steady-state.
        # 3D needs more warmup than 2D — gradient must propagate through 12-connected mesh.
        for _ in range(WARMUP_TICKS):
            self._world.step(None)

        # Place agent at a random valid FCC cell (excluding the beacon cell).
        rng = np.random.default_rng(self._seed + self._episode_count)
        self._episode_count += 1
        beacon_rank = COORD_TO_RANK[(BEACON_X, BEACON_Y, BEACON_Z)]
        while True:
            rank = int(rng.integers(0, CELL_COUNT))
            if rank != beacon_rank:
                break
        self._agent_x, self._agent_y, self._agent_z = CELLS[rank]

        # Stamp agent position and tick once more.
        cmd = Command.set_field(
            AGENT_FIELD, [self._agent_x, self._agent_y, self._agent_z], 1.0
        )
        self._world.step([cmd])

        tick_id, age_ticks = self._obs_plan.execute(
            self._world, self._obs_buf, self._mask_buf
        )
        obs = self._obs_buf.copy()
        return obs, {"tick_id": tick_id, "age_ticks": age_ticks}

    def _action_to_commands(self, action: Any) -> list[Command]:
        # Single array lookup — NEXT_RANK encodes the full graph topology.
        # Invalid moves (boundary) map to self (absorb).
        cur = COORD_TO_RANK[(self._agent_x, self._agent_y, self._agent_z)]
        nxt = int(NEXT_RANK[cur, action])
        self._agent_x, self._agent_y, self._agent_z = CELLS[nxt]

        return [Command.set_field(
            AGENT_FIELD, [self._agent_x, self._agent_y, self._agent_z], 1.0
        )]

    def _compute_reward(self, obs: np.ndarray, info: dict) -> float:
        # obs layout: [beacon(256), radiation(256), agent_pos(256)]
        beacon = obs[:CELL_COUNT]
        radiation = obs[CELL_COUNT : 2 * CELL_COUNT]
        agent_rank = COORD_TO_RANK[(self._agent_x, self._agent_y, self._agent_z)]
        gradient = float(beacon[agent_rank] - radiation[agent_rank])
        reward = GRADIENT_SCALE * gradient - STEP_PENALTY
        if self._check_terminated(obs, info):
            reward += TERMINAL_BONUS
        return reward

    def _check_terminated(self, obs: np.ndarray, info: dict) -> bool:
        return (
            self._agent_x == BEACON_X
            and self._agent_y == BEACON_Y
            and self._agent_z == BEACON_Z
        )


# ─── Evaluation ──────────────────────────────────────────────────

def evaluate(model, n_episodes: int = 10) -> tuple[float, float, float]:
    """Run n episodes and return (mean_reward, mean_length, reach_rate)."""
    env = CrystalNavEnv(seed=9999)
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
    print("  Crystal Navigator: Murk + PPO on 3D FCC12 Lattice")
    print("=" * 60)
    print()
    print(f"  Lattice:     {GRID_W}x{GRID_H}x{GRID_D} FCC12 ({CELL_COUNT} cells)")
    print(f"  Beacon:      ({BEACON_X},{BEACON_Y},{BEACON_Z}), D={BEACON_D}")
    print(f"  Radiation:   ({RADIATION_X},{RADIATION_Y},{RADIATION_Z}), D={RADIATION_D}")
    print(f"  Actions:     13 (stay + 12 FCC offsets)")
    print(f"  Obs size:    {CELL_COUNT * 3} (beacon + radiation + agent_pos)")
    print(f"  Warmup:      {WARMUP_TICKS} ticks")
    print(f"  Training:    {TOTAL_TIMESTEPS:,} timesteps")
    print()

    # ── Pin all RNG seeds for reproducibility ──────────────
    set_random_seed(42)
    torch.manual_seed(42)

    # ── Create vectorized environment for PPO ────────────────
    env = DummyVecEnv([lambda: CrystalNavEnv(seed=42)])

    # ── Create PPO model ─────────────────────────────────────
    #
    # Decision: network architecture.
    #   768-dim input + 13 actions warrants a slightly larger network
    #   than heat_seeker's default 64x64. Two 128-unit hidden layers
    #   give enough capacity for dual-gradient navigation.
    #
    #
    # Decision: entropy coefficient.
    #   FCC12 has 13 actions where each offset changes exactly 2 of 3 axes.
    #   The agent must alternate actions to navigate in all 3 dimensions.
    #   Without sufficient entropy, PPO collapses to a single action
    #   (e.g. always (+1,+1,0)) and never explores z-axis movement.
    #   ent_coef=0.15 prevents premature policy collapse in 3D.
    #
    model = PPO(
        "MlpPolicy",
        env,
        n_steps=2048,
        ent_coef=0.15,
        verbose=0,
        policy_kwargs=dict(net_arch=[128, 128]),
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
    demo_env = CrystalNavEnv(seed=1234)
    obs, _ = demo_env.reset(seed=1234)
    action_names = ["stay"] + [
        f"({dx:+d},{dy:+d},{dz:+d})" for dx, dy, dz in FCC_OFFSETS
    ]

    for step in range(50):
        action, _ = model.predict(obs, deterministic=True)
        action = int(action)
        obs, reward, terminated, truncated, _ = demo_env.step(action)
        marker = " ***" if terminated else ""
        print(
            f"  t={step + 1:3d}  action={action_names[action]:>12s}"
            f"  pos=({demo_env._agent_x},{demo_env._agent_y},{demo_env._agent_z})"
            f"  reward={reward:7.3f}{marker}"
        )
        if terminated or truncated:
            break

    demo_env.close()
    env.close()
    print()
    print("Done.")


if __name__ == "__main__":
    main()
