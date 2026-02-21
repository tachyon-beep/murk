# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.8] - 2026-02-22

FFI panic safety hardening and observability improvements.

### Added

- `ffi_guard!` macro: all 41 `extern "C"` functions wrapped in `catch_unwind`
- `MurkStatus::Panicked` (-128) error code for caught panics
- `murk_last_panic_message()` FFI function for panic diagnostics
- `Propagator::max_dt()` now receives `&dyn Space` for topology-aware CFL bounds
- `sparse_reuse_hits` and `sparse_reuse_misses` metrics in StepMetrics/FFI/Python
- Formal semantic documentation for ProductSpace (CR-1)

### Changed

- ABI version bumped from v2.0 to v2.1
- `MurkStepMetrics` layout: 48 → 56 bytes (added reuse counters)
- `ScalarDiffusion` CFL bound derived from space topology (no longer defaults to 12-neighbour worst-case)
- `validate_pipeline()` signature extended with `space: &dyn Space` parameter

## [0.1.7] - 2026-02-21

Major stabilisation release: 90+ bug fixes across all crates, 6 new library
propagators, cbindgen C header generation, and sparse reclamation observability.

### Added

#### BatchedEngine
- `BatchedEngine` for high-throughput parallel world stepping with a single GIL release
- `BatchedVecEnv` SB3-compatible Python interface for batched training
- `batched_heat_seeker` example: BatchedVecEnv migration with vectorized state
- `batched_cookbook` example: low-level BatchedWorld API recipes
- `batched_benchmark` example: performance comparison of BatchedVecEnv vs MurkVecEnv
- Batched topology validation: reject incompatible space topologies and validate obs before stepping

#### Library Propagators
- `ScalarDiffusion` with builder pattern and configurable parameters
- `GradientCompute` standalone propagator with buffer bounds guarding
- `IdentityCopy` propagator for field mirroring
- `FlowField` propagator for vector field advection
- `AgentEmission` propagator for agent-driven field writes
- `ResourceField` propagator for resource dynamics
- `MorphologicalOp` propagator for spatial erosion/dilation
- `WavePropagation` propagator for wave equation simulation
- `NoiseInjection` propagator with `rand` dependency
- PyO3 bindings for all new propagators
- Integration tests through `LockstepWorld` for all propagators
- Parity test between old and new diffusion implementations

#### FFI & Observability
- Auto-generated `include/murk.h` C header via cbindgen (42 functions, 8 structs, 8 enums)
- Sparse reclamation metrics: `sparse_retired_ranges` and `sparse_pending_retired`
  exposed through `StepMetrics` → `MurkStepMetrics` → Python `StepMetrics`
- `UnsupportedCommand` error variant for rejected command types

#### Documentation
- Modeling concepts section in README with 20+ domain-specific simulation patterns
- Explicit determinism contract and authoritative surface area documentation
- BatchedEngine documentation across all guides (CONCEPTS.md, ARCHITECTURE.md, etc.)
- Comprehensive documentation review and fixes for 0.1.7
- Bug hunt script for automated issue discovery

### Changed

- ABI version bumped from v1.0 to v2.0 (`MurkStepMetrics` layout: 40 → 48 bytes)
- `MurkStepMetrics` `#[repr(C)]` struct extended with sparse reclamation fields
- Examples migrated from hardcoded propagators to library propagators
  (`heat_seeker`, `crystal_nav`, `hex_pursuit`)
- Benchmark profiles switched to library propagators
- Hardcoded field constants in propagators deprecated
- Retired range tuple replaced with named `RetiredRange` struct in arena internals

### Fixed

#### Critical
- Receipt buffer out-of-bounds panic in Python `step()` (BUG-001)
- Sparse segment memory leak from unbounded CoW allocations
- FFI metrics race condition in concurrent world stepping

#### Arena (murk-arena)
- Per-tick allocation undercount in memory reporting
- Scratch region reuse across ticks
- Segment slice beyond cursor panic
- Missing segment size validation
- Publish-without-begin-tick state guard
- Static arena duplicate field ID acceptance
- Descriptor clone-per-tick overhead
- Cell count components overflow
- Generation counter overflow handling
- Sparse CoW generation rollover

#### Engine (murk-engine)
- SetField command visibility across tick boundary
- Ring buffer spurious `None` on latest snapshot
- Shutdown blocks on slow tick in RealtimeAsync mode
- Backoff config not validated at construction
- Adaptive backoff output unused
- Egress epoch/tick mismatch
- Observe buffer bounds check
- Reset returns wrong error variant
- Tick accepts non-SetField commands silently
- Cell count u32 truncation

#### FFI (murk-ffi)
- Mutex poisoning panics across FFI boundary (3 fixes)
- Obs conversion duplicated across modules
- ObsPlan lock ordering inconsistency
- Trampoline null pointer dereference
- Config not consumed on null output pointer
- Inconsistent mutex poisoning handling
- `usize` in `#[repr(C)]` struct
- Handle accessor ambiguity (returns 0 for both success and invalid handle)
- Generation wraparound safety

#### Python (murk-python)
- CString from raw pointer potential UB
- Trampoline panic across FFI boundary
- Missing type stubs for library propagators
- Close skips ObsPlan destroy
- Batched VecEnv missing spaces property
- Command docstring expiry default mismatch
- Error hints reference unexposed config
- Reset-all no-seeds validation
- VecEnv false SB3 compatibility claim
- Auto-reset hook, priority derivation, path crash

#### Propagators
- Diffusion CFL uses hardcoded degree instead of space connectivity
- Scratch bytes/slots mismatch in capacity calculation
- Agent presence issues with tick-0 actions
- NaN/infinity validation gaps
- Resolve-axis duplicated computation
- Reward stale heat gradient dependency
- Pipeline NaN max_dt bypass
- Performance hotspots in inner loops

#### Observation (murk-obs)
- FlatBuffer silent u16 truncation
- FlatBuffer signed/unsigned cast corruption
- Per-agent scratch allocation overflow
- Normalize inverted range
- Canonical rank negative coordinate handling
- Pool NaN produces infinity
- Plan fast-path unchecked index panic
- Geometry `is_interior` missing dimension check

#### Replay (murk-replay)
- Unbounded allocation from wire data
- Compare sentinel zero divergence false positive
- Hash of empty snapshot returns nonzero
- Writer no flush on drop
- Write path u32 truncation
- Expires/arrival_seq not serialized

#### Space (murk-space)
- Hex2D disk overflow on large radii
- FCC12 parity overflow
- Product space weighted metric truncation
- Compliance ordering for membership checks
- `is_multiple_of` MSRV compatibility

#### Cross-cutting
- Workspace unused `indexmap` dependencies
- Missing `#[must_use]` attributes
- Error types missing `PartialEq`
- Field type zero-dims constructible
- Umbrella snapshot not importable

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
- `layered_hex` — ProductSpace (Hex2D × Line1D) multi-floor navigation demo.
- `quickstart.rs` — Compilable Rust example: diffusion propagator, command
  injection, snapshot reading, and world reset.
- `realtime_async.rs` — RealtimeAsyncWorld: background ticking, observe,
  and graceful shutdown.
- `replay.rs` — Deterministic replay: record, verify, and prove determinism.
- `lockstep_rl.rs` — End-to-end Rust RL loop with reference propagators.

#### Testing
- 660+ tests across the workspace (unit, integration, property, stress).
- Miri verification for `murk-arena` memory safety.
- CI: check, test, clippy, rustfmt, and Miri on every push and PR.

[unreleased]: https://github.com/tachyon-beep/murk/compare/v0.1.7...HEAD
[0.1.7]: https://github.com/tachyon-beep/murk/compare/v0.1.0...v0.1.7
[0.1.0]: https://github.com/tachyon-beep/murk/releases/tag/v0.1.0
