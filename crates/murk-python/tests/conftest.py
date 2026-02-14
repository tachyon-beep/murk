"""Shared fixtures for murk Python tests."""

import numpy as np
import pytest

from murk._murk import (
    Config,
    FieldMutability,
    FieldType,
    ObsEntry,
    ObsPlan,
    PropagatorDef,
    SpaceType,
    World,
    WriteMode,
)


def make_const_world(value=7.0, n_cells=10, seed=42):
    """Create a world with a constant-value propagator.

    Returns (world, obs_plan) with a single scalar field.
    """

    def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
        writes[0][:] = value

    cfg = Config()
    cfg.set_space(SpaceType.Line1D, [float(n_cells), 0.0])
    cfg.add_field("energy", mutability=FieldMutability.PerTick)
    cfg.set_dt(0.1)
    cfg.set_seed(seed)

    prop = PropagatorDef("const", step_fn, writes=[(0, WriteMode.Full)])
    prop.register(cfg)

    world = World(cfg)

    entries = [ObsEntry(0)]
    plan = ObsPlan(world, entries)

    return world, plan


def make_grid_world(width=5, height=5, n_fields=4, seed=42, step_fn=None):
    """Create a Square4 world with multiple scalar fields.

    Fields: field_0, field_1, ..., field_{n_fields-1}
    Returns (world, obs_plan).
    """
    if step_fn is None:

        def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
            for w in writes:
                w[:] = 0.0

    cfg = Config()
    cfg.set_space(SpaceType.Square4, [float(width), float(height), 0.0])
    for i in range(n_fields):
        cfg.add_field(f"field_{i}", mutability=FieldMutability.PerTick)
    cfg.set_dt(0.1)
    cfg.set_seed(seed)

    writes = [(i, WriteMode.Full) for i in range(n_fields)]
    prop = PropagatorDef("grid_prop", step_fn, writes=writes)
    prop.register(cfg)

    world = World(cfg)

    entries = [ObsEntry(i) for i in range(n_fields)]
    plan = ObsPlan(world, entries)

    return world, plan
