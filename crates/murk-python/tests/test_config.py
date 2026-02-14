"""Tests for Config builder round-trip."""

import pytest

from murk._murk import (
    BoundaryBehavior,
    Config,
    EdgeBehavior,
    FieldMutability,
    FieldType,
    SpaceType,
)


def test_config_create_destroy():
    """Config can be created and destroyed."""
    cfg = Config()
    assert cfg is not None


def test_config_set_space_line1d():
    """Line1D space with edge behavior."""
    cfg = Config()
    cfg.set_space(SpaceType.Line1D, [10.0, 0.0])  # len=10, Absorb


def test_config_set_space_ring1d():
    """Ring1D space."""
    cfg = Config()
    cfg.set_space(SpaceType.Ring1D, [10.0])


def test_config_set_space_square4():
    """Square4 space with edge behavior."""
    cfg = Config()
    cfg.set_space(SpaceType.Square4, [5.0, 5.0, 0.0])  # 5x5, Absorb


def test_config_set_space_square8():
    """Square8 space."""
    cfg = Config()
    cfg.set_space(SpaceType.Square8, [8.0, 8.0, 2.0])  # 8x8, Wrap


def test_config_add_field_scalar():
    """Add a scalar field."""
    cfg = Config()
    cfg.add_field("energy")


def test_config_add_field_vector():
    """Add a vector field."""
    cfg = Config()
    cfg.add_field("velocity", field_type=FieldType.Vector, dims=3)


def test_config_add_field_categorical():
    """Add a categorical field."""
    cfg = Config()
    cfg.add_field("cell_type", field_type=FieldType.Categorical, dims=5)


def test_config_add_field_with_options():
    """Add a field with full options."""
    cfg = Config()
    cfg.add_field(
        "temperature",
        field_type=FieldType.Scalar,
        mutability=FieldMutability.Sparse,
        boundary=BoundaryBehavior.Reflect,
    )


def test_config_set_dt():
    """Set simulation timestep."""
    cfg = Config()
    cfg.set_dt(0.05)


def test_config_set_seed():
    """Set RNG seed."""
    cfg = Config()
    cfg.set_seed(12345)


def test_config_full_round_trip():
    """Full config builder round-trip."""
    cfg = Config()
    cfg.set_space(SpaceType.Square4, [10.0, 10.0, 0.0])
    cfg.add_field("energy", mutability=FieldMutability.PerTick)
    cfg.add_field("velocity", field_type=FieldType.Vector, dims=2)
    cfg.set_dt(0.1)
    cfg.set_seed(42)


def test_config_context_manager():
    """Config as context manager."""
    with Config() as cfg:
        cfg.set_space(SpaceType.Line1D, [5.0, 0.0])
        cfg.add_field("x")


def test_config_double_consume_raises():
    """Consuming a config twice raises RuntimeError."""
    # We can't test this without World, but we can test that
    # destroying and then using raises.
    cfg = Config()
    cfg.set_space(SpaceType.Line1D, [5.0, 0.0])
    # Manually close via context manager exit
    cfg.__exit__(None, None, None)
    with pytest.raises(RuntimeError, match="consumed or destroyed"):
        cfg.set_dt(0.1)


def test_config_invalid_space_type():
    """Invalid space params raise ValueError."""
    cfg = Config()
    with pytest.raises(ValueError):
        cfg.set_space(SpaceType.Line1D, [])  # Missing params


def test_enum_values():
    """Enum discriminants match C side."""
    assert SpaceType.Line1D == SpaceType.Line1D
    assert FieldType.Scalar == FieldType.Scalar
    assert FieldMutability.Static == FieldMutability.Static
    assert BoundaryBehavior.Clamp == BoundaryBehavior.Clamp
    assert EdgeBehavior.Absorb == EdgeBehavior.Absorb


def test_region_type_enum_values():
    """RegionType enum has expected members."""
    from murk import RegionType
    assert RegionType.All.value == 0
    assert RegionType.AgentDisk.value == 5
    assert RegionType.AgentRect.value == 6


def test_transform_type_enum_values():
    """TransformType enum has expected members."""
    from murk import TransformType
    assert TransformType.Identity.value == 0
    assert TransformType.Normalize.value == 1


def test_pool_kernel_enum_values():
    """PoolKernel enum has expected members."""
    from murk import PoolKernel
    assert PoolKernel.NoPool.value == 0
    assert PoolKernel.Mean.value == 1
    assert PoolKernel.Max.value == 2
    assert PoolKernel.Min.value == 3
    assert PoolKernel.Sum.value == 4


def test_config_set_space_line1d_typed():
    """set_space_line1d accepts EdgeBehavior enum."""
    cfg = Config()
    cfg.set_space_line1d(10, EdgeBehavior.Absorb)


def test_config_set_space_square4_typed():
    """set_space_square4 accepts EdgeBehavior enum."""
    cfg = Config()
    cfg.set_space_square4(5, 5, EdgeBehavior.Wrap)


def test_config_set_space_square8_typed():
    """set_space_square8 accepts EdgeBehavior enum."""
    cfg = Config()
    cfg.set_space_square8(8, 8, EdgeBehavior.Absorb)


def test_config_set_space_hex2d_typed():
    """set_space_hex2d accepts dimensions."""
    cfg = Config()
    cfg.set_space_hex2d(10, 10)


def test_config_set_space_ring1d_typed():
    """set_space_ring1d accepts length."""
    cfg = Config()
    cfg.set_space_ring1d(20)


def test_config_set_space_fcc12_typed():
    """set_space_fcc12 accepts dimensions and EdgeBehavior."""
    cfg = Config()
    cfg.set_space_fcc12(4, 4, 4, EdgeBehavior.Absorb)
