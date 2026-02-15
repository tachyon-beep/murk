# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2026-02-15

Initial public release of Murk: a tick-based world simulation engine
for reinforcement learning and real-time applications.

### Added

#### Core Framework
- `murk-core` crate: field definitions (`Scalar`, `Vector`, `Categorical`),
  mutability classes (`Static`, `PerTick`, `Sparse`), command types, receipts,
  `SnapshotAccess` and `FieldReader`/`FieldWriter` traits, `FieldSet` bitset
  with property tests.
- `murk-arena` crate: double-buffered ping-pong arena allocator with
  generational addressing, copy-on-write sparse slab, shared static arena,
  and `Snapshot`/`OwnedSnapshot` types.

#### Spatial Backends
- `murk-space` crate with `Space` trait and seven built-in backends:
  - `Line1D` — 1D line lattice with configurable edge behavior
  - `Ring1D` — 1D periodic ring
  - `Square4` — 2D grid, 4-connected (N/S/E/W)
  - `Square8` — 2D grid, 8-connected (+ diagonals)
  - `Hex2D` — hexagonal lattice, 6-connected (pointy-top axial coordinates)
  - `Fcc12` — 3D face-centred cubic lattice, 12-connected (isotropic)
  - `ProductSpace` — Cartesian product of arbitrary spaces with configurable
    distance metric (L1, L-infinity, weighted)
- Edge behaviors: `Absorb`, `Clamp`, `Wrap` (periodic/torus)
- `RegionSpec` with `All`, `Disk`, `Rect`, `Neighbours`, and `Coords` variants
- `Space::canonical_rank()` for O(1) coordinate-to-index mapping

#### Propagator Pipeline
- `murk-propagator` crate: `Propagator` trait with `reads`/`reads_previous`/
  `writes` declarations, `WriteMode::Full` and `WriteMode::Incremental`,
  CFL stability checking, `StepContext` with split-borrow field access.
- Pipeline validation: write-conflict detection, undefined-field errors,
  `dt` vs `max_dt` constraint checking.
- `ReadResolutionPlan` for zero-overhead per-tick overlay routing.
- `FullWriteGuard` for debug-mode cell coverage tracking.

#### Observation System
- `murk-obs` crate: `ObsSpec` → `ObsPlan` → flat `f32` tensor pipeline.
- Region types: `All`, `AgentDisk`, `AgentRect` for foveated local perception.
- Transforms: `Identity`, `Normalize(min, max)`.
- Pooling: `Mean`, `Max`, `Min`, `Sum` kernels with configurable stride.
- `execute_agents()` for batched multi-agent observation extraction.
- Generation binding with `age_ticks` staleness reporting.
- `ObsPlanCache` with `SpaceInstanceId`-based invalidation.

#### Engine
- `murk-engine` crate with `TickEngine`, `IngressQueue`, and two runtime modes:
  - `LockstepWorld` — synchronous `step_sync(&mut self)` with borrow-checker
    enforced single-threaded access
  - `RealtimeAsyncWorld` — background tick thread, bounded command channels,
    egress worker pool, epoch-based snapshot reclamation, adaptive backoff,
    and graceful shutdown FSM
- `StepMetrics` with per-propagator timing and memory reporting.
- Command ordering: `priority_class` → `source_id` → `arrival_seq`.

#### Replay
- `murk-replay` crate: binary replay format (v2) with per-tick snapshot
  hashing, determinism verification, and divergence reports.

#### FFI
- `murk-ffi` crate: 28+ `extern "C"` functions with slot+generation handle
  tables, safe double-destroy, null-pointer validation, and versioned ABI.

#### Python Bindings
- `murk-python` crate: PyO3/maturin native extension exposing `Config`,
  `World`, `Command`, `ObsPlan`, `ObsEntry`, `PropagatorDef`, `StepMetrics`,
  and all enum types.
- `MurkEnv` Gymnasium adapter with `step`/`reset`/`close` and override hooks
  (`_action_to_commands`, `_compute_reward`, `_check_terminated`, `_check_truncated`).
- `MurkVecEnv` vectorized environment with auto-reset.
- GIL release on all blocking FFI calls.
- Python type stubs (`.pyi`) with PEP 561 `py.typed` marker.
- Typed enum API: `EdgeBehavior`, `WriteMode`, `RegionType`, `TransformType`,
  `PoolKernel`, `DType`, `FieldType`, `FieldMutability`, `BoundaryBehavior`.
- Typed `set_space_*` methods: `set_space_line1d()`, `set_space_ring1d()`,
  `set_space_square4()`, `set_space_square8()`, `set_space_hex2d()`,
  `set_space_fcc12()`.

#### Reference Propagators
- `murk-propagators` crate: diffusion, agent movement, and reward propagators
  with configurable parameters.
- Benchmark profiles for lockstep RL evaluation.

#### Documentation
- Concepts guide (`docs/CONCEPTS.md`) covering spaces, fields, propagators,
  commands, observations, runtime modes, and arena memory.
- Architecture document (`docs/ARCHITECTURE.md`).
- Replay format specification (`docs/replay-format.md`).
- Error reference (`docs/error-reference.md`).

#### Examples
- `heat_seeker` — PPO agent learns to navigate a heat gradient on a 16x16
  Square4 grid with Python propagator.
- `crystal_nav` — FCC12 crystal lattice navigation demo.
- `hex_pursuit` — Multi-agent predator-prey on Hex2D with AgentDisk foveation
  and `execute_agents()` batched observation.
- `quickstart.rs` — Compilable Rust example: diffusion propagator, command
  injection, snapshot reading, and world reset.
- `lockstep_rl.rs` — End-to-end Rust RL loop with reference propagators.

#### Testing
- 640+ tests across the workspace (unit, integration, property, stress).
- Miri verification for `murk-arena` memory safety.
- CI: check, test, clippy, rustfmt, and Miri on every push and PR.

[unreleased]: https://github.com/tachyon-beep/murk/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tachyon-beep/murk/releases/tag/v0.1.0
