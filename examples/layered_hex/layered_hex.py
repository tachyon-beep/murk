"""Layered Hex World: ProductSpace composition with Hex2D x Line1D.

An agent navigates a 3-floor hexagonal building. Each floor is a 6x6
Hex2D grid (36 cells), and the floors are connected vertically by a
Line1D with 3 cells (Absorb edges). The agent starts on floor 0 and
must reach a goal on floor 2 by combining hex movement with floor
transitions.

This demonstrates:
- ProductSpace composition (Hex2D x Line1D = 108 cells)
- Cross-component navigation (hex moves + floor changes)
- Beacon diffusion across a composed space
- Observation from ProductSpace (graph Laplacian with product neighbours)
- 9-action discrete control (stay + 6 hex + up + down)

Usage:
    cd crates/murk-python && maturin develop --release
    pip install stable-baselines3 gymnasium numpy torch
    python examples/layered_hex/layered_hex.py
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
from murk import (
    Command, Config, ObsEntry, FieldMutability, FieldType,
    EdgeBehavior, SpaceType, WriteMode,
)

# --- World parameters --------------------------------------------------------
#
# Decision: grid dimensions.
#   6x6 hex per floor = 36 cells. Three floors = 108 total cells.
#   Small enough for fast training, large enough that random walks
#   rarely reach the goal (floor 0 -> floor 2 + hex navigation).
#
HEX_COLS, HEX_ROWS = 6, 6
HEX_CELLS = HEX_COLS * HEX_ROWS  # 36 cells per floor
N_FLOORS = 3
CELL_COUNT = HEX_CELLS * N_FLOORS  # 108 total cells

# Hex neighbour offsets in axial (dq, dr): E, NE, NW, W, SW, SE.
# Matches crates/murk-space/src/hex2d.rs HEX_OFFSETS exactly.
HEX_OFFSETS = [
    (1, 0),   # E
    (1, -1),  # NE
    (0, -1),  # NW
    (-1, 0),  # W
    (-1, 1),  # SW
    (0, 1),   # SE
]

# Goal position: corner of the top floor.
GOAL_Q, GOAL_R, GOAL_Z = 5, 5, 2

# Diffusion parameters.
#
# Decision: diffusion coefficient and decay.
#   The ProductSpace graph has up to 8 neighbours per cell (6 hex + 2 line).
#   Non-negative weights condition: max_degree * D * dt + decay < 1.
#   8 * 0.06 + 0.01 = 0.49 (safe).
#
#   Beacon decay controls gradient characteristic length. With D=0.06,
#   decay=0.01 on a graph: half-value ~1.7 hops, detectable ~6 hops.
#   This gives a usable gradient across the 3-floor building.
#
BEACON_D = 0.06
BEACON_DECAY = 0.01
SOURCE_INTENSITY = 10.0

# Field IDs (assigned by add_field order).
BEACON_FIELD = 0
AGENT_FIELD = 1

MAX_STEPS = 200
WARMUP_TICKS = 60

# Reward shaping.
#
# Decision: reward structure.
#   At goal (beacon~10):   0.1*10 - 0.5 + 100 = +99.5  (terminal)
#   1 hop from goal (~5):  0.1*5  - 0.5       = +0.0    (neutral)
#   Far away (~0.5):       0.1*0.5 - 0.5      = -0.45   (negative)
#   Camping 200 steps:     200 * -0.45         = -90     (worse than reaching)
#
GRADIENT_SCALE = 0.1
TERMINAL_BONUS = 100.0
STEP_PENALTY = 0.5

# Training budget.
TOTAL_TIMESTEPS = 200_000


# --- ProductSpace coordinate utilities ----------------------------------------
#
# ProductSpace concatenates per-component coordinates. For Hex2D(q,r) x
# Line1D(z), each cell is addressed as [q, r, z].
#
# Canonical ordering: leftmost component (hex) slowest, rightmost (line)
# fastest. So the rank of [q, r, z] is:
#     rank = hex_rank(q, r) * N_FLOORS + z
# where hex_rank(q, r) = r * HEX_COLS + q  (hex uses r-then-q ordering).
#
# Neighbours vary one component at a time:
#   - 6 hex neighbours: change (q, r), hold z
#   - up to 2 line neighbours: change z, hold (q, r)
#   Total: up to 8 for interior cells.
#

def _build_product_structures():
    """Build cell list, lookup tables, and adjacency for the product space.

    Returns:
        cells:           list of (q, r, z) in canonical order
        coord_to_rank:   dict (q, r, z) -> rank
        nbr_idx:         int32 array (CELL_COUNT, 8), sentinel = CELL_COUNT
        degree:          int32 array (CELL_COUNT,)
        next_rank:       int32 array (CELL_COUNT, 9), action transition table
    """
    # Enumerate cells in canonical order: hex(r-then-q) slowest, line fastest.
    cells = []
    coord_to_rank = {}
    for r in range(HEX_ROWS):
        for q in range(HEX_COLS):
            for z in range(N_FLOORS):
                coord_to_rank[(q, r, z)] = len(cells)
                cells.append((q, r, z))

    cell_count = len(cells)
    assert cell_count == CELL_COUNT

    # Build neighbour index with sentinel for missing neighbours.
    # Max 8 neighbours: 6 hex + 2 line.
    sentinel = cell_count
    nbr_idx = np.full((cell_count, 8), sentinel, dtype=np.int32)
    degree = np.zeros(cell_count, dtype=np.int32)

    for rank, (q, r, z) in enumerate(cells):
        d = 0
        # Hex neighbours (vary q,r; hold z).
        for dq, dr in HEX_OFFSETS:
            nq, nr = q + dq, r + dr
            if 0 <= nq < HEX_COLS and 0 <= nr < HEX_ROWS:
                nbr_idx[rank, d] = coord_to_rank[(nq, nr, z)]
                d += 1
        # Line neighbours (vary z; hold q,r). Absorb edges.
        if z > 0:
            nbr_idx[rank, d] = coord_to_rank[(q, r, z - 1)]
            d += 1
        if z < N_FLOORS - 1:
            nbr_idx[rank, d] = coord_to_rank[(q, r, z + 1)]
            d += 1
        degree[rank] = d

    # Build action transition table.
    # 9 actions: 0=stay, 1-6=hex offsets (E,NE,NW,W,SW,SE), 7=down, 8=up.
    next_rank = np.zeros((cell_count, 9), dtype=np.int32)

    for rank, (q, r, z) in enumerate(cells):
        # Action 0: stay.
        next_rank[rank, 0] = rank
        # Actions 1-6: hex offsets.
        for ai, (dq, dr) in enumerate(HEX_OFFSETS):
            nq, nr = q + dq, r + dr
            if 0 <= nq < HEX_COLS and 0 <= nr < HEX_ROWS:
                next_rank[rank, ai + 1] = coord_to_rank[(nq, nr, z)]
            else:
                next_rank[rank, ai + 1] = rank  # absorb -> stay
        # Action 7: down (z - 1).
        if z > 0:
            next_rank[rank, 7] = coord_to_rank[(q, r, z - 1)]
        else:
            next_rank[rank, 7] = rank  # absorb -> stay
        # Action 8: up (z + 1).
        if z < N_FLOORS - 1:
            next_rank[rank, 8] = coord_to_rank[(q, r, z + 1)]
        else:
            next_rank[rank, 8] = rank  # absorb -> stay

    return cells, coord_to_rank, nbr_idx, degree, next_rank


# Precompute at module load.
CELLS, COORD_TO_RANK, NBR_IDX, DEGREE, NEXT_RANK = _build_product_structures()


# --- Debug assertions ---------------------------------------------------------

if __debug__:
    assert CELL_COUNT == 108, f"expected 108 cells, got {CELL_COUNT}"

    # All cells have valid coordinates.
    for q, r, z in CELLS:
        assert 0 <= q < HEX_COLS
        assert 0 <= r < HEX_ROWS
        assert 0 <= z < N_FLOORS

    # Interior cells (not on any hex edge, not on floor boundary) should
    # have degree 8 (6 hex + 2 line).
    for rank, (q, r, z) in enumerate(CELLS):
        if 1 <= q < HEX_COLS - 1 and 1 <= r < HEX_ROWS - 1 and 0 < z < N_FLOORS - 1:
            # Interior hex cells always have 6 hex neighbours in a rectangular grid.
            assert DEGREE[rank] == 8, (
                f"interior cell ({q},{r},{z}) has degree {DEGREE[rank]}, expected 8"
            )

    # Non-negative weights: 8*D*dt + decay < 1.
    assert 8 * BEACON_D * 1.0 + BEACON_DECAY < 1.0, "beacon weights go negative"

    # Goal cell exists.
    assert (GOAL_Q, GOAL_R, GOAL_Z) in COORD_TO_RANK, "goal not a valid cell"


# --- Propagator: graph Laplacian diffusion ------------------------------------
#
# Decision: graph Laplacian vs per-component diffusion.
#   ProductSpace neighbours mix hex and line components. The graph
#   Laplacian on the full product graph handles this naturally: every
#   edge (hex or line) contributes equally to the sum. No need to run
#   separate diffusion passes per component.
#
# Decision: sentinel trick (same as crystal_nav).
#   Pad the field array with one extra zero at CELL_COUNT. Missing
#   neighbours in NBR_IDX point there, so fancy indexing gathers 0.0.
#   DEGREE corrects the self-term in the Laplacian.
#

def beacon_diffusion_step(reads, reads_prev, writes, tick_id, dt, cell_count):
    """Graph Laplacian diffusion for the beacon scent field.

    reads_prev[0]: previous tick's beacon field
    writes[0]:     output beacon field
    """
    prev = reads_prev[0]
    out = writes[0]

    # Sentinel trick: append a zero for out-of-bounds indexing.
    padded = np.zeros(cell_count + 1, dtype=np.float32)
    padded[:cell_count] = prev

    # Graph Laplacian: L*u = sum_nbr(u_nbr) - deg(v)*u_v
    nbr_sum = padded[NBR_IDX].sum(axis=1)
    laplacian = nbr_sum - DEGREE * prev

    new_beacon = prev + BEACON_D * dt * laplacian - BEACON_DECAY * dt * prev

    # Inject source at goal cell.
    goal_rank = COORD_TO_RANK[(GOAL_Q, GOAL_R, GOAL_Z)]
    new_beacon[goal_rank] = SOURCE_INTENSITY

    np.maximum(new_beacon, 0.0, out=new_beacon)
    out[:] = new_beacon


# --- Environment --------------------------------------------------------------
#
# Decision: observation space.
#   2 fields x 108 cells = 216 floats. Trivial for PPO's MLP.
#   The agent sees the full beacon gradient (for path planning across
#   floors) and its own position (one-hot-style).
#
# Decision: action space.
#   9 discrete actions: stay + 6 hex moves + down + up.
#   Cross-component actions (floor transitions) let the agent navigate
#   the vertical axis of the product space.
#

class LayeredHexEnv(murk.MurkEnv):
    """3-floor hex building: navigate from floor 0 to goal on floor 2."""

    def __init__(self, seed: int = 0):
        config = Config()

        # Space: Hex2D(6,6) x Line1D(3, Absorb) via the low-level set_space().
        #
        # ProductSpace uses the generic set_space() method with a flat
        # parameter array encoding:
        #   [n_components, type_0, n_params_0, p0_0, ..., type_1, ...]
        #
        # SpaceType enum values: Hex2D=4, Line1D=0.
        # EdgeBehavior enum values: Absorb=0.
        # Hex2D params: [cols, rows].
        # Line1D params: [length, edge_behavior].
        config.set_space(
            SpaceType.ProductSpace,
            [
                2.0,             # n_components
                4.0,             # type_0 = Hex2D
                2.0,             # n_params_0
                float(HEX_COLS), # cols
                float(HEX_ROWS), # rows
                0.0,             # type_1 = Line1D
                2.0,             # n_params_1
                float(N_FLOORS), # length
                0.0,             # edge_behavior = Absorb
            ],
        )

        # Fields: beacon scent and agent position.
        config.add_field("beacon_scent", FieldType.Scalar, FieldMutability.PerTick)
        config.add_field("agent_pos", FieldType.Scalar, FieldMutability.PerTick)

        config.set_dt(1.0)
        config.set_seed(seed)

        # Register the beacon diffusion propagator (Jacobi-style read).
        murk.add_propagator(
            config,
            name="beacon_diffusion",
            step_fn=beacon_diffusion_step,
            reads_previous=[BEACON_FIELD],
            writes=[(BEACON_FIELD, WriteMode.Full)],
        )

        # Observation plan: both fields in full.
        obs_entries = [
            ObsEntry(BEACON_FIELD),
            ObsEntry(AGENT_FIELD),
        ]

        super().__init__(
            config=config,
            obs_entries=obs_entries,
            n_actions=9,
            seed=seed,
        )

        self._tick_limit = MAX_STEPS
        self._agent_q = 0
        self._agent_r = 0
        self._agent_z = 0
        self._episode_count = 0

    def reset(
        self, *, seed: int | None = None, options: dict | None = None
    ) -> tuple[np.ndarray, dict]:
        if seed is not None:
            self._seed = seed

        self._world.reset(self._seed)

        # Warmup: let beacon diffuse to near-steady-state.
        for _ in range(WARMUP_TICKS):
            self._world.step(None)

        # Place agent at a random cell on floor 0 (excluding the goal).
        rng = np.random.default_rng(self._seed + self._episode_count)
        self._episode_count += 1
        goal_rank = COORD_TO_RANK[(GOAL_Q, GOAL_R, GOAL_Z)]
        while True:
            # Random hex cell on floor 0.
            self._agent_q = int(rng.integers(0, HEX_COLS))
            self._agent_r = int(rng.integers(0, HEX_ROWS))
            self._agent_z = 0
            rank = COORD_TO_RANK[(self._agent_q, self._agent_r, self._agent_z)]
            if rank != goal_rank:
                break

        # Stamp agent position and tick once more.
        cmd = Command.set_field(
            AGENT_FIELD,
            [self._agent_q, self._agent_r, self._agent_z],
            1.0,
        )
        self._world.step([cmd])

        tick_id, age_ticks = self._obs_plan.execute(
            self._world, self._obs_buf, self._mask_buf
        )
        self._episode_start_tick = tick_id
        obs = self._obs_buf.copy()
        return obs, {"tick_id": tick_id, "age_ticks": age_ticks}

    def _action_to_commands(self, action: Any) -> list[Command]:
        # Single array lookup for the full product graph.
        cur = COORD_TO_RANK[(self._agent_q, self._agent_r, self._agent_z)]
        nxt = int(NEXT_RANK[cur, action])
        self._agent_q, self._agent_r, self._agent_z = CELLS[nxt]

        return [Command.set_field(
            AGENT_FIELD,
            [self._agent_q, self._agent_r, self._agent_z],
            1.0,
        )]

    def _compute_reward(self, obs: np.ndarray, info: dict) -> float:
        # obs layout: [beacon(108 floats), agent_pos(108 floats)]
        beacon = obs[:CELL_COUNT]
        agent_rank = COORD_TO_RANK[(self._agent_q, self._agent_r, self._agent_z)]
        reward = GRADIENT_SCALE * float(beacon[agent_rank]) - STEP_PENALTY
        if self._check_terminated(obs, info):
            reward += TERMINAL_BONUS
        return reward

    def _check_terminated(self, obs: np.ndarray, info: dict) -> bool:
        return (
            self._agent_q == GOAL_Q
            and self._agent_r == GOAL_R
            and self._agent_z == GOAL_Z
        )


# --- Evaluation ---------------------------------------------------------------

def evaluate(model, n_episodes: int = 10) -> tuple[float, float, float]:
    """Run n episodes and return (mean_reward, mean_length, reach_rate)."""
    env = LayeredHexEnv(seed=9999)
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


# --- Main ---------------------------------------------------------------------

def main():
    print("=" * 60)
    print("  Layered Hex World: ProductSpace (Hex2D x Line1D)")
    print("=" * 60)
    print()
    print(f"  Hex grid:    {HEX_COLS}x{HEX_ROWS} ({HEX_CELLS} cells/floor)")
    print(f"  Floors:      {N_FLOORS} (Line1D, Absorb edges)")
    print(f"  Total cells: {CELL_COUNT} (ProductSpace)")
    print(f"  Goal:        ({GOAL_Q},{GOAL_R},{GOAL_Z}) — floor 2, corner")
    print(f"  Actions:     9 (stay + 6 hex + down + up)")
    print(f"  Obs size:    {CELL_COUNT * 2} (beacon + agent_pos)")
    print(f"  Warmup:      {WARMUP_TICKS} ticks")
    print(f"  Training:    {TOTAL_TIMESTEPS:,} timesteps")
    print()

    # Pin all RNG seeds for reproducibility.
    set_random_seed(42)
    torch.manual_seed(42)

    # Create vectorized environment for PPO.
    env = DummyVecEnv([lambda: LayeredHexEnv(seed=42)])

    # Create PPO model.
    #
    # Decision: hyperparameters.
    #   216-dim input + 9 actions — small enough for a compact MLP.
    #   n_steps=2048: long rollouts for good value estimates.
    #   ent_coef=0.15: prevents premature collapse on the 9-action space;
    #   the agent must discover floor transitions AND hex navigation.
    #   net_arch=[128, 128]: sufficient for the 216-dim observation.
    #
    model = PPO(
        "MlpPolicy",
        env,
        n_steps=2048,
        ent_coef=0.15,
        verbose=0,
        policy_kwargs=dict(net_arch=[128, 128]),
    )

    # Evaluate before training (random policy baseline).
    print("Evaluating random policy (before training)...")
    mean_r, mean_l, reach = evaluate(model, n_episodes=10)
    print(f"  Mean reward:  {mean_r:8.1f}")
    print(f"  Mean length:  {mean_l:8.1f} steps")
    print(f"  Reach rate:   {reach:8.0%}")
    print()

    # Train.
    print(f"Training PPO for {TOTAL_TIMESTEPS:,} timesteps...")
    t0 = time.time()
    model.learn(total_timesteps=TOTAL_TIMESTEPS, progress_bar=True)
    elapsed = time.time() - t0
    print(f"  Done in {elapsed:.1f}s ({TOTAL_TIMESTEPS / elapsed:.0f} steps/sec)")
    print()

    # Evaluate after training.
    print("Evaluating trained policy (after training)...")
    mean_r, mean_l, reach = evaluate(model, n_episodes=10)
    print(f"  Mean reward:  {mean_r:8.1f}")
    print(f"  Mean length:  {mean_l:8.1f} steps")
    print(f"  Reach rate:   {reach:8.0%}")
    print()

    # Show a sample trajectory.
    print("Sample trajectory (trained agent):")
    demo_env = LayeredHexEnv(seed=1234)
    obs, _ = demo_env.reset(seed=1234)
    action_names = ["stay", "E", "NE", "NW", "W", "SW", "SE", "down", "up"]

    for step in range(40):
        action, _ = model.predict(obs, deterministic=True)
        action = int(action)
        obs, reward, terminated, truncated, _ = demo_env.step(action)
        marker = " ***" if terminated else ""
        print(
            f"  t={step + 1:3d}  action={action_names[action]:4s}"
            f"  pos=({demo_env._agent_q},{demo_env._agent_r},f{demo_env._agent_z})"
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
