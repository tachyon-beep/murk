"""Type stubs for the murk._murk native extension (PyO3)."""

from __future__ import annotations

from collections.abc import Callable
from typing import Any

import numpy as np
import numpy.typing as npt

# ---------------------------------------------------------------------------
# Enums
# ---------------------------------------------------------------------------

class SpaceType:
    Line1D: SpaceType
    Ring1D: SpaceType
    Square4: SpaceType
    Square8: SpaceType
    Hex2D: SpaceType
    ProductSpace: SpaceType
    Fcc12: SpaceType
    def __int__(self) -> int: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class FieldType:
    Scalar: FieldType
    Vector: FieldType
    Categorical: FieldType
    def __int__(self) -> int: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class FieldMutability:
    Static: FieldMutability
    PerTick: FieldMutability
    Sparse: FieldMutability
    def __int__(self) -> int: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class BoundaryBehavior:
    Clamp: BoundaryBehavior
    Reflect: BoundaryBehavior
    Absorb: BoundaryBehavior
    Wrap: BoundaryBehavior
    def __int__(self) -> int: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class EdgeBehavior:
    Absorb: EdgeBehavior
    Clamp: EdgeBehavior
    Wrap: EdgeBehavior
    def __int__(self) -> int: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class RegionType:
    All: RegionType
    AgentDisk: RegionType
    AgentRect: RegionType
    @property
    def value(self) -> int: ...
    def __int__(self) -> int: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class TransformType:
    Identity: TransformType
    Normalize: TransformType
    @property
    def value(self) -> int: ...
    def __int__(self) -> int: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class PoolKernel:
    NoPool: PoolKernel
    Mean: PoolKernel
    Max: PoolKernel
    Min: PoolKernel
    Sum: PoolKernel
    @property
    def value(self) -> int: ...
    def __int__(self) -> int: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class DType:
    F32: DType
    @property
    def value(self) -> int: ...
    def __int__(self) -> int: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class WriteMode:
    Full: WriteMode
    Incremental: WriteMode
    def __int__(self) -> int: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

class CommandType:
    SetParameter: CommandType
    SetField: CommandType
    def __int__(self) -> int: ...
    def __eq__(self, other: object) -> bool: ...
    def __hash__(self) -> int: ...

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

class Config:
    def __init__(self) -> None: ...
    def set_space(self, space_type: SpaceType, params: list[float]) -> None:
        """Set spatial topology (low-level). Prefer typed set_space_* methods."""
        ...
    def set_space_line1d(self, length: int, edge: EdgeBehavior) -> None:
        """Set space to Line1D."""
        ...
    def set_space_ring1d(self, length: int) -> None:
        """Set space to Ring1D (periodic 1D)."""
        ...
    def set_space_square4(self, width: int, height: int, edge: EdgeBehavior) -> None:
        """Set space to Square4 (2D grid, 4-connected)."""
        ...
    def set_space_square8(self, width: int, height: int, edge: EdgeBehavior) -> None:
        """Set space to Square8 (2D grid, 8-connected)."""
        ...
    def set_space_hex2d(self, cols: int, rows: int) -> None:
        """Set space to Hex2D (hexagonal lattice, 6-connected)."""
        ...
    def set_space_fcc12(self, width: int, height: int, depth: int, edge: EdgeBehavior) -> None:
        """Set space to Fcc12 (3D FCC lattice, 12-connected)."""
        ...
    def add_field(
        self,
        name: str,
        field_type: FieldType = ...,
        mutability: FieldMutability = ...,
        dims: int = ...,
        boundary: BoundaryBehavior = ...,
    ) -> None: ...
    def set_dt(self, dt: float) -> None: ...
    def set_seed(self, seed: int) -> None: ...
    def __enter__(self) -> Config: ...
    def __exit__(self, _exc_type: Any = ..., _exc_val: Any = ..., _exc_tb: Any = ...) -> None: ...

# ---------------------------------------------------------------------------
# Command / Receipt
# ---------------------------------------------------------------------------

class Command:
    @staticmethod
    def set_field(
        field_id: int,
        coord: list[int],
        value: float,
        expires_after_tick: int = ...,
    ) -> Command: ...
    @staticmethod
    def set_parameter(
        param_key: int,
        value: float,
        expires_after_tick: int = ...,
    ) -> Command: ...

class Receipt:
    @property
    def accepted(self) -> bool: ...
    @property
    def applied_tick_id(self) -> int: ...
    @property
    def reason_code(self) -> int: ...
    @property
    def command_index(self) -> int: ...
    def __repr__(self) -> str: ...

# ---------------------------------------------------------------------------
# World
# ---------------------------------------------------------------------------

class World:
    def __init__(self, config: Config) -> None: ...
    def step(self, commands: list[Command] | None = ...) -> tuple[list[Receipt], StepMetrics]: ...
    def reset(self, seed: int) -> None: ...
    def read_field(self, field_id: int, output: npt.NDArray[np.float32]) -> None: ...
    @property
    def current_tick(self) -> int: ...
    @property
    def seed(self) -> int: ...
    @property
    def is_tick_disabled(self) -> bool: ...
    def destroy(self) -> None: ...
    def __enter__(self) -> World: ...
    def __exit__(self, _exc_type: Any = ..., _exc_val: Any = ..., _exc_tb: Any = ...) -> None: ...

# ---------------------------------------------------------------------------
# ObsEntry / ObsPlan
# ---------------------------------------------------------------------------

class ObsEntry:
    def __init__(
        self,
        field_id: int,
        region_type: RegionType = ...,
        transform_type: TransformType = ...,
        normalize_min: float = ...,
        normalize_max: float = ...,
        dtype: DType = ...,
        region_params: list[int] | None = ...,
        pool_kernel: PoolKernel = ...,
        pool_kernel_size: int = ...,
        pool_stride: int = ...,
    ) -> None: ...

class ObsPlan:
    def __init__(self, world: World, entries: list[ObsEntry]) -> None: ...
    def execute(
        self,
        world: World,
        output: npt.NDArray[np.float32],
        mask: npt.NDArray[np.uint8],
    ) -> tuple[int, int]: ...
    def execute_agents(
        self,
        world: World,
        agent_centers: npt.NDArray[np.int32],
        output: npt.NDArray[np.float32],
        mask: npt.NDArray[np.uint8],
    ) -> list[tuple[int, int]]: ...
    @property
    def output_len(self) -> int: ...
    @property
    def mask_len(self) -> int: ...
    def destroy(self) -> None: ...
    def __enter__(self) -> ObsPlan: ...
    def __exit__(self, _exc_type: Any = ..., _exc_val: Any = ..., _exc_tb: Any = ...) -> None: ...

# ---------------------------------------------------------------------------
# BatchedWorld
# ---------------------------------------------------------------------------

class BatchedWorld:
    def __init__(
        self,
        configs: list[Config],
        obs_entries: list[ObsEntry] | None = ...,
    ) -> None: ...
    def step_and_observe(
        self,
        commands_per_world: list[list[Command]],
        obs_output: npt.NDArray[np.float32],
        obs_mask: npt.NDArray[np.uint8],
    ) -> list[int]: ...
    def observe_all(
        self,
        obs_output: npt.NDArray[np.float32],
        obs_mask: npt.NDArray[np.uint8],
    ) -> None: ...
    def reset_world(self, index: int, seed: int) -> None: ...
    def reset_all(self, seeds: list[int]) -> None: ...
    @property
    def num_worlds(self) -> int: ...
    @property
    def obs_output_len(self) -> int: ...
    @property
    def obs_mask_len(self) -> int: ...
    def destroy(self) -> None: ...
    def __enter__(self) -> BatchedWorld: ...
    def __exit__(self, _exc_type: Any = ..., _exc_val: Any = ..., _exc_tb: Any = ...) -> None: ...

# ---------------------------------------------------------------------------
# StepMetrics
# ---------------------------------------------------------------------------

class StepMetrics:
    @property
    def total_us(self) -> int: ...
    @property
    def command_processing_us(self) -> int: ...
    @property
    def snapshot_publish_us(self) -> int: ...
    @property
    def memory_bytes(self) -> int: ...
    @property
    def propagator_us(self) -> list[tuple[str, int]]: ...
    def to_dict(self) -> dict[str, Any]: ...
    def __repr__(self) -> str: ...

# ---------------------------------------------------------------------------
# Propagator
# ---------------------------------------------------------------------------

class PropagatorDef:
    def __init__(
        self,
        name: str,
        step_fn: Callable[..., None],
        reads: list[int] = ...,
        reads_previous: list[int] = ...,
        writes: list[tuple[int, WriteMode]] = ...,
    ) -> None: ...
    def register(self, config: Config) -> None: ...

def add_propagator(
    config: Config,
    name: str,
    step_fn: Callable[..., None],
    reads: list[int] = ...,
    reads_previous: list[int] = ...,
    writes: list[tuple[int, WriteMode]] = ...,
) -> None: ...

# ---------------------------------------------------------------------------
# Library Propagators
# ---------------------------------------------------------------------------

class ScalarDiffusion:
    """Native diffusion propagator with optional gradient output and clamping."""
    def __init__(
        self,
        input_field: int,
        output_field: int,
        coefficient: float = ...,
        decay: float = ...,
        sources: list[tuple[int, float]] = ...,
        clamp_min: float | None = ...,
        clamp_max: float | None = ...,
        gradient_field: int | None = ...,
        max_degree: int = ...,
    ) -> None: ...
    def register(self, config: Config) -> None: ...
    def __repr__(self) -> str: ...

class GradientCompute:
    """Computes discrete spatial gradient from a scalar field."""
    def __init__(self, input_field: int, output_field: int) -> None: ...
    def register(self, config: Config) -> None: ...
    def __repr__(self) -> str: ...

class IdentityCopy:
    """Copies a field's previous-generation values into the current generation."""
    def __init__(self, field: int) -> None: ...
    def register(self, config: Config) -> None: ...
    def __repr__(self) -> str: ...

class FlowField:
    """Computes a flow (velocity) field from a potential field's gradient."""
    def __init__(
        self,
        potential_field: int,
        flow_field: int,
        normalize: bool = ...,
    ) -> None: ...
    def register(self, config: Config) -> None: ...
    def __repr__(self) -> str: ...

class AgentEmission:
    """Emits values into a field at agent locations."""
    def __init__(
        self,
        presence_field: int,
        emission_field: int,
        intensity: float = ...,
        additive: bool = ...,
    ) -> None: ...
    def register(self, config: Config) -> None: ...
    def __repr__(self) -> str: ...

class ResourceField:
    """Consumable/regrowing resource field driven by agent presence."""
    def __init__(
        self,
        field: int,
        presence_field: int,
        consumption_rate: float = ...,
        regrowth_rate: float = ...,
        capacity: float = ...,
        logistic: bool = ...,
    ) -> None: ...
    def register(self, config: Config) -> None: ...
    def __repr__(self) -> str: ...

class MorphologicalOp:
    """Binary morphological dilation or erosion on a scalar field."""
    def __init__(
        self,
        input_field: int,
        output_field: int,
        dilate: bool = ...,
        radius: int = ...,
        threshold: float = ...,
    ) -> None: ...
    def register(self, config: Config) -> None: ...
    def __repr__(self) -> str: ...

class WavePropagation:
    """Wave equation propagator with displacement and velocity fields."""
    def __init__(
        self,
        displacement_field: int,
        velocity_field: int,
        wave_speed: float = ...,
        damping: float = ...,
    ) -> None: ...
    def register(self, config: Config) -> None: ...
    def __repr__(self) -> str: ...

class NoiseInjection:
    """Injects stochastic noise into a field each tick."""
    def __init__(
        self,
        field: int,
        noise_type: str = ...,
        scale: float = ...,
        seed_offset: int = ...,
    ) -> None: ...
    def register(self, config: Config) -> None: ...
    def __repr__(self) -> str: ...
