"""Tests for World lifecycle: create, step, reset, destroy, read_field."""

import numpy as np
import pytest

from murk._murk import Command, Config, FieldMutability, World, PropagatorDef

from conftest import make_const_world


def test_create_step_destroy():
    """World can be created, stepped, and destroyed."""
    world, plan = make_const_world()
    assert world.current_tick == 0
    receipts, metrics = world.step()
    assert world.current_tick == 1
    assert metrics.total_us > 0
    world.destroy()


def test_read_field_correct_values():
    """After stepping, field values match propagator output."""
    world, _ = make_const_world(value=42.0)
    world.step()
    buf = np.zeros(10, dtype=np.float32)
    world.read_field(0, buf)
    np.testing.assert_array_equal(buf, 42.0)
    world.destroy()


def test_reset_clears_tick():
    """Reset brings tick back to 0."""
    world, _ = make_const_world()
    world.step()
    assert world.current_tick == 1
    world.reset(99)
    assert world.current_tick == 0
    assert world.seed == 99
    world.destroy()


def test_step_after_reset():
    """Step works correctly after reset."""
    world, _ = make_const_world(value=7.0)
    world.step()
    world.reset(123)
    world.step()
    buf = np.zeros(10, dtype=np.float32)
    world.read_field(0, buf)
    np.testing.assert_array_equal(buf, 7.0)
    world.destroy()


def test_context_manager():
    """World works as context manager."""
    world, _ = make_const_world()
    with world:
        world.step()
        assert world.current_tick == 1


def test_double_destroy():
    """Double destroy doesn't crash."""
    world, _ = make_const_world()
    world.destroy()
    world.destroy()  # Should be a no-op


def test_use_after_destroy_raises():
    """Using a destroyed world raises RuntimeError."""
    world, _ = make_const_world()
    world.destroy()
    with pytest.raises(RuntimeError, match="already destroyed"):
        world.step()


def test_step_with_commands():
    """Step accepts SetField commands."""
    world, _ = make_const_world(value=0.0)
    world.step()  # First step to populate

    cmd = Command.set_field(field_id=0, coord=[0], value=99.0)
    receipts, _ = world.step([cmd])
    assert len(receipts) >= 1


def test_properties():
    """current_tick, seed, is_tick_disabled."""
    world, _ = make_const_world(seed=77)
    assert world.current_tick == 0
    assert world.seed == 77
    assert world.is_tick_disabled is False
    world.destroy()


def test_metrics_populated():
    """Step returns populated metrics."""
    world, _ = make_const_world()
    _, metrics = world.step()
    assert metrics.total_us >= 0
    assert metrics.memory_bytes > 0
    assert len(metrics.propagator_us) == 1
    assert metrics.queue_full_rejections >= 0
    assert metrics.tick_disabled_rejections >= 0
    assert metrics.rollback_events >= 0
    assert metrics.tick_disabled_transitions >= 0
    assert metrics.worker_stall_events >= 0
    assert metrics.ring_not_available_events >= 0
    name, us = metrics.propagator_us[0]
    assert name == "const"
    world.destroy()


def test_metrics_to_dict():
    """StepMetrics.to_dict() returns correct structure."""
    world, _ = make_const_world()
    _, metrics = world.step()
    d = metrics.to_dict()
    assert "total_us" in d
    assert "memory_bytes" in d
    assert "propagator_us" in d
    assert "queue_full_rejections" in d
    assert "tick_disabled_rejections" in d
    assert "rollback_events" in d
    assert "tick_disabled_transitions" in d
    assert "worker_stall_events" in d
    assert "ring_not_available_events" in d
    world.destroy()
