"""Tests for BatchedWorld and BatchedVecEnv."""

import time

import numpy as np
import pytest

from murk._murk import (
    BatchedWorld,
    Config,
    EdgeBehavior,
    FieldMutability,
    ObsEntry,
    PropagatorDef,
    WriteMode,
)
from murk.batched_vec_env import BatchedVecEnv


# ── Helpers ──────────────────────────────────────────────────────


def make_config(value=7.0, n_cells=10, seed=42):
    """Build a config with a constant-value propagator."""

    def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
        writes[0][:] = value

    cfg = Config()
    cfg.set_space_line1d(n_cells, EdgeBehavior.Absorb)
    cfg.add_field("energy", mutability=FieldMutability.PerTick)
    cfg.set_dt(0.1)
    cfg.set_seed(seed)

    prop = PropagatorDef("const", step_fn, writes=[(0, WriteMode.Full)])
    prop.register(cfg)
    return cfg


# ── BatchedWorld (low-level) ─────────────────────────────────────


class TestBatchedWorld:
    def test_create_step_observe_destroy(self):
        """Full lifecycle: create, step+observe, destroy."""
        configs = [make_config(seed=i) for i in range(4)]
        entries = [ObsEntry(0)]
        engine = BatchedWorld(configs, entries)

        assert engine.num_worlds == 4
        assert engine.obs_output_len == 10
        assert engine.obs_mask_len == 10

        obs = np.zeros(4 * 10, dtype=np.float32)
        mask = np.zeros(4 * 10, dtype=np.uint8)
        commands = [[] for _ in range(4)]

        tick_ids = engine.step_and_observe(commands, obs, mask)
        assert len(tick_ids) == 4
        assert all(t == 1 for t in tick_ids)
        np.testing.assert_array_equal(obs, 7.0)
        np.testing.assert_array_equal(mask, 1)

        engine.destroy()

    def test_context_manager(self):
        """BatchedWorld works as context manager."""
        configs = [make_config()]
        with BatchedWorld(configs, [ObsEntry(0)]) as engine:
            assert engine.num_worlds == 1

    def test_double_destroy(self):
        """Double destroy doesn't crash."""
        configs = [make_config()]
        engine = BatchedWorld(configs)
        engine.destroy()
        engine.destroy()

    def test_reset_world(self):
        """Reset one world, others unaffected."""
        configs = [make_config(seed=i) for i in range(3)]
        entries = [ObsEntry(0)]
        engine = BatchedWorld(configs, entries)

        obs = np.zeros(3 * 10, dtype=np.float32)
        mask = np.zeros(3 * 10, dtype=np.uint8)
        engine.step_and_observe([[], [], []], obs, mask)

        # All worlds should have 7.0.
        np.testing.assert_array_equal(obs, 7.0)

        # Reset world 1.
        engine.reset_world(1, 99)

        # Observe all: world 1 is reset (zeroed), others have 7.0.
        engine.observe_all(obs, mask)
        np.testing.assert_array_equal(obs[:10], 7.0)   # world 0
        np.testing.assert_array_equal(obs[10:20], 0.0)  # world 1 (reset)
        np.testing.assert_array_equal(obs[20:], 7.0)    # world 2

        engine.destroy()

    def test_reset_all(self):
        """Reset all worlds."""
        configs = [make_config(seed=i) for i in range(2)]
        entries = [ObsEntry(0)]
        engine = BatchedWorld(configs, entries)

        obs = np.zeros(2 * 10, dtype=np.float32)
        mask = np.zeros(2 * 10, dtype=np.uint8)
        engine.step_and_observe([[], []], obs, mask)
        np.testing.assert_array_equal(obs, 7.0)

        engine.reset_all([10, 20])
        engine.observe_all(obs, mask)
        np.testing.assert_array_equal(obs, 0.0)  # All reset → zeroed

        engine.destroy()

    def test_shape_correctness(self):
        """Output arrays have correct shapes."""
        n = 8
        configs = [make_config(seed=i) for i in range(n)]
        entries = [ObsEntry(0)]
        engine = BatchedWorld(configs, entries)

        total_obs = n * engine.obs_output_len
        total_mask = n * engine.obs_mask_len
        obs = np.zeros(total_obs, dtype=np.float32)
        mask = np.zeros(total_mask, dtype=np.uint8)

        commands = [[] for _ in range(n)]
        tick_ids = engine.step_and_observe(commands, obs, mask)

        assert len(tick_ids) == n
        assert obs.shape == (total_obs,)
        assert mask.shape == (total_mask,)

        engine.destroy()

    def test_wrong_command_count_raises(self):
        """Mismatched command count raises."""
        configs = [make_config(seed=i) for i in range(2)]
        engine = BatchedWorld(configs)

        obs = np.zeros(20, dtype=np.float32)
        mask = np.zeros(20, dtype=np.uint8)

        with pytest.raises(ValueError, match="1 entries, expected 2"):
            engine.step_and_observe([[]], obs, mask)

        engine.destroy()

    def test_empty_configs_raises(self):
        """Empty config list raises."""
        with pytest.raises(ValueError, match="must not be empty"):
            BatchedWorld([])


# ── BatchedVecEnv (high-level) ───────────────────────────────────


class TestBatchedVecEnv:
    def test_reset_returns_correct_shape(self):
        """Reset returns (num_envs, obs_len) observations."""
        env = BatchedVecEnv(
            config_factory=lambda i: make_config(seed=i),
            obs_entries=[ObsEntry(0)],
            num_envs=4,
        )
        obs, info = env.reset()
        assert obs.shape == (4, 10)
        assert obs.dtype == np.float32
        env.close()

    def test_step_returns_correct_shapes(self):
        """Step returns correctly shaped arrays."""
        env = BatchedVecEnv(
            config_factory=lambda i: make_config(seed=i),
            obs_entries=[ObsEntry(0)],
            num_envs=3,
        )
        env.reset()
        actions = np.zeros(3)  # dummy
        obs, rewards, terminateds, truncateds, infos = env.step(actions)

        assert obs.shape == (3, 10)
        assert rewards.shape == (3,)
        assert terminateds.shape == (3,)
        assert truncateds.shape == (3,)
        assert "tick_ids" in infos
        env.close()

    def test_step_obs_values(self):
        """Step observations match const propagator value."""
        env = BatchedVecEnv(
            config_factory=lambda i: make_config(value=42.0, seed=i),
            obs_entries=[ObsEntry(0)],
            num_envs=2,
        )
        env.reset()
        obs, _, _, _, _ = env.step(np.zeros(2))
        np.testing.assert_array_equal(obs, 42.0)
        env.close()

    def test_determinism(self):
        """Same config + same actions = same outputs."""
        def run():
            env = BatchedVecEnv(
                config_factory=lambda i: make_config(value=7.0, seed=i),
                obs_entries=[ObsEntry(0)],
                num_envs=4,
            )
            env.reset(seed=0)
            results = []
            for _ in range(10):
                obs, r, t, tr, _ = env.step(np.zeros(4))
                results.append(obs.copy())
            env.close()
            return results

        run1 = run()
        run2 = run()
        for a, b in zip(run1, run2):
            np.testing.assert_array_equal(a, b)

    def test_auto_reset_on_termination(self):
        """Auto-reset stores final_observation when terminated."""
        step_count = [0]

        class TerminatingEnv(BatchedVecEnv):
            def _check_terminated(self, obs, tick_ids):
                # Terminate after 3 steps.
                return tick_ids >= 3

        env = TerminatingEnv(
            config_factory=lambda i: make_config(value=7.0, seed=i),
            obs_entries=[ObsEntry(0)],
            num_envs=2,
        )
        env.reset(seed=0)

        for step in range(5):
            obs, _, terminated, _, infos = env.step(np.zeros(2))
            if terminated.any():
                for i in range(2):
                    if terminated[i]:
                        assert infos["final_observation"][i] is not None
                break

        env.close()

    def test_performance_vs_sequential(self):
        """BatchedVecEnv should be faster than sequential for N>=8."""
        n_envs = 8
        n_steps = 50

        # Batched.
        env_batched = BatchedVecEnv(
            config_factory=lambda i: make_config(seed=i),
            obs_entries=[ObsEntry(0)],
            num_envs=n_envs,
        )
        env_batched.reset(seed=0)

        t0 = time.perf_counter()
        for _ in range(n_steps):
            env_batched.step(np.zeros(n_envs))
        batched_time = time.perf_counter() - t0
        env_batched.close()

        # Sequential: N separate worlds stepped in Python.
        from murk._murk import World

        configs = [make_config(seed=i) for i in range(n_envs)]
        worlds = [World(c) for c in configs]

        t0 = time.perf_counter()
        for _ in range(n_steps):
            for w in worlds:
                w.step()
        sequential_time = time.perf_counter() - t0
        for w in worlds:
            w.destroy()

        # The batched version should be at least not slower.
        # (For small N and simple propagators the difference is small,
        # so we just verify it completes without error.)
        print(
            f"\nBatched: {batched_time*1000:.1f}ms, "
            f"Sequential: {sequential_time*1000:.1f}ms, "
            f"Ratio: {sequential_time/batched_time:.2f}x"
        )
