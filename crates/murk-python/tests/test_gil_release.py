"""Tests for GIL release: concurrent worlds make progress in threads."""

import threading
import time

import numpy as np
import pytest

from murk._murk import (
    Config,
    FieldMutability,
    PropagatorDef,
    SpaceType,
    World,
    WriteMode,
)


def _make_world(seed=42, n_cells=100):
    """Helper: create a world for threading tests."""

    def step_fn(reads, reads_prev, writes, tick_id, dt, cell_count):
        # Do some work to make step non-trivial
        writes[0][:] = float(tick_id) * 0.1

    cfg = Config()
    cfg.set_space(SpaceType.Line1D, [float(n_cells), 0.0])
    cfg.add_field("x", mutability=FieldMutability.PerTick)
    cfg.set_dt(0.01)
    cfg.set_seed(seed)

    prop = PropagatorDef("work", step_fn, writes=[(0, WriteMode.Full)])
    prop.register(cfg)

    return World(cfg)


def test_concurrent_worlds_make_progress():
    """N threads stepping N worlds concurrently all complete."""
    n_threads = 4
    n_steps = 50
    results = [None] * n_threads

    def worker(idx):
        world = _make_world(seed=idx)
        for _ in range(n_steps):
            world.step()
        results[idx] = world.current_tick
        world.destroy()

    threads = [threading.Thread(target=worker, args=(i,)) for i in range(n_threads)]
    start = time.monotonic()
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=30)
    elapsed = time.monotonic() - start

    for i, r in enumerate(results):
        assert r == n_steps, f"Thread {i} only completed {r}/{n_steps} steps"


def test_concurrent_faster_than_sequential():
    """Concurrent execution is not dramatically slower than sequential.

    Due to GIL release, concurrent should be roughly similar in wall time
    to sequential (the engine does real work outside GIL). We check that
    concurrent takes less than 3x sequential to account for variance.
    """
    n_steps = 100

    # Sequential timing
    start = time.monotonic()
    for i in range(2):
        world = _make_world(seed=i, n_cells=50)
        for _ in range(n_steps):
            world.step()
        world.destroy()
    seq_time = time.monotonic() - start

    # Concurrent timing
    def worker():
        world = _make_world(seed=99, n_cells=50)
        for _ in range(n_steps):
            world.step()
        world.destroy()

    threads = [threading.Thread(target=worker) for _ in range(2)]
    start = time.monotonic()
    for t in threads:
        t.start()
    for t in threads:
        t.join(timeout=30)
    par_time = time.monotonic() - start

    # Concurrent should not be much slower than sequential
    # (ideally faster, but Python propagator re-acquires GIL)
    assert par_time < seq_time * 3, (
        f"Concurrent ({par_time:.3f}s) was >3x slower than sequential ({seq_time:.3f}s)"
    )
