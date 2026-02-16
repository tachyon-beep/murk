"""BatchedWorld Cookbook: direct low-level API for power users.

Shows how to use BatchedWorld directly (without BatchedVecEnv)
for fine-grained control over the step/observe/reset cycle.

This is useful when:
  - You need custom observation timing (observe at different cadences)
  - You're building a non-Gymnasium training loop
  - You want maximum control over the auto-reset policy
  - You're integrating with a framework that has its own env abstraction

Usage:
    cd crates/murk-python && maturin develop --release
    python examples/batched_cookbook/batched_cookbook.py
"""

from __future__ import annotations

import numpy as np

import murk
from murk import (
    BatchedWorld,
    Command,
    Config,
    EdgeBehavior,
    FieldMutability,
    FieldType,
    ObsEntry,
    RegionType,
    WriteMode,
)


# ─── Shared config ────────────────────────────────────────────────

GRID_W, GRID_H = 8, 8
CELL_COUNT = GRID_W * GRID_H
HEAT_FIELD = 0
SOURCE_INTENSITY = 5.0


def make_config(seed: int) -> Config:
    """Build a minimal config with a constant-source propagator."""
    config = Config()
    config.set_space_square4(GRID_W, GRID_H, EdgeBehavior.Absorb)
    config.add_field("heat", FieldType.Scalar, FieldMutability.PerTick)
    config.set_dt(1.0)
    config.set_seed(seed)

    def source_step(reads, reads_prev, writes, tick_id, dt, cell_count):
        out = writes[0]
        out[0] = SOURCE_INTENSITY  # cell (0,0)

    murk.add_propagator(
        config,
        name="source",
        step_fn=source_step,
        writes=[(HEAT_FIELD, WriteMode.Full)],
    )
    return config


def make_obs_entries() -> list[ObsEntry]:
    return [ObsEntry(HEAT_FIELD, region_type=RegionType.All)]


# ═══════════════════════════════════════════════════════════════════
# Recipe 1: Basic lifecycle (create, step, observe, destroy)
# ═══════════════════════════════════════════════════════════════════

def recipe_basic_lifecycle():
    """Create a BatchedWorld, step it, read observations, destroy it."""
    print("Recipe 1: Basic lifecycle")
    print("-" * 50)

    N = 4
    configs = [make_config(seed=i) for i in range(N)]
    obs_entries = make_obs_entries()

    # Create the batched engine. This consumes all configs.
    engine = BatchedWorld(configs, obs_entries)
    print(f"  Created {engine.num_worlds} worlds")
    print(f"  Obs per world: {engine.obs_output_len} floats")
    print(f"  Mask per world: {engine.obs_mask_len} bytes")

    # Pre-allocate output buffers (reuse across steps).
    obs = np.zeros(N * engine.obs_output_len, dtype=np.float32)
    mask = np.zeros(N * engine.obs_mask_len, dtype=np.uint8)

    # Step all worlds with no commands.
    empty_cmds: list[list] = [[] for _ in range(N)]
    tick_ids = engine.step_and_observe(empty_cmds, obs, mask)
    print(f"  After step: tick_ids = {tick_ids}")

    # Reshape obs to (N, obs_len) for per-world access.
    obs_2d = obs.reshape(N, engine.obs_output_len)
    for i in range(N):
        val_at_origin = obs_2d[i, 0]
        print(f"  World {i}: heat at (0,0) = {val_at_origin:.1f}")

    engine.destroy()
    print("  Destroyed.")
    print()


# ═══════════════════════════════════════════════════════════════════
# Recipe 2: Context manager (automatic cleanup)
# ═══════════════════════════════════════════════════════════════════

def recipe_context_manager():
    """Use BatchedWorld as a context manager for guaranteed cleanup."""
    print("Recipe 2: Context manager")
    print("-" * 50)

    N = 2
    configs = [make_config(seed=i) for i in range(N)]

    with BatchedWorld(configs, make_obs_entries()) as engine:
        obs = np.zeros(N * engine.obs_output_len, dtype=np.float32)
        mask = np.zeros(N * engine.obs_mask_len, dtype=np.uint8)

        tick_ids = engine.step_and_observe([[], []], obs, mask)
        print(f"  Step 1: tick_ids = {tick_ids}")

        tick_ids = engine.step_and_observe([[], []], obs, mask)
        print(f"  Step 2: tick_ids = {tick_ids}")

    # engine.destroy() called automatically by __exit__
    print("  Engine auto-destroyed by context manager.")
    print()


# ═══════════════════════════════════════════════════════════════════
# Recipe 3: Per-world commands
# ═══════════════════════════════════════════════════════════════════

def recipe_per_world_commands():
    """Send different commands to different worlds."""
    print("Recipe 3: Per-world commands")
    print("-" * 50)

    N = 3
    configs = [make_config(seed=i) for i in range(N)]

    with BatchedWorld(configs, make_obs_entries()) as engine:
        obs = np.zeros(N * engine.obs_output_len, dtype=np.float32)
        mask = np.zeros(N * engine.obs_mask_len, dtype=np.uint8)

        # Step once to populate propagator outputs.
        engine.step_and_observe([[], [], []], obs, mask)

        # Each world gets a different SetField command at cell (1,0).
        # commands_per_world[i] is a list of Commands for world i.
        commands = [
            [Command.set_field(HEAT_FIELD, [1, 0], 100.0)],   # world 0
            [Command.set_field(HEAT_FIELD, [1, 0], 200.0)],   # world 1
            [],                                                 # world 2: no cmd
        ]
        tick_ids = engine.step_and_observe(commands, obs, mask)

        obs_2d = obs.reshape(N, engine.obs_output_len)
        # Cell (1,0) = row 1, col 0 → flat index = GRID_W * 1 + 0 = 8
        idx = GRID_W * 1 + 0
        for i in range(N):
            print(f"  World {i}: heat at (1,0) = {obs_2d[i, idx]:.1f}")

    print()


# ═══════════════════════════════════════════════════════════════════
# Recipe 4: Reset individual worlds
# ═══════════════════════════════════════════════════════════════════

def recipe_selective_reset():
    """Reset individual worlds without affecting others."""
    print("Recipe 4: Selective reset")
    print("-" * 50)

    N = 3
    configs = [make_config(seed=i) for i in range(N)]

    with BatchedWorld(configs, make_obs_entries()) as engine:
        obs = np.zeros(N * engine.obs_output_len, dtype=np.float32)
        mask = np.zeros(N * engine.obs_mask_len, dtype=np.uint8)

        # Step 10 times.
        for _ in range(10):
            engine.step_and_observe([[], [], []], obs, mask)

        # Now reset only world 1, leaving worlds 0 and 2 running.
        engine.reset_world(1, seed=999)

        # Observe all to see the effect.
        engine.observe_all(obs, mask)
        obs_2d = obs.reshape(N, engine.obs_output_len)
        for i in range(N):
            total_heat = obs_2d[i].sum()
            print(f"  World {i}: total heat = {total_heat:.1f}")

    print("  (After reset, the engine runs an initial tick, so the source")
    print("   propagator has already populated cell (0,0).)")
    print()


# ═══════════════════════════════════════════════════════════════════
# Recipe 5: Observe without stepping
# ═══════════════════════════════════════════════════════════════════

def recipe_observe_only():
    """Extract observations without advancing the simulation."""
    print("Recipe 5: Observe without stepping")
    print("-" * 50)

    N = 2
    configs = [make_config(seed=i) for i in range(N)]

    with BatchedWorld(configs, make_obs_entries()) as engine:
        obs = np.zeros(N * engine.obs_output_len, dtype=np.float32)
        mask = np.zeros(N * engine.obs_mask_len, dtype=np.uint8)

        # Step once to have data.
        tick_ids = engine.step_and_observe([[], []], obs, mask)
        first_obs = obs.copy()

        # Observe again without stepping — should get identical data.
        engine.observe_all(obs, mask)
        assert np.array_equal(first_obs, obs), "Obs should be identical!"
        print("  observe_all returns same data as last step_and_observe.")

        # Step again — tick advances but obs is the same here because
        # the source propagator is deterministic (always writes 5.0).
        tick_ids = engine.step_and_observe([[], []], obs, mask)
        print(f"  After second step: tick_ids = {tick_ids}")
        print(f"  Obs changed: {not np.array_equal(first_obs, obs)}"
              f" (deterministic propagator → same output each tick)")

    print()


# ═══════════════════════════════════════════════════════════════════
# Recipe 6: Reset-all with different seeds
# ═══════════════════════════════════════════════════════════════════

def recipe_reset_all():
    """Reset all worlds at once with distinct seeds."""
    print("Recipe 6: Reset all with different seeds")
    print("-" * 50)

    N = 4
    configs = [make_config(seed=i) for i in range(N)]

    with BatchedWorld(configs, make_obs_entries()) as engine:
        obs = np.zeros(N * engine.obs_output_len, dtype=np.float32)
        mask = np.zeros(N * engine.obs_mask_len, dtype=np.uint8)

        # Step a few times.
        for _ in range(5):
            engine.step_and_observe([[] for _ in range(N)], obs, mask)

        # Reset all with new seeds.
        new_seeds = [100, 200, 300, 400]
        engine.reset_all(new_seeds)
        print(f"  Reset all with seeds: {new_seeds}")

        # Observe after reset (before any step).
        engine.observe_all(obs, mask)
        obs_2d = obs.reshape(N, engine.obs_output_len)
        for i in range(N):
            total_heat = obs_2d[i].sum()
            print(f"  World {i}: total heat = {total_heat:.1f}")

    print("  (After reset, the engine runs an initial tick so the propagator")
    print("   stamps the source cell. Fields are otherwise zeroed.)")
    print()


# ─── Main ────────────────────────────────────────────────────────

def main():
    print("=" * 50)
    print("  BatchedWorld Cookbook")
    print("=" * 50)
    print()

    recipe_basic_lifecycle()
    recipe_context_manager()
    recipe_per_world_commands()
    recipe_selective_reset()
    recipe_observe_only()
    recipe_reset_all()

    print("All recipes passed.")


if __name__ == "__main__":
    main()
