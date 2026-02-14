"""Tests for Python propagator trampoline."""

import numpy as np
import pytest

from murk._murk import (
    Config,
    EdgeBehavior,
    FieldMutability,
    ObsEntry,
    ObsPlan,
    PropagatorDef,
    World,
    WriteMode,
)


def test_python_propagator_writes_values():
    """Python propagator correctly writes values to field."""

    def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
        writes[0][:] = 42.0

    cfg = Config()
    cfg.set_space_line1d(5, EdgeBehavior.Absorb)
    cfg.add_field("x", mutability=FieldMutability.PerTick)
    cfg.set_dt(0.1)
    cfg.set_seed(0)

    prop = PropagatorDef("writer", step_fn, writes=[(0, WriteMode.Full)])
    prop.register(cfg)

    world = World(cfg)
    world.step()

    buf = np.zeros(5, dtype=np.float32)
    world.read_field(0, buf)
    np.testing.assert_array_equal(buf, 42.0)
    world.destroy()


def test_python_propagator_uses_tick_id():
    """Propagator receives correct tick_id."""

    def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
        writes[0][:] = float(tick_id)

    cfg = Config()
    cfg.set_space_line1d(5, EdgeBehavior.Absorb)
    cfg.add_field("x", mutability=FieldMutability.PerTick)
    cfg.set_dt(0.1)
    cfg.set_seed(0)

    prop = PropagatorDef("ticker", step_fn, writes=[(0, WriteMode.Full)])
    prop.register(cfg)

    world = World(cfg)

    for expected_tick in range(1, 4):
        world.step()
        buf = np.zeros(5, dtype=np.float32)
        world.read_field(0, buf)
        np.testing.assert_array_equal(buf, float(expected_tick))

    world.destroy()


def test_python_propagator_cell_count():
    """Propagator receives correct cell_count."""
    recorded_counts = []

    def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
        recorded_counts.append(cell_count)
        writes[0][:] = 0.0

    cfg = Config()
    cfg.set_space_square4(3, 4, EdgeBehavior.Absorb)  # 3x4 = 12 cells
    cfg.add_field("x", mutability=FieldMutability.PerTick)
    cfg.set_dt(0.1)
    cfg.set_seed(0)

    prop = PropagatorDef("counter", step_fn, writes=[(0, WriteMode.Full)])
    prop.register(cfg)

    world = World(cfg)
    world.step()
    assert recorded_counts[-1] == 12
    world.destroy()


def test_python_propagator_multiple_writes():
    """Propagator writing to multiple fields."""

    def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
        writes[0][:] = 1.0
        writes[1][:] = 2.0

    cfg = Config()
    cfg.set_space_line1d(5, EdgeBehavior.Absorb)
    cfg.add_field("a", mutability=FieldMutability.PerTick)
    cfg.add_field("b", mutability=FieldMutability.PerTick)
    cfg.set_dt(0.1)
    cfg.set_seed(0)

    prop = PropagatorDef("multi", step_fn, writes=[(0, WriteMode.Full), (1, WriteMode.Full)])
    prop.register(cfg)

    world = World(cfg)
    world.step()

    buf_a = np.zeros(5, dtype=np.float32)
    buf_b = np.zeros(5, dtype=np.float32)
    world.read_field(0, buf_a)
    world.read_field(1, buf_b)
    np.testing.assert_array_equal(buf_a, 1.0)
    np.testing.assert_array_equal(buf_b, 2.0)
    world.destroy()


def test_python_propagator_dt_passed():
    """Propagator receives correct dt."""
    recorded_dts = []

    def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
        recorded_dts.append(dt)
        writes[0][:] = 0.0

    cfg = Config()
    cfg.set_space_line1d(5, EdgeBehavior.Absorb)
    cfg.add_field("x", mutability=FieldMutability.PerTick)
    cfg.set_dt(0.05)
    cfg.set_seed(0)

    prop = PropagatorDef("dt_checker", step_fn, writes=[(0, WriteMode.Full)])
    prop.register(cfg)

    world = World(cfg)
    world.step()
    assert abs(recorded_dts[-1] - 0.05) < 1e-10
    world.destroy()


def test_python_propagator_convenience_function():
    """add_propagator convenience function works."""
    from murk._murk import add_propagator

    def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
        writes[0][:] = 99.0

    cfg = Config()
    cfg.set_space_line1d(5, EdgeBehavior.Absorb)
    cfg.add_field("x", mutability=FieldMutability.PerTick)
    cfg.set_dt(0.1)
    cfg.set_seed(0)

    add_propagator(cfg, "conv", step_fn, writes=[(0, WriteMode.Full)])

    world = World(cfg)
    world.step()

    buf = np.zeros(5, dtype=np.float32)
    world.read_field(0, buf)
    np.testing.assert_array_equal(buf, 99.0)
    world.destroy()


def test_propagator_accepts_write_mode_enum():
    """PropagatorDef accepts WriteMode enum in write tuples."""

    def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
        writes[0][:] = 1.0

    cfg = Config()
    cfg.set_space_line1d(5, EdgeBehavior.Absorb)
    cfg.add_field("x", mutability=FieldMutability.PerTick)
    cfg.set_dt(0.1)
    cfg.set_seed(0)

    prop = PropagatorDef("writer", step_fn, writes=[(0, WriteMode.Full)])
    prop.register(cfg)

    world = World(cfg)
    world.step()

    buf = np.zeros(5, dtype=np.float32)
    world.read_field(0, buf)
    np.testing.assert_array_equal(buf, 1.0)
    world.destroy()
