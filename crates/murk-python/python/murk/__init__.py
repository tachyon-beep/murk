"""Murk: Python bindings for the Murk simulation framework.

This package provides:
- Low-level bindings wrapping the C FFI (via the ``_murk`` Rust extension)
- Gymnasium-compatible environment adapters for RL training
"""

from murk._murk import (
    BoundaryBehavior,
    Command,
    CommandType,
    Config,
    DType,
    EdgeBehavior,
    FieldMutability,
    FieldType,
    ObsEntry,
    ObsPlan,
    PoolKernel,
    PropagatorDef,
    Receipt,
    RegionType,
    SpaceType,
    StepMetrics,
    TransformType,
    World,
    WriteMode,
    add_propagator,
)

from murk.env import MurkEnv
from murk.vec_env import MurkVecEnv

__all__ = [
    # Enums
    "SpaceType",
    "FieldType",
    "FieldMutability",
    "BoundaryBehavior",
    "EdgeBehavior",
    "WriteMode",
    "CommandType",
    "RegionType",
    "TransformType",
    "PoolKernel",
    "DType",
    # Core classes
    "Config",
    "Command",
    "Receipt",
    "World",
    "ObsEntry",
    "ObsPlan",
    "StepMetrics",
    "PropagatorDef",
    # Functions
    "add_propagator",
    # Gymnasium
    "MurkEnv",
    "MurkVecEnv",
]
