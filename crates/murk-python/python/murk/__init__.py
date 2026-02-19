"""Murk: Python bindings for the Murk simulation framework.

This package provides:
- Low-level bindings wrapping the C FFI (via the ``_murk`` Rust extension)
- Gymnasium-compatible environment adapters for RL training
"""

from murk._murk import (
    AgentEmission,
    BatchedWorld,
    BoundaryBehavior,
    Command,
    CommandType,
    Config,
    DType,
    EdgeBehavior,
    FieldMutability,
    FieldType,
    FlowField,
    GradientCompute,
    IdentityCopy,
    MorphologicalOp,
    NoiseInjection,
    ObsEntry,
    ObsPlan,
    PoolKernel,
    PropagatorDef,
    Receipt,
    RegionType,
    ResourceField,
    ScalarDiffusion,
    SpaceType,
    StepMetrics,
    TransformType,
    WavePropagation,
    World,
    WriteMode,
    add_propagator,
)

from murk.batched_vec_env import BatchedVecEnv
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
    "BatchedWorld",
    "Config",
    "Command",
    "Receipt",
    "World",
    "ObsEntry",
    "ObsPlan",
    "StepMetrics",
    "PropagatorDef",
    # Library propagators
    "AgentEmission",
    "FlowField",
    "GradientCompute",
    "IdentityCopy",
    "MorphologicalOp",
    "NoiseInjection",
    "ResourceField",
    "ScalarDiffusion",
    "WavePropagation",
    # Functions
    "add_propagator",
    # Gymnasium
    "BatchedVecEnv",
    "MurkEnv",
    "MurkVecEnv",
]
