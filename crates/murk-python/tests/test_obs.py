"""Tests for ObsPlan compile, execute, and numpy buffer correctness."""

import numpy as np
import pytest

from murk._murk import ObsEntry, ObsPlan

from conftest import make_const_world, make_grid_world


def test_obsplan_compile_and_lengths():
    """ObsPlan compilation reports correct output/mask lengths."""
    world, plan = make_const_world(n_cells=10)
    assert plan.output_len == 10
    assert plan.mask_len == 10
    world.destroy()


def test_obsplan_execute_matches_field():
    """ObsPlan execute fills buffer with correct field values."""
    world, plan = make_const_world(value=3.14, n_cells=10)
    world.step()

    obs = np.zeros(plan.output_len, dtype=np.float32)
    mask = np.zeros(plan.mask_len, dtype=np.uint8)
    tick_id, age = plan.execute(world, obs, mask)

    assert tick_id == 1
    assert age == 0
    np.testing.assert_allclose(obs, 3.14, rtol=1e-5)
    np.testing.assert_array_equal(mask, 1)
    world.destroy()


def test_obsplan_multiple_fields():
    """ObsPlan with multiple fields concatenates correctly."""
    world, plan = make_grid_world(width=3, height=3, n_fields=2)
    world.step()

    # 3x3 grid = 9 cells, 2 fields = 18 output elements
    assert plan.output_len == 18
    obs = np.zeros(plan.output_len, dtype=np.float32)
    mask = np.zeros(plan.mask_len, dtype=np.uint8)
    plan.execute(world, obs, mask)

    # All zeros from default step_fn
    np.testing.assert_array_equal(obs, 0.0)
    world.destroy()


def test_obsplan_context_manager():
    """ObsPlan works as context manager."""
    world, plan = make_const_world()
    with plan:
        world.step()
        obs = np.zeros(plan.output_len, dtype=np.float32)
        mask = np.zeros(plan.mask_len, dtype=np.uint8)
        plan.execute(world, obs, mask)
    world.destroy()


def test_obsplan_normalize_transform():
    """Normalize transform scales values to [0, 1]."""
    from murk import TransformType
    world, _ = make_const_world(value=5.0, n_cells=10)
    world.step()

    # Normalize from [0, 10] => 5.0 -> 0.5
    entries = [ObsEntry(0, transform_type=TransformType.Normalize, normalize_min=0.0, normalize_max=10.0)]
    plan = ObsPlan(world, entries)

    obs = np.zeros(plan.output_len, dtype=np.float32)
    mask = np.zeros(plan.mask_len, dtype=np.uint8)
    plan.execute(world, obs, mask)

    np.testing.assert_allclose(obs, 0.5, rtol=1e-5)
    world.destroy()


def test_obsplan_reuse_across_steps():
    """Same ObsPlan can be executed across multiple steps."""
    world, plan = make_const_world(value=1.0)
    obs = np.zeros(plan.output_len, dtype=np.float32)
    mask = np.zeros(plan.mask_len, dtype=np.uint8)

    for i in range(5):
        world.step()
        tick_id, _ = plan.execute(world, obs, mask)
        assert tick_id == i + 1
        np.testing.assert_array_equal(obs, 1.0)

    world.destroy()


def test_obsentry_accepts_enum_types():
    """ObsEntry accepts RegionType and TransformType enums."""
    from murk import RegionType, TransformType, PoolKernel
    entry = ObsEntry(
        0,
        region_type=RegionType.All,
        transform_type=TransformType.Identity,
        pool_kernel=PoolKernel.NoPool,
    )


def test_obsentry_normalize_with_enum():
    """ObsEntry with TransformType.Normalize works end-to-end."""
    from murk import TransformType
    world, _ = make_const_world(value=5.0, n_cells=10)
    world.step()

    entries = [ObsEntry(0, transform_type=TransformType.Normalize,
                        normalize_min=0.0, normalize_max=10.0)]
    plan = ObsPlan(world, entries)

    obs = np.zeros(plan.output_len, dtype=np.float32)
    mask = np.zeros(plan.mask_len, dtype=np.uint8)
    plan.execute(world, obs, mask)

    np.testing.assert_allclose(obs, 0.5, rtol=1e-5)
    world.destroy()
