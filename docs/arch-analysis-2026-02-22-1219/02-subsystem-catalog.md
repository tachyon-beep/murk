# Subsystem Catalog

This catalog is intentionally scoped to high-signal subsystems that drive
fitness and future work: engine runtime, arena allocator, spaces/regions,
observations, replay/determinism, and bindings.

## 0) `murk` (Facade / Public Rust API)

- Location: `crates/murk`
- Responsibility: user-facing facade crate that re-exports the core sub-crates under a single dependency.
- Key components:
  - `crates/murk/src/lib.rs`: re-exports + prelude.
  - `crates/murk/README.md`: minimal “how to use” entrypoint.
- Dependencies: depends on most workspace crates; primary entrypoint for external Rust consumers.
- Confidence: High.

## 1) `murk-core` (Core Types)

- Location: `crates/murk-core`
- Responsibility: foundational types shared across the workspace (IDs, field defs, commands/receipts, error types, core traits).
- Key interfaces (doc-level): `FieldDef`, `FieldId`, `Command`, `Receipt`, `SnapshotAccess`.
- Dependencies: leaf-level crate by design (depended on by most others).
- Confidence: High (documented in `crates/murk-core/README.md` + architecture docs).

## 2) `murk-arena` (Generational Allocation)

- Location: `crates/murk-arena`
- Responsibility: ping-pong generation staging/publish, static/per-tick/sparse storage, snapshot read model.
- Key interfaces (doc-level): `PingPongArena`, `Snapshot`, `ArenaConfig`.
- Dependencies: built atop `murk-core`; consumed by engine/obs/replay/propagators.
- Key components:
  - `crates/murk-arena/src/pingpong.rs`: `PingPongArena` orchestrates staging/publish and generation swaps.
  - `crates/murk-arena/src/segment.rs`: `SegmentList` / segment allocation budget and per-generation buffers.
  - `crates/murk-arena/src/sparse.rs`: `SparseSlab` copy-on-write behavior and retired-range reuse.
  - `crates/murk-arena/src/descriptor.rs`, `crates/murk-arena/src/handle.rs`: `FieldDescriptor` / `FieldHandle` mapping.
  - `crates/murk-arena/src/read.rs`, `crates/murk-arena/src/write.rs`: `Snapshot`/`WriteArena` access boundary.
- Confidence: Medium (subsystem boundaries are clear; exact perf behavior depends on internal allocation/reuse patterns).

## 3) `murk-space` (Topology + Regions)

- Location: `crates/murk-space`
- Responsibility: `Space` trait, lattice backends, distance + neighbors, canonical ordering, and region planning for observations.
- Key interfaces (doc-level): `Space`, `RegionSpec`, backends like `Square4`, `Hex2D`, `Fcc12`, `ProductSpace`.
- Dependencies: built atop `murk-core`; consumed by engine/obs/propagators.
- Key components:
  - `crates/murk-space/src/space.rs`: `Space` trait (topology + canonical ordering + region planning hooks).
  - `crates/murk-space/src/region.rs`: `RegionSpec` / `RegionPlan` compile + iteration contract.
  - `crates/murk-space/src/product.rs`: `ProductSpace` composition + metrics.
  - `crates/murk-space/src/compliance.rs`: compliance tests for backend invariants.
- Confidence: Medium.

## 4) `murk-propagator` + `murk-propagators` (Simulation Logic Extension Point)

- Locations: `crates/murk-propagator`, `crates/murk-propagators`
- Responsibility: stateless `Propagator` trait, pipeline validation, `StepContext`; plus a standard-library set of reference propagators.
- Key interfaces (doc-level): `Propagator`, `StepContext`, `WriteMode`, pipeline validation.
- Dependencies: consumes `murk-core` + `murk-space` + `murk-arena`; used by `murk-engine`.
- Confidence: Medium.

## 5) `murk-obs` (Observation Extraction)

- Location: `crates/murk-obs`
- Responsibility: `ObsSpec -> ObsPlan` compilation and snapshot extraction into fixed-layout tensors + masks, including pooling/foveation/batching.
- Key interfaces (doc-level): `ObsSpec`, `ObsEntry`, `ObsPlan`.
- Dependencies: consumes `murk-core` + `murk-space` + `murk-arena`.
- Key components:
  - `crates/murk-obs/src/spec.rs`: `ObsSpec` / `ObsEntry` (regions/transforms/pooling/dtype).
  - `crates/murk-obs/src/plan.rs`: compilation + execution hot paths (`execute`, `execute_agents`, `execute_batch`).
  - `crates/murk-obs/src/cache.rs`: `ObsPlanCache` keyed on space identity.
  - `crates/murk-obs/src/geometry.rs`: geometry helpers for fast interior paths.
  - `crates/murk-obs/src/pool.rs`: pooling kernels.
  - `crates/murk-obs/src/metadata.rs`: `ObsMetadata` emitted from execution.
- Confidence: Medium.

## 6) `murk-engine` (Runtimes + Tick Pipeline)

- Location: `crates/murk-engine`
- Responsibility: ingress/egress/tick separation, two runtime modes (`LockstepWorld`, `RealtimeAsyncWorld`), and high-throughput `BatchedEngine`.
- Key interfaces (doc-level): `LockstepWorld`, `RealtimeAsyncWorld`, `BatchedEngine`, `WorldConfig`.
- Dependencies: sits “above” core subsystems; used by FFI/Python.
- Key components:
  - `crates/murk-engine/src/tick.rs`: `TickEngine` core loop + rollback/disable behavior.
  - `crates/murk-engine/src/ingress.rs`: `IngressQueue` (bounded buffering + deterministic ordering).
  - `crates/murk-engine/src/overlay.rs`: overlay caches and read resolution routing.
  - `crates/murk-engine/src/lockstep.rs`: `LockstepWorld` synchronous API surface.
  - `crates/murk-engine/src/realtime.rs`, `crates/murk-engine/src/tick_thread.rs`: realtime tick thread + shutdown/reset.
  - `crates/murk-engine/src/egress.rs`: egress pool / observation tasks.
  - `crates/murk-engine/src/ring.rs`, `crates/murk-engine/src/epoch.rs`: snapshot ring + epoch pinning.
  - `crates/murk-engine/src/batched.rs`: `BatchedEngine` stepping + batched observation path.
- Confidence: Medium.

## 7) `murk-replay` (Determinism Tooling)

- Location: `crates/murk-replay`
- Responsibility: recording + reading replay frames and verifying determinism via per-tick snapshot hashing.
- Key interfaces (doc-level): `ReplayWriter`, `ReplayReader`, replay wire format (current version in `murk_replay::FORMAT_VERSION`, presently v3).
- Dependencies: consumes `murk-arena` (snapshots) + `murk-core` types.
- Confidence: Medium.

## 8) `murk-ffi` + `murk-python` (Bindings)

- Locations: `crates/murk-ffi`, `crates/murk-python`
- Responsibility: stable C ABI (handles + versioning + panic safety), and Python package surface (PyO3 extension + Gymnasium adapters + BatchedVecEnv).
- Key interfaces (doc-level): ABI version query, config/world lifecycle, step/observe/reset, error/panic diagnostics.
- Dependencies: `murk-engine` + `murk-obs` + `murk-replay` (via engine) as the “host”.
- Key components:
  - `crates/murk-ffi/src/handle.rs`: slot+generation handle tables.
  - `crates/murk-ffi/src/world.rs`, `crates/murk-ffi/src/batched.rs`: stepping APIs for single/batched worlds.
  - `crates/murk-ffi/src/obs.rs`: ObsPlan execution and buffer validation.
  - `crates/murk-ffi/src/config.rs`: config builder API.
  - `crates/murk-ffi/src/metrics.rs`: step metrics bridging.
  - `crates/murk-ffi/src/lib.rs`: ABI versioning + panic boundary tooling.
  - `crates/murk-python/src/lib.rs`: PyO3 module surface.
  - `crates/murk-python/pyproject.toml`: packaging metadata (including `requires-python >= 3.12`).
  - `crates/murk-python/Cargo.toml`: PyO3 build config (including `abi3-py312`).
  - `crates/murk-python/python/murk/*.py`: Gymnasium adapters (`MurkEnv`, `MurkVecEnv`, `BatchedVecEnv`).
- Confidence: Medium.

## Supporting Crates (Tooling/Test)

- `crates/murk-test-utils`: shared test fixtures/helpers used across crates.
- `crates/murk-bench`: benchmark harnesses and utilities (paired with scheduled benchmark CI).
