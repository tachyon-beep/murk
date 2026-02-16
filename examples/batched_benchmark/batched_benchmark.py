"""Batched vs Sequential benchmark: measure the GIL elimination payoff.

Standalone timing script comparing BatchedVecEnv against MurkVecEnv
across different world counts and propagator complexities.

Usage:
    cd crates/murk-python && maturin develop --release
    python examples/batched_benchmark/batched_benchmark.py
"""

from __future__ import annotations

import sys
import os
import time

import numpy as np

import murk
from murk import (
    BatchedVecEnv,
    BatchedWorld,
    Command,
    Config,
    EdgeBehavior,
    FieldMutability,
    FieldType,
    MurkVecEnv,
    ObsEntry,
    RegionType,
    WriteMode,
)
from murk.env import MurkEnv


# ─── Configurable parameters ─────────────────────────────────────

GRID_W, GRID_H = 16, 16
CELL_COUNT = GRID_W * GRID_H
HEAT_FIELD = 0
DIFFUSION_COEFF = 0.1
SOURCE_INTENSITY = 10.0
HEAT_DECAY = 0.005


# ─── Propagator ──────────────────────────────────────────────────

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
    new_heat[GRID_H - 2, GRID_W - 2] = SOURCE_INTENSITY
    np.maximum(new_heat, 0.0, out=new_heat)
    out[:] = new_heat.ravel()


# ─── Config builders ─────────────────────────────────────────────

def make_config(seed: int) -> Config:
    config = Config()
    config.set_space_square4(GRID_W, GRID_H, EdgeBehavior.Absorb)
    config.add_field("heat", FieldType.Scalar, FieldMutability.PerTick)
    config.set_dt(1.0)
    config.set_seed(seed)
    murk.add_propagator(
        config,
        name="diffusion",
        step_fn=diffusion_step,
        reads_previous=[HEAT_FIELD],
        writes=[(HEAT_FIELD, WriteMode.Full)],
    )
    return config


def make_obs_entries() -> list[ObsEntry]:
    return [ObsEntry(HEAT_FIELD, region_type=RegionType.All)]


# ─── Single-env wrapper (for MurkVecEnv baseline) ────────────────

class BenchmarkEnv(MurkEnv):
    """Minimal MurkEnv for benchmark comparison."""

    def __init__(self, seed: int = 0):
        config = make_config(seed)
        obs_entries = make_obs_entries()
        super().__init__(config=config, obs_entries=obs_entries, n_actions=1, seed=seed)


# ─── Batched wrapper (for BatchedVecEnv) ─────────────────────────

class BenchmarkBatchedEnv(BatchedVecEnv):
    """Minimal BatchedVecEnv for benchmark comparison."""

    def __init__(self, num_envs: int, base_seed: int = 0):
        super().__init__(
            config_factory=lambda i: make_config(base_seed + i),
            obs_entries=make_obs_entries(),
            num_envs=num_envs,
        )


# ─── Benchmark functions ─────────────────────────────────────────

def bench_batched(num_envs: int, num_steps: int, warmup: int = 10) -> dict:
    """Benchmark BatchedVecEnv. Returns timing stats."""
    env = BenchmarkBatchedEnv(num_envs=num_envs)
    env.reset(seed=0)
    empty_cmds: list[list] = [[] for _ in range(num_envs)]

    # Warmup (not timed).
    for _ in range(warmup):
        env.step(np.zeros(num_envs, dtype=np.int32))

    # Timed run.
    t0 = time.perf_counter()
    for _ in range(num_steps):
        env.step(np.zeros(num_envs, dtype=np.int32))
    elapsed = time.perf_counter() - t0

    env.close()
    total = num_envs * num_steps
    return {
        "total_world_steps": total,
        "elapsed_s": elapsed,
        "world_steps_per_s": total / elapsed,
        "wall_steps_per_s": num_steps / elapsed,
    }


def bench_vecenv(num_envs: int, num_steps: int, warmup: int = 10) -> dict:
    """Benchmark MurkVecEnv (sequential Python loop). Returns timing stats."""
    env = MurkVecEnv([
        lambda i=i: BenchmarkEnv(seed=i) for i in range(num_envs)
    ])
    env.reset(seed=[i for i in range(num_envs)])

    # Warmup (not timed).
    for _ in range(warmup):
        env.step(np.zeros(num_envs, dtype=np.int32))

    # Timed run.
    t0 = time.perf_counter()
    for _ in range(num_steps):
        env.step(np.zeros(num_envs, dtype=np.int32))
    elapsed = time.perf_counter() - t0

    env.close()
    total = num_envs * num_steps
    return {
        "total_world_steps": total,
        "elapsed_s": elapsed,
        "world_steps_per_s": total / elapsed,
        "wall_steps_per_s": num_steps / elapsed,
    }


def bench_raw_batched_world(num_envs: int, num_steps: int, warmup: int = 10) -> dict:
    """Benchmark raw BatchedWorld API (lowest overhead). Returns timing stats."""
    configs = [make_config(seed=i) for i in range(num_envs)]
    obs_entries = make_obs_entries()

    with BatchedWorld(configs, obs_entries) as engine:
        obs = np.zeros(num_envs * engine.obs_output_len, dtype=np.float32)
        mask = np.zeros(num_envs * engine.obs_mask_len, dtype=np.uint8)
        empty_cmds: list[list] = [[] for _ in range(num_envs)]

        # Warmup.
        for _ in range(warmup):
            engine.step_and_observe(empty_cmds, obs, mask)

        # Timed run.
        t0 = time.perf_counter()
        for _ in range(num_steps):
            engine.step_and_observe(empty_cmds, obs, mask)
        elapsed = time.perf_counter() - t0

    total = num_envs * num_steps
    return {
        "total_world_steps": total,
        "elapsed_s": elapsed,
        "world_steps_per_s": total / elapsed,
        "wall_steps_per_s": num_steps / elapsed,
    }


# ─── Main ────────────────────────────────────────────────────────

def main():
    print("=" * 72)
    print("  Batched vs Sequential Benchmark")
    print("=" * 72)
    print()
    print(f"  Grid: {GRID_W}x{GRID_H} ({CELL_COUNT} cells)")
    print(f"  Propagator: diffusion (numpy, reads_previous + writes)")
    print(f"  Observation: full field ({CELL_COUNT} floats per world)")
    print()

    num_steps = 200
    env_counts = [1, 2, 4, 8, 16, 32, 64]

    # Header
    print(f"{'N':>5s}  "
          f"{'Raw Batched':>14s}  "
          f"{'BatchedVecEnv':>14s}  "
          f"{'MurkVecEnv':>14s}  "
          f"{'Speedup':>8s}  "
          f"{'Raw Speedup':>11s}")
    print(f"{'':>5s}  "
          f"{'(steps/s)':>14s}  "
          f"{'(steps/s)':>14s}  "
          f"{'(steps/s)':>14s}  "
          f"{'(BVE/MVE)':>8s}  "
          f"{'(Raw/MVE)':>11s}")
    print("-" * 72)

    for n in env_counts:
        raw = bench_raw_batched_world(n, num_steps)
        batched = bench_batched(n, num_steps)
        vecenv = bench_vecenv(n, num_steps)

        speedup = batched["world_steps_per_s"] / vecenv["world_steps_per_s"]
        raw_speedup = raw["world_steps_per_s"] / vecenv["world_steps_per_s"]

        print(
            f"{n:5d}  "
            f"{raw['world_steps_per_s']:14,.0f}  "
            f"{batched['world_steps_per_s']:14,.0f}  "
            f"{vecenv['world_steps_per_s']:14,.0f}  "
            f"{speedup:7.2f}x  "
            f"{raw_speedup:10.2f}x"
        )

    print()
    print("Legend:")
    print("  Raw Batched   = BatchedWorld.step_and_observe() directly (no Python env layer)")
    print("  BatchedVecEnv = Python subclass with override hooks")
    print("  MurkVecEnv    = Python loop over N independent MurkEnv instances")
    print("  Speedup       = BatchedVecEnv / MurkVecEnv throughput ratio")
    print("  Raw Speedup   = Raw BatchedWorld / MurkVecEnv throughput ratio")
    print()


if __name__ == "__main__":
    main()
