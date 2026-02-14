"""Hex Pursuit: multi-agent predator-prey on a hexagonal grid.

Demonstrates:
  - Hex2D space backend (6-connected, pointy-top axial coordinates)
  - Multi-agent observation via ObsPlan.execute_agents()
  - AgentDisk regions for local perception (foveation)
  - Per-agent commands via SetField
  - Competitive reward design

A predator chases a prey on a hex grid. Both agents see only a local
disk around their position (AgentDisk). The predator is rewarded for
closing distance; the prey is rewarded for increasing it. The episode
ends when the predator catches the prey or a tick limit is reached.

Usage:
    cd crates/murk-python && maturin develop --release
    python examples/hex_pursuit/hex_pursuit.py
"""

from __future__ import annotations

import numpy as np

from murk._murk import (
    Command,
    Config,
    FieldMutability,
    ObsEntry,
    ObsPlan,
    PropagatorDef,
    RegionType,
    World,
    WriteMode,
)

# ─── World parameters ───────────────────────────────────────────
#
# Decision: grid size.
#   12x12 hex = 144 cells. Large enough that agents can't see
#   each other from across the grid with radius=3 perception,
#   small enough for fast iteration.
#
COLS, ROWS = 12, 12
CELL_COUNT = COLS * ROWS

# Decision: perception radius.
#   Radius 3 on a hex grid gives a disk of ~37 cells (vs 144 total).
#   This means each agent sees ~25% of the grid — enough to navigate
#   locally but not enough to always see the opponent.
#
PERCEPTION_RADIUS = 3

# Hex2D neighbor offsets (pointy-top, axial coordinates).
# The 6 directions in (dq, dr) form.
HEX_DIRS = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, -1), (-1, 1)]

# Field IDs (assigned by add_field order).
PREDATOR_FIELD = 0  # Binary: 1.0 at predator position, 0.0 elsewhere
PREY_FIELD = 1      # Binary: 1.0 at prey position, 0.0 elsewhere

MAX_TICKS = 100
N_EPISODES = 5

# ─── Propagator ─────────────────────────────────────────────────
#
# The propagator is a no-op identity: agent positions are set entirely
# by SetField commands. This is the simplest pattern for agent-based
# sims where positions change via external actions, not internal physics.
#
# We still need a propagator because Murk requires at least one
# propagator to write each PerTick field (otherwise the field would
# be zero every tick). We use WriteMode.Incremental so the previous
# tick's values carry forward, then overwrite via commands.
#

def identity_step(reads, reads_prev, writes, tick_id, dt, cell_count):
    """Copy previous values forward. Commands overwrite afterwards."""
    writes[0][:] = reads_prev[0]
    writes[1][:] = reads_prev[1]


# ─── Hex coordinate helpers ─────────────────────────────────────

def hex_distance(q1: int, r1: int, q2: int, r2: int) -> int:
    """Cube distance between two axial hex coordinates."""
    dq = q2 - q1
    dr = r2 - r1
    return max(abs(dq), abs(dr), abs(dq + dr))


def hex_move(q: int, r: int, direction: int, cols: int, rows: int) -> tuple[int, int]:
    """Move one step in a hex direction, clamping to grid bounds (absorb)."""
    dq, dr = HEX_DIRS[direction]
    nq = q + dq
    nr = r + dr
    # Absorb boundary: clamp to valid range.
    nq = max(0, min(cols - 1, nq))
    nr = max(0, min(rows - 1, nr))
    return nq, nr


# ─── Main ───────────────────────────────────────────────────────

def main():
    print("=" * 60)
    print("  Hex Pursuit: multi-agent predator-prey on Hex2D")
    print("=" * 60)
    print()
    print(f"  Grid:        {COLS}x{ROWS} Hex2D ({CELL_COUNT} cells)")
    print(f"  Agents:      predator + prey")
    print(f"  Perception:  AgentDisk radius={PERCEPTION_RADIUS}")
    print(f"  Actions:     6 (hex directions) + 1 (stay) = 7")
    print()

    # ── Build world ───────────────────────────────────────────
    config = Config()
    config.set_space_hex2d(COLS, ROWS)

    config.add_field("predator", mutability=FieldMutability.PerTick)
    config.add_field("prey", mutability=FieldMutability.PerTick)

    config.set_dt(1.0)
    config.set_seed(42)

    # Register a propagator that copies previous values forward.
    # WriteMode.Full because we write every cell (identity copy).
    prop = PropagatorDef(
        "identity",
        identity_step,
        reads_previous=[PREDATOR_FIELD, PREY_FIELD],
        writes=[
            (PREDATOR_FIELD, WriteMode.Full),
            (PREY_FIELD, WriteMode.Full),
        ],
    )
    prop.register(config)

    world = World(config)

    # ── Compile observation plan with AgentDisk ───────────────
    #
    # Each agent observes both fields through a local disk.
    # With AgentDisk, the observation size is fixed regardless
    # of grid size — it's determined by the disk radius only.
    #
    obs_entries = [
        ObsEntry(
            PREDATOR_FIELD,
            region_type=RegionType.AgentDisk,
            region_params=[PERCEPTION_RADIUS],
        ),
        ObsEntry(
            PREY_FIELD,
            region_type=RegionType.AgentDisk,
            region_params=[PERCEPTION_RADIUS],
        ),
    ]

    plan = ObsPlan(world, obs_entries)

    n_agents = 2
    obs_per_agent = plan.output_len
    mask_per_agent = plan.mask_len

    print(f"  Obs per agent: {obs_per_agent} floats")
    print(f"  Mask per agent: {mask_per_agent} bytes")
    print(f"  (Compare: full grid would be {CELL_COUNT * 2} floats)")
    print()

    # Pre-allocate batched buffers.
    obs_buf = np.zeros(n_agents * obs_per_agent, dtype=np.float32)
    mask_buf = np.zeros(n_agents * mask_per_agent, dtype=np.uint8)

    rng = np.random.default_rng(42)

    # ── Run episodes ──────────────────────────────────────────

    for episode in range(N_EPISODES):
        world.reset(episode)

        # Random starting positions (ensure they're apart).
        pred_q, pred_r = int(rng.integers(0, COLS)), int(rng.integers(0, ROWS))
        prey_q, prey_r = int(rng.integers(0, COLS)), int(rng.integers(0, ROWS))
        while hex_distance(pred_q, pred_r, prey_q, prey_r) < 4:
            prey_q = int(rng.integers(0, COLS))
            prey_r = int(rng.integers(0, ROWS))

        # Place agents via commands + step.
        cmds = [
            Command.set_field(PREDATOR_FIELD, [pred_q, pred_r], 1.0),
            Command.set_field(PREY_FIELD, [prey_q, prey_r], 1.0),
        ]
        world.step(cmds)

        total_pred_reward = 0.0
        caught = False

        for tick in range(MAX_TICKS):
            # ── Observe ───────────────────────────────────────
            # Build agent centers array: shape (2, 2) for 2D hex.
            agent_centers = np.array(
                [[pred_q, pred_r], [prey_q, prey_r]],
                dtype=np.int32,
            )
            plan.execute_agents(world, agent_centers, obs_buf, mask_buf)

            # obs_buf layout: [pred_obs (obs_per_agent floats), prey_obs (obs_per_agent floats)]
            pred_obs = obs_buf[:obs_per_agent]
            prey_obs = obs_buf[obs_per_agent:]

            # ── Decide actions (random policy) ────────────────
            # 0-5 = hex directions, 6 = stay
            pred_action = int(rng.integers(0, 7))
            prey_action = int(rng.integers(0, 7))

            # ── Move agents ───────────────────────────────────
            # Clear old positions.
            old_pred_q, old_pred_r = pred_q, pred_r
            old_prey_q, old_prey_r = prey_q, prey_r

            if pred_action < 6:
                pred_q, pred_r = hex_move(pred_q, pred_r, pred_action, COLS, ROWS)
            if prey_action < 6:
                prey_q, prey_r = hex_move(prey_q, prey_r, prey_action, COLS, ROWS)

            # ── Step world with new positions ─────────────────
            cmds = [
                # Clear old positions.
                Command.set_field(PREDATOR_FIELD, [old_pred_q, old_pred_r], 0.0),
                Command.set_field(PREY_FIELD, [old_prey_q, old_prey_r], 0.0),
                # Set new positions.
                Command.set_field(PREDATOR_FIELD, [pred_q, pred_r], 1.0),
                Command.set_field(PREY_FIELD, [prey_q, prey_r], 1.0),
            ]
            world.step(cmds)

            # ── Reward ────────────────────────────────────────
            dist = hex_distance(pred_q, pred_r, prey_q, prey_r)
            pred_reward = -float(dist)  # Predator: minimize distance
            total_pred_reward += pred_reward

            # ── Termination ───────────────────────────────────
            if dist == 0:
                caught = True
                total_pred_reward += 50.0  # Catch bonus
                break

        status = "CAUGHT" if caught else "ESCAPED"
        print(
            f"  Episode {episode + 1}: {status} at tick {tick + 1:>3}, "
            f"pred_reward={total_pred_reward:>7.1f}, "
            f"final_dist={hex_distance(pred_q, pred_r, prey_q, prey_r)}"
        )

    world.destroy()
    print()
    print("Done. With RL training, the predator learns to chase efficiently")
    print("while the prey learns evasion — both using only local observations.")


if __name__ == "__main__":
    main()
