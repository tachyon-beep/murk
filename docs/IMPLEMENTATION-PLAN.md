# Murk World Engine — Implementation Plan v1.0

**Status:** Approved by SME team (unanimous consensus)
**Date:** 2026-02-09
**Source:** HLD v3.0.1 (`docs/HLD.md`)
**Team:** Systems Architect, Simulation Engineer, DRL Integration Specialist, Quality Engineer

---

## 0. How to Read This Document

This plan is the authoritative implementation roadmap for Murk World Engine v1. It defines:

- **What** to build (work packages with acceptance criteria)
- **In what order** (dependency DAG and critical path)
- **How to verify** (quality gates per milestone)
- **What to watch for** (risk register with mitigations)

All design decisions were resolved through cross-challenge review with unanimous consensus across 4 domain experts. Traceability to HLD requirements uses `R-XXX-N` references.

---

## 1. Crate Structure (9 Crates)

### 1.1 Workspace Layout

```
murk/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── murk-core/                # Leaf crate: IDs, errors, field defs, traits
│   ├── murk-arena/               # Arena-based generational allocation
│   ├── murk-space/               # Space trait + all backends
│   ├── murk-propagator/          # Propagator trait, StepContext, pipeline validation
│   ├── murk-obs/                 # ObsSpec, ObsPlan, tensor export
│   ├── murk-engine/              # TickEngine, modes, ingress queue
│   ├── murk-replay/              # Replay log format, recording, playback
│   ├── murk-ffi/                 # C ABI: handle-based, stable
│   └── murk-python/              # PyO3 bindings, GIL release
├── crates/murk-test-utils/       # Dev-only: test fixtures, mocks, builders
├── crates/murk-bench/            # Dev-only: Criterion + custom benchmarks
├── examples/
│   ├── hello-world/              # M0 reference: Square4, 3 propagators
│   └── lockstep-rl/              # M1 reference: Gymnasium training loop
└── tests/
    ├── integration/              # Cross-crate integration tests
    ├── determinism/              # Replay-and-compare tests
    └── stress/                   # System stress tests (§23)
```

### 1.2 Crate Dependency DAG

```
murk-core (leaf — zero murk deps)
  ├── murk-arena       (core)
  ├── murk-space       (core)
  ├── murk-propagator  (core, space)        ← NOT arena
  ├── murk-obs         (core, space)         ← NOT arena (Decision N: reads via &dyn SnapshotAccess)
  ├── murk-engine      (core, arena, space, propagator, obs)
  ├── murk-replay      (core)
  ├── murk-ffi         (core, engine, obs)
  └── murk-python      (ffi)
```

**Key design decision:** `murk-propagator` does NOT depend on `murk-arena`. The `FieldReader`/`FieldWriter` traits are defined in `murk-core`, implemented by `murk-arena`, and consumed by `murk-propagator` via `StepContext<R: FieldReader, W: FieldWriter>`. This:

- Decouples user-authored propagators from arena internals
- Enables mock-based propagator testing (no real arena needed)
- Shortens the critical path (propagator work can start before arena is complete)

### 1.3 What Each Crate Contains

| Crate | Contents | Stability |
|-------|----------|-----------|
| **murk-core** | `FieldId`, `SpaceId`, `TickId`, `WorldGenerationId`, `ParameterVersion`, `Command`, `Receipt`, error enums (§9.7 + `SHUTTING_DOWN` + `TICK_DISABLED`), `FieldDef`, `FieldMutability`, `FieldSet`, `FieldReader`/`FieldWriter` traits, `SnapshotAccess` trait (Decision N) | Semi-stable |
| **murk-arena** | `ReadArena` (Send+Sync), `WriteArena` (&mut), `FieldHandle`, segmented arena (64MB segments), double-buffer ping-pong, sparse slab, static gen-0 arena, `ScratchRegion`, implements `FieldReader`/`FieldWriter` | Internal |
| **murk-space** | `Space` trait, `LatticeSpace` (Line1D, Ring1D, Square4, Square8, Hex2D), `ProductSpace`, `VoxelOctreeSpace` wrapper, region queries, dual distance API | Semi-stable |
| **murk-propagator** | `Propagator` trait, `StepContext<R,W>`, `WriteMode`, pipeline validation (DAG, write conflicts, dt, Incremental budget), `PropagatorError` | Semi-stable |
| **murk-obs** | `ObsSpec`, `ObsPlan`, plan compilation, tensor fill, validity masks, plan classes (Simple/Standard), generation binding, `ObsError` | Semi-stable |
| **murk-engine** | `TickEngine`, `LockstepWorld`, `RealtimeAsyncWorld`, `World` trait (GAT), ingress queue, command ordering, TTL evaluation, receipt generation, tick atomicity, snapshot publication, overlay resolution tables | Internal |
| **murk-replay** | Replay log format, `ReplayWriter`, `ReplayReader`, build metadata header | Internal |
| **murk-ffi** | `extern "C"` functions, opaque handles (`MurkWorld`, `MurkSnapshot`, `MurkObsPlan`), `murk_abi_version()`, error code mapping, `murk_lockstep_step_vec()` | **Stable** (versioned) |
| **murk-python** | PyO3 module, `MurkEnv(gymnasium.Env)`, `MurkVecEnv(gymnasium.vector.VectorEnv)`, GIL release, NumPy buffer integration | **Stable** (versioned) |

### 1.4 Determinism Enforcement

- `#![forbid(unsafe_code)]` in all crates except murk-ffi and murk-arena
- `#![deny(unsafe_code)]` in murk-arena with per-function `#[allow]` in bounded `raw.rs` module (≤5 functions), mandatory `// SAFETY:` comment, Miri coverage (Decision B)
- `#![deny(unsafe_code)]` in murk-ffi with per-function `#[allow]` + mandatory `// SAFETY:` comment
- `clippy::disallowed_types` banning `HashMap`/`HashSet` in all deterministic-path crates (core, arena, space, propagator, obs, engine, replay)
- `IndexMap`/`IndexSet` as default replacement; `BTreeMap` where sorted order is semantically required
- `HashMap` freely allowed in murk-ffi and murk-python only
- `#![deny(missing_docs)]` in all public crates
- `#![deny(rustdoc::broken_intra_doc_links)]` workspace-wide

---

## 2. Work Packages

### WP-0: Test Infrastructure and Workspace Skeleton
- **Description:** Cargo workspace with all 9 crate stubs, CI pipeline, test framework, clippy configuration.
- **Complexity:** S
- **Hard deps:** None (first WP)
- **Deliverables:** Workspace `Cargo.toml`, crate stubs with `#![forbid(unsafe_code)]` (except murk-arena: `#![deny(unsafe_code)]`) and `#![deny(missing_docs)]`, CI pipeline (`cargo check` + `cargo test` + `cargo clippy` + `cargo miri` for murk-arena), `murk-test-utils` with `TestWorldBuilder` scaffold, `MockFieldReader`/`MockFieldWriter`, and `MockSnapshot` (implements `SnapshotAccess` with `Vec<f32>` backing, Decision N), proptest configured, Criterion harness configured, `clippy.toml` per-crate with `disallowed_types`, PR checklist template.
- **Milestone:** Pre-M0

### WP-1: Core Types and Error Model
- **Description:** All shared types, IDs, error enums, field definitions, and FieldReader/FieldWriter traits.
- **Complexity:** S
- **Hard deps:** WP-0
- **HLD refs:** R-ARCH-1, R-ARCH-2, §2 (glossary), §9 (error codes), §13 (field model)
- **Deliverables:** `murk-core` crate with: `FieldId`, `SpaceId`, `TickId`, `WorldGenerationId`, `ParameterVersion`, `Command` (with `expires_after_tick`), `Receipt`, all error enums from §9.7 (including `MURK_ERROR_SHUTTING_DOWN` and `MURK_ERROR_TICK_DISABLED`), `FieldDef` (scalar/vector/categorical, units, bounds, boundary behavior), `FieldMutability` enum (Static/PerTick/Sparse), `FieldSet`, `FieldReader` trait, `FieldWriter` trait, `SnapshotAccess` trait (4 methods: `read_field`, `tick_id`, `world_generation_id`, `parameter_version` — Decision N).
- **Acceptance:** All types compile. Error code enum is exhaustive per §9.7. Property test: `FieldSet` operations (union, intersection, difference) satisfy set algebra axioms.

### WP-2: Arena Allocator
- **Description:** Generational arena with segmented bump allocation, double-buffer ping-pong, sparse slab, static gen-0 arena.
- **Complexity:** L
- **Hard deps:** WP-1
- **HLD refs:** §5.1-5.6, R-FIELD-3
- **Deliverables:** `murk-arena` crate with: `ReadArena` (Send+Sync), `WriteArena` (&mut), `FieldHandle` (generation-scoped integer: generation u32, segment_index u16, offset u32, len u32), segmented arena (64MB segments, linked list), double-buffer ping-pong for Lockstep (§5.6), sparse slab with promotion on reclaim, static generation-0 arena, `ScratchRegion` bump allocator, implements `FieldReader` for `ReadArena` and `FieldWriter` for `WriteArena`.
- **Phase 1 (WP-2 through WP-5):** `Vec<f32>` zero-init for all arena allocations. Safe during early development. `cfg(feature = "zero-init-arena")` as permanent safety net opt-in (Decision B).
- **Phase 2 (after WP-4 delivers FullWriteGuard):** Migrate to `MaybeUninit<f32>` + `FullWriteGuard`. Bounded unsafe in `crates/murk-arena/src/raw.rs` only (≤5 functions).
- **FullWriteGuard:** Debug builds: `BitVec` coverage tracking per cell, panics on drop if `Full` write buffer incompletely written (diagnostic: propagator name, field ID, coverage %). Release builds: bare `&mut [MaybeUninit<f32>]`, zero overhead.
- **Acceptance:** Property tests: arbitrary field counts × sizes × generation sequences round-trip correctly. Handle from gen N invalid after gen N+2 recycling. Static fields share allocation across generations (pointer equality). Sparse fields share until modified. Memory bound: after N Lockstep ticks, arena size ≤ 2× PerTick + 1× Static + Sparse slab. Miri clean. Unsafe limited to `raw.rs` (≤5 functions, each with `// SAFETY:` comment). Criterion micro-benchmark: allocate/publish/resolve cycle.
- **M2 extension:** Static generation-0 arena wrapped in `Arc` for cross-environment sharing in vectorized training. 128 `LockstepWorld` instances MUST share a single static arena allocation. This is the mechanism behind M2's "Static field sharing via Arc" quality gate.
- **Risk:** HIGH — handle validation, epoch reclamation interaction (RealtimeAsync deferred to WP-10).

### WP-3: Space Trait and Simple Backends
- **Description:** Space abstraction + Line1D, Ring1D, Square4, Square8.
- **Complexity:** M
- **Hard deps:** WP-1
- **HLD refs:** R-SPACE-0 through R-SPACE-7
- **Sub-packages:**
  - **WP-3a:** Space trait definition + Line1D + Ring1D (S)
  - **WP-3b:** Square4 + Square8 (S)
  - **WP-3c:** ~~VoxelOctreeSpace~~ **DEFERRED to v1.5.** No existing voxel/octree system exists to wrap. The lattice backends (Line1D, Ring1D, Square4, Square8, Hex2D) cover all v1 use cases. VoxelOctreeSpace can be added later without affecting the Space trait design — trait compliance tests from WP-3a validate the abstraction. Removed from M0 quality gate.
- **Deliverables:** `murk-space` crate with: `Space` trait (`Space: Any + Send + 'static`; ndim, cell_count, neighbours, distance, compile_region, iter_region, map_coord_to_tensor_index, canonical_ordering), `downcast_ref::<T>()` on `dyn Space` for opt-in specialization (Decision M), `Coord` (SmallVec<[i32; 4]>), 4 lattice backends with boundary handling (clamp, wrap, absorb), region types (`RegionSpec`, `RegionPlan` — see §6.6).
- **Acceptance:** Per-topology unit tests (neighbours, distance, iteration). Property tests: distance is a metric (d(a,a)=0, symmetry, triangle inequality). Neighbours symmetric (b in neighbours(a) iff a in neighbours(b)). Iteration deterministic (call twice → same sequence). RegionPlan: valid_ratio = 1.0 for All on rectangular grids.

### WP-4: Propagator Pipeline
- **Description:** Propagator trait, StepContext with split-borrow semantics, pipeline validation, execution.
- **Complexity:** L
- **Hard deps:** WP-1, WP-3a (Space trait)
- **Soft deps:** WP-2 (for real arena testing; use mocks initially)
- **HLD refs:** R-PROP-1 through R-PROP-5, §15.2, §15.3
- **Deliverables:** `murk-propagator` crate with: `Propagator` trait (&self, Send + 'static), `StepContext<R: FieldReader, W: FieldWriter>` with `reads` (in-tick overlay view) and `reads_previous` (frozen tick-start view), `WriteMode` enum (Full/Incremental), pipeline validation (write-write conflict detection, DAG consistency with user-provided order, dt validation, Incremental budget estimation, all field refs exist), `PropagatorError`. 3 validation fixtures: `IdentityPropagator`, `ConstPropagator`, `FailingPropagator` in murk-test-utils.
- **Acceptance:** Pipeline validation rejects write-write conflicts. Rejects unresolvable field IDs. Rejects dt > min(max_dt). Property test: arbitrary propagator DAGs validate iff no conflicts and all refs exist. StepContext mock tests verify split-borrow semantics (reads sees staged, reads_previous doesn't). **FullWriteGuard acceptance:** debug-mode `BitVec` tracking catches incomplete `Full` writes with diagnostic (propagator name, field ID, coverage %) — Decision B. **`downcast_ref` documented:** propagator examples show opt-in specialization via `ctx.space.downcast_ref::<Square4Space>()` — Decision M. Criterion: pipeline validation time for 10 propagators.

### WP-5: TickEngine Core
- **Description:** Central tick execution: drain ingress → validate → run propagators → publish snapshot.
- **Complexity:** L
- **Hard deps:** WP-2, WP-4
- **Soft deps:** WP-3 (need at least Square4 for testing)
- **HLD refs:** R-ARCH-1, R-ARCH-3, §9.1 (tick atomicity)
- **Sub-packages:**
  - **WP-5a:** Command Processing (M) — command drain with `expires_after_tick` evaluation, deterministic ordering (priority_class → source_id/source_seq → arrival_seq, stable sort), receipt generation, ingress bounded queue, `arrival_seq` monotonic counter. Independently testable without propagators or arena overlay.
  - **WP-5b:** Tick Execution and Atomicity (L) — **precomputed ReadResolutionPlan** (per-propagator read routing built at startup), propagator execution in dependency order with overlay resolution, tick atomicity (all-or-nothing via arena abandon on propagator failure — commands dropped with `TICK_ROLLBACK`, not re-enqueued), snapshot descriptor publication, **configurable NaN sentinel check** on written fields (HLD §9.2: off by default, enabled via pipeline config), **`consecutive_rollback_count` tracking** and **`tick_disabled` mechanism** (AtomicBool set after 3 consecutive rollbacks; ingress rejects with `MURK_ERROR_TICK_DISABLED`; recovery via `reset()` — Decision J), `murk_consecutive_rollbacks()` C ABI query function.
- **Critical sub-task:** Overlay resolution (WP-5b) — the ReadResolutionPlan that routes `reads()` to base gen or staged writes from prior propagators. This is ~50-100 lines with the highest correctness criticality in the engine.
- **Acceptance:**
  - **WP-5a:** Command ordering unit tests (priority, source disambiguation, arrival_seq tiebreak). TTL rejection test (expired → STALE). Property test: arbitrary command batches sorted deterministically. Receipt fields complete per §14.2.
  - **WP-5b:** Tick atomicity test (propagator failure → no snapshot, state unchanged). **Three-propagator overlay visibility test** (5 cases: A writes X, B reads X via reads() sees A's value; B reads X via reads_previous() sees base gen; C reads X sees B's staged value if B wrote; etc.). ReadResolutionPlan unit test (precomputed routing correct). Criterion: full tick cycle on reference profile.
- **Risk:** HIGH — overlay resolution (WP-5b) is the single highest-risk correctness item. Splitting into WP-5a/5b allows command processing to be validated independently before overlay complexity is introduced.

### WP-6: Lockstep Mode
- **Description:** `LockstepWorld` callable struct with step_sync(), reset(), &mut self lifecycle.
- **Complexity:** M
- **Hard deps:** WP-5
- **Soft deps:** WP-7 (ObsPlan for obs, but M0 uses direct field reads)
- **HLD refs:** R-MODE-1, §7.1, §5.6, §8.1
- **Deliverables:** `LockstepWorld` in `murk-engine`: `step_sync(&mut self, commands) -> StepResult<&Snapshot>`, `reset(&mut self, seed) -> &Snapshot`, double-buffer ping-pong arena recycling, Send not Sync. **Graceful shutdown:** `LockstepWorld` implements `Drop`; `&mut self` guarantees no outstanding borrows; arena reset reclaims all memory (Decision E).
- **Acceptance:** 1000-step determinism (2 runs, same seed+commands → bit-exact snapshots at every tick). Memory bound assertion (RSS at tick 1000 ≈ tick 10). Reset reclaims both buffers. &mut self prevents snapshot aliasing (compile-time test). Basic telemetry: per-step timing (total, per-propagator), memory usage reporting (R-OPS-1, Lockstep subset).
- **M0 exit:** This WP + reference propagators = M0 complete.

### WP-7: ObsSpec/ObsPlan (Simple Plan Class)
- **Description:** Observation specification, compilation to executable plan, tensor fill, validity masks.
- **Complexity:** L
- **Hard deps:** WP-1, WP-3
- **Soft deps:** WP-5 (need real snapshots to test against; use `MockSnapshot` from murk-test-utils initially)
- **Note:** murk-obs does NOT depend on murk-arena (Decision N). ObsPlan reads via `&dyn SnapshotAccess`, not `ReadArena` directly. This allows WP-7 to start in parallel with WP-2 and WP-5.
- **HLD refs:** R-OBS-1 through R-OBS-9, §16.1
- **Sub-packages:**
  - **WP-7a:** Rust-native ObsSpec + ObsPlan compilation + Simple plan class (flat gather) (M)
  - **WP-7b:** Generation binding + PLAN_INVALIDATED detection (S)
- **Deliverables:** `murk-obs` crate with: `ObsSpec` (Rust struct for M0-M2, FlatBuffers at M3), `ObsPlan::compile()`, Simple plan class (branch-free gather into caller-allocated buffer), validity masks, `valid_ratio` computation and threshold checks (< 0.5 warn, < 0.35 error), plan caching by generation ID, all 6 metadata fields (tick_id, age_ticks, coverage, validity_mask, world_generation_id, parameter_version).
- **Acceptance:** Flat tensor fill correct for Square4. PLAN_INVALIDATED on generation mismatch. valid_ratio = 1.0 for square grids. All metadata fields populated.

### WP-8: C ABI (FFI)
- **Description:** Handle-based C ABI with full error model, lifecycle safety.
- **Complexity:** L
- **Hard deps:** WP-6, WP-7
- **HLD refs:** R-FFI-1 through R-FFI-5, §9.6, §18
- **Deliverables:** `murk-ffi` crate: opaque handles (slot-based with generation counter, not raw pointers), create/destroy lifecycle (double-destroy safe, use-after-destroy returns INVALID_HANDLE), `murk_abi_version()`, `murk_dt_range()`, caller-allocated buffers, `murk_lockstep_step()`, `murk_lockstep_reset()`, `murk_obsplan_compile()`, `murk_obsplan_execute()`, `murk_lockstep_step_vec()` (MUST v1; dispatches via rayon thread pool for M2 vectorized training), all error codes from §9.7.
- **Acceptance:** Handle lifecycle proptest (random create/step/observe/destroy sequences → no UB). Miri clean. Error code coverage (every error path returns defined code). double-destroy no-op. null-handle → error code. Criterion: FFI overhead per call.

### WP-9: Python Bindings
- **Description:** PyO3 wrapping C ABI with GIL release, NumPy integration, Gymnasium interface.
- **Complexity:** M
- **Hard deps:** WP-8
- **HLD refs:** R-FFI-2, R-FFI-5, §18
- **Deliverables:** `murk-python` crate (PyO3): `MurkEnv(gymnasium.Env)` class with step/reset/observation_space/action_space, `MurkVecEnv(gymnasium.vector.VectorEnv)` with auto-reset, GIL released via `py.allow_threads()` on all C ABI calls, NumPy buffer integration (caller-allocated, zero-copy pointer pass), context manager for handle lifecycle. **PettingZoo Parallel API** (R-MODE-4, SHOULD v1): `MurkParallelEnv(pettingzoo.ParallelEnv)` wrapper for multi-agent environments if schedule permits; interface design MUST be finalized for v1.5.
- **Acceptance:** GIL release verified (N concurrent Python threads make progress during step). Gymnasium compliance (step/reset contract). NumPy buffer contains correct observation data. PPO smoke test (100K steps, learning curve shows improvement — soft gate; **concrete scenario:** single agent on 10×10 Square4 grid, reward = negative Manhattan distance to fixed target cell, episode terminates on arrival or after 200 steps; success = mean episode length decreases over training). PettingZoo: if implemented, passes AEC API compliance test.

### WP-10: Hex2D and ProductSpace
- **Description:** Hex2D lattice backend + ProductSpace composition with dual distance API.
- **Complexity:** XL
- **Hard deps:** WP-3 (Space trait established)
- **Soft deps:** WP-7 (ObsPlan for hex tensor export testing)
- **HLD refs:** R-SPACE-4 through R-SPACE-12, §11.1, §12
- **Sub-packages:**
  - **WP-10a:** Hex2D (M) — axial coords, 6 neighbours, cube-distance, canonical ordering (r-then-q), hex tensor export (bounding box + validity mask, branch-free gather with precomputed index tables)
  - **WP-10b:** ProductSpace (L) — stores components as `Vec<Box<dyn Space>>` (Decision M; vtable dispatch handles nesting naturally), composition, per-component neighbours (R-SPACE-8), L1 graph-geodesic distance (R-SPACE-9), `metric_distance()` with configurable metrics, lexicographic iteration (R-SPACE-10), region queries as Cartesian products (R-SPACE-11), `valid_ratio` computation (product of per-component ratios)
- **Acceptance:** Hex2D: 6 neighbours in documented order, distance matches BFS, valid_ratio converges to 0.75 for large R. ProductSpace: worked examples from HLD §11.1 (Hex2D×Line1D distance, neighbours, iteration). Property tests: distance metric axioms, neighbour symmetry, BFS = geodesic. valid_ratio ≥ 0.35 for all v1 compositions. Hex2D×Hex2D ≈ 0.56 (warns, doesn't fail).
- **Risk:** MEDIUM — Hex2D tensor export mapping (branch-free gather with precomputed index tables) is deceptively hard. ProductSpace lexicographic iteration with mixed-dimensionality components needs careful nesting.

### WP-11: Foveation and Standard ObsPlan
- **Description:** Agent-centred observation windows, pooling, Standard plan class, FlatBuffers ObsSpec.
- **Complexity:** L
- **Hard deps:** WP-7, WP-10
- **HLD refs:** §16.1, §16.3
- **Deliverables:** Agent-relative regions with interior/boundary dispatch (O(1) check, ~90% interior path for radius < grid/4), pooling operations, Standard plan class, ObsPlan caching with rate-limited recompilation, FlatBuffers ObsSpec serialization for cross-language use. **Batch ObsPlan execution** (R-OBS-8, SHOULD v1): single traversal fills N agent observation buffers; interface MUST be designed even if batch execution is deferred to v1.5.
- **Acceptance:** Interior/boundary dispatch produces identical results (functional equivalence test). Hex foveation correct (hex disk region + validity mask). FlatBuffers round-trip test.

### WP-12: RealtimeAsync Mode
- **Description:** RealtimeAsyncWorld with TickEngine thread, egress pool, epoch reclamation.
- **Complexity:** XL
- **Hard deps:** WP-5, WP-7
- **HLD refs:** R-MODE-1, §7.2, §8.2, §8.3, P-1, P-3
- **Deliverables:** `RealtimeAsyncWorld` in `murk-engine`: TickEngine on dedicated thread, snapshot ring buffer (default K=8, configurable; count + byte-budget eviction), egress thread pool, epoch-based reclamation with stalled worker teardown (§8.3: max_epoch_hold, cancellation flag, cooperative check between region iterations), 60Hz wall-clock deadline, fallback snapshot selection, `ttl_ms → expires_after_tick` conversion at ingress, adaptive max_tick_skew, backpressure policy (R-MODE-2), telemetry (tick duration, queue depth, snapshot age). **Graceful shutdown protocol** (Decision E): 4-state machine (Running→Draining→Quiescing→Dropped), bounded timeouts (≤300ms total: ~33ms drain + ~200ms quiesce + ~10ms join), reuses §8.3 stalled-worker machinery, returns `ShutdownResult` with phase reporting, `MURK_ERROR_SHUTTING_DOWN` for commands arriving during shutdown. **`tick_disabled` mechanism** (Decision J): `tick_disabled: AtomicBool` integration with shutdown (thread stays alive for orderly shutdown even when tick-disabled).
- **Acceptance:** 60Hz sustained under reference profile. P-1 verified (egress always returns — stalled workers get ObsError::ExecutionFailed with WORKER_STALLED). Stress tests: death spiral (§23 #15), mass invalidation (#16), rejection oscillation (#17). Epoch reclamation: memory bounded under sustained load.
- **Risk:** HIGH — epoch reclamation + stalled worker interaction is the hardest correctness problem. Implement LAST among engine features.

### WP-13: Replay System
- **Description:** Replay log format, recording, playback, determinism CI framework.
- **Complexity:** M
- **Hard deps:** WP-1, WP-5
- **Soft deps:** WP-6 (Lockstep is primary replay target)
- **HLD refs:** R-DET-1 through R-DET-6, §14.3-14.4, §19.1
- **Deliverables:** `murk-replay` crate: replay log format (init descriptor + seed + per-tick command records with full ordering provenance + build metadata header per R-DET-5), `ReplayWriter`, `ReplayReader`, CI replay-and-compare framework (per-tick bit-exact snapshot comparison with first-divergence reporting), determinism source catalogue (R-DET-6, initial document).
- **Acceptance:** All 8 determinism replay scenarios pass (5 from §19.1 MUST + 3 SHOULD: tick rollback recovery, GlobalParameter mid-episode, 10+ propagator pipeline). 1000+ tick minimum. First-divergence reporting includes tick ID, field ID, byte offset.

### WP-14: Reference Propagators and Examples
- **Description:** 3 polished reference propagators + reference profile scenario.
- **Complexity:** M
- **Hard deps:** WP-4, WP-5, WP-6
- **Soft deps:** WP-7 (for end-to-end examples)
- **HLD refs:** §15.3, R-PERF-3
- **Deliverables:** Diffusion propagator (Full write, spatial averaging via reads_previous — Jacobi style), agent movement propagator (Incremental, command-driven), reward propagator (Full, reads all prior outputs). Reference profile scenario: 10K cells (100×100 Square4), 5 fields, 3 propagators, 16 agents. Stress scenario: 100K cells. End-to-end Lockstep RL example.
- **Acceptance:** Reference profile benchmarks within HLD targets. Diffusion produces correct steady-state. Movement respects topology boundaries.

### WP-15: Integration Tests, Stress Tests, and CI
- **Description:** Complete mandatory test set (§23), CI benchmarks, final quality gates.
- **Complexity:** L
- **Hard deps:** WP-6, WP-7, WP-12, WP-13, WP-14
- **HLD refs:** §23 (all 17 tests), R-PERF-3, §19.1
- **Deliverables:** All 17 §23 mandatory tests (unit/property, integration, stress). CI benchmark pipeline with Criterion (15 micro-benchmarks) + custom system harness (throughput, memory growth). Regression detection: -5% warns, -10% blocks (see §6.13 for CI infrastructure). Reference profile CI artifact. Graceful shutdown tests. NaN detection (configurable sentinel check). Determinism catalogue complete and reviewed. Arena fragmentation profiling under sustained RealtimeAsync load (Risk #10). Error reference document and replay format specification document.
- **Acceptance:** All §23 tests pass. Benchmarks within budget. R-DET-6 catalogue reviewed. No regressions from baseline.

---

## 3. Dependency DAG

```
WP-0 (Test Infra) ──────────────────────────────────────────────────────┐
  │                                                                      │
  └─→ WP-1 (Core Types) ───────────────────────────────────────────────┤
        ├─→ WP-2 (Arena) ──────────────────────────┐                    │
        ├─→ WP-3a (Space trait + Line/Ring) ───────┤                    │
        │     ├─→ WP-3b (Square4/Square8) ────────┤                    │
        │     ├── [WP-3c deferred to v1.5]         │                    │
        │     └─→ WP-10a (Hex2D) ─────────────────┤                    │
        │           └─→ WP-10b (ProductSpace) ────┤                    │
        │                                           │                    │
        ├─→ WP-4 (Propagator Pipeline) ───────────┤                    │
        │     [uses mocks, no arena dep]            │                    │
        │                                           │                    │
        ├─→ WP-7a (ObsPlan Simple) ───────────────┤                    │
        │     [uses MockSnapshot, no arena dep]     │                    │
        │     └─→ WP-7b (Generation binding) ─────┤                    │
        │                                           │                    │
        ┌───────────────────────────────────────────┘                    │
        v                                                                │
  WP-5 (TickEngine Core) ──────────────────────────────────────────────┤
    │  WP-5a: Command Processing (can test independently)               │
    │  WP-5b: Tick Execution + Atomicity (highest-risk)                 │
    │                                                                    │
    ├─→ WP-6 (Lockstep Mode) ──────────────────────┐                   │
    │                                                │                   │
    ├─→ WP-8 (C ABI / FFI) ───[needs WP-6+WP-7]──┤                   │
    │     └─→ WP-9 (Python / PyO3) ───────────────┤                   │
    │                                                │                   │
    ├─→ WP-11 (Foveation + Standard ObsPlan) ─────┤                   │
    │                                                │                   │
    ├─→ WP-12 (RealtimeAsync) ────────────────────┤                   │
    │                                                │                   │
    ├─→ WP-13 (Replay System) ────────────────────┤                   │
    │                                                │                   │
    ├─→ WP-14 (Reference Propagators) ────────────┤                   │
    │                                                │                   │
    └─→ WP-15 (Tests + CI) ───────────────────────┘                   │
```

**Note:** WP-7a starts from WP-1 + WP-3a (parallel with WP-2, WP-4, WP-5), not after WP-5. This shortens the M1 critical path. WP-8 converges from WP-6 (engine) and WP-7 (observations).

### Critical Path (M0)

**WP-0 → WP-1 → WP-2 + WP-3a + WP-4 (parallel) → WP-5a → WP-5b → WP-6**

This is the shortest path to a working Lockstep engine with direct field reads. WP-5a (command processing) can be validated independently before WP-5b (overlay resolution) introduces the highest-risk complexity.

### Critical Path (M1 — first Python training run)

**M0 path; WP-7a starts after WP-1+WP-3a (parallel with WP-2/WP-5); converges at WP-8 (needs WP-6+WP-7a) → WP-9**

WP-7a no longer gates on WP-5 (uses MockSnapshot), shortening the M1 critical path.

### Parallelizable Tracks

| Track | Packages | Can Start After |
|-------|----------|----------------|
| **A: Engine core** | WP-2 → WP-5a → WP-5b → WP-6 | WP-1 |
| **B: Spaces** | WP-3a → WP-3b → WP-3c, WP-10a → WP-10b | WP-1 |
| **C: Observations** | WP-7a → WP-7b, WP-11 | WP-1 + WP-3a (MockSnapshot; no arena dep) |
| **D: Replay** | WP-13 | WP-1 |
| **E: Propagator** | WP-4 (with mocks) | WP-1 + WP-3a |

Tracks A, B, C, D, and E are independent until they converge at WP-5 (A needs WP-2+WP-4) and WP-8 (needs WP-6+WP-7).

---

## 4. Milestones and Quality Gates

### M0: Core Engine (Rust-Only Hello World)

**Feature gate:** `LockstepWorld::step_sync()` and `reset()` work end-to-end with 3 reference propagators on Square4.

**WPs required:** WP-0, WP-1, WP-2, WP-3a/3b, WP-4, WP-5, WP-6

**M0 world configuration:**
- Space: Square4 10×10 (100 cells)
- Fields: 5 (terrain/Static, agent_position/PerTick/Incremental, temperature/PerTick/Full, smoothed_temp/PerTick/Full/reads_previous, reward+done/PerTick/Full)
- Propagators: 3 (AgentMovement, SmoothField with reads_previous, RewardCompute)
- Observation: Direct `&[f32]` field reads (no ObsPlan)

**Quality gate:**
- [ ] 1000-step determinism (2 runs, same seed → bit-exact at every tick)
- [ ] Memory bound (RSS tick 1000 ≈ tick 10)
- [ ] WP-5a: Command ordering unit tests (priority, source, arrival_seq tiebreak)
- [ ] WP-5b: Three-propagator overlay visibility test (5 cases)
- [ ] ReadResolutionPlan unit test (precomputed routing correct)
- [ ] Rollback negative test (propagator failure → state unchanged)
- [ ] Arena property tests pass
- [ ] No `unsafe` blocks outside murk-arena `raw.rs` (≤5 functions, Decision B)
- [ ] No `HashMap` in deterministic crates (clippy lint)
- [ ] Criterion micro-benchmarks baselined

### M1: Python Integration (First Training Run)

**Feature gate:** Python → C ABI → Lockstep → ObsPlan → NumPy buffer. Gymnasium-compatible single-env wrapper.

**WPs required:** M0 + WP-7a, WP-7b, WP-8, WP-9

**Quality gate:**
- [ ] FFI handle lifecycle (proptest: random create/step/observe/destroy)
- [ ] GIL release verified (N concurrent Python threads)
- [ ] All §9.7 error codes exercised
- [ ] PLAN_INVALIDATED on generation mismatch
- [ ] Pipeline-level determinism
- [ ] NumPy zero-copy pointer validation
- [ ] PPO smoke test: 100K steps, learning curve shows improvement (soft)
- [ ] Cumulative: all M0 tests still pass

### M2: Vectorized Training

**Feature gate:** 16-128 envs stepping in parallel from single Python call. Static field sharing via Arc.

**WPs required:** M1 + `step_vec` in WP-8, rayon thread pool, WP-14 (reference propagators — needed for meaningful scaling benchmarks)

**M2 benchmark workload:** Reference profile from WP-14 (10K cells, 5 fields, 3 propagators, 16 agents). The M0 hello-world scenario (100 cells) is too small to stress vectorization.

**Quality gate:**
- [ ] ≥80% per-core throughput scaling (95% CI method) on reference profile workload
- [ ] Memory < 7MB per env in Lockstep
- [ ] Per-env determinism (each env independently deterministic)
- [ ] Static field sharing verified (128 envs share one allocation)
- [ ] 2000-tick memory stability (no growth)
- [ ] Cumulative: all M0+M1 tests still pass

### M3: Spatial Diversity

**Feature gate:** Hex2D, ProductSpace (Hex2D×Line1D at minimum), foveation, FlatBuffers ObsSpec.

**WPs required:** M2 + WP-10a, WP-10b, WP-11

**Quality gate:**
- [ ] Spatial property tests (metric axioms, neighbour symmetry, BFS=geodesic)
- [ ] valid_ratio correct for all v1 compositions
- [ ] Hex2D×Line1D ObsPlan integration test
- [ ] Interior/boundary dispatch functional equivalence
- [ ] FlatBuffers ObsSpec round-trip
- [ ] Cumulative: all prior tests still pass

### M4: Production Readiness (RealtimeAsync)

**Feature gate:** 60Hz sustained under reference profile. Epoch reclamation. Adaptive backpressure.

**WPs required:** M3 + WP-12

**Quality gate:**
- [ ] Stress tests pass (§23 #15 death spiral, #16 mass invalidation, #17 rejection oscillation)
- [ ] Reference profile benchmarks within HLD budgets (§20)
- [ ] Epoch reclamation: memory bounded under sustained load
- [ ] Stalled worker teardown: WORKER_STALLED returned, P-1 satisfied
- [ ] Full §19.1 determinism replay (8 scenarios, 1000+ ticks)
- [ ] Cumulative: all prior tests still pass

### M5: v1 Release

**Feature gate:** All §22 v1 deliverables complete.

**WPs required:** M4 + WP-13, WP-14, WP-15

**Quality gate:**
- [ ] All 17 §23 mandatory tests pass
- [ ] Determinism catalogue (R-DET-6) reviewed and complete
- [ ] Performance regression CI active (-5% warn, -10% block)
- [ ] Reference profile CI artifact published
- [ ] VoxelOctreeSpace deferred to v1.5 (R-MIG-1 — no existing system; lattice backends validate Space trait)
- [ ] Graceful shutdown tested
- [ ] Documentation complete: API docs via `deny(missing_docs)` rustdoc, error reference document (all §9.7 codes with scenarios and recovery guidance), replay log format specification document. **Note:** Error reference and replay format docs require explicit authoring effort beyond rustdoc — allocate during WP-13/WP-15.
- [ ] Cumulative: all prior tests still pass

---

## 5. Risk Register

| # | Risk | Severity | WP | Mitigation |
|---|------|----------|-----|-----------|
| 1 | **Overlay resolution correctness** | Critical | WP-5b | Precomputed ReadResolutionPlan (zero runtime conditionals). Three-propagator overlay test as WP-5b acceptance criterion. WP-5a/5b split allows command processing to be validated independently first. Determinism replay as safety net. |
| 2 | **Epoch-based reclamation** | Critical | WP-12 | Implement LAST (after Lockstep proven). Property-based tests with arbitrary worker timing. Stress tests §23 #15-17. Consider crossbeam-epoch as starting point. |
| 3 | **Arena allocator correctness** | High | WP-2 | Property-based tests (arbitrary field counts/sizes/generations). Miri for memory safety. Criterion for performance validation. Formal review before WP-5. |
| 4 | **StepContext split-borrow ergonomics** | High | WP-4 | FieldReader/FieldWriter traits enable mock testing. Pre-slicing based on declared reads/writes (validated at startup). Zero unsafe. |
| 5 | **ProductSpace complexity** | High | WP-10b | Start with 2-component (Hex2D×Line1D). Add 3-component only after 2 is solid. HLD §11.1 worked examples as TDD acceptance tests. |
| 6 | **Hex2D tensor export** | Medium | WP-10a | Branch-free gather with precomputed index tables. Limit v1 hex shapes to rectangles + disks (O(1) interior check). Wrap-around hex deferred to v1.5. |
| 7 | **FFI lifecycle safety** | Medium | WP-8 | Slot-based handles with generation counter (not raw pointers). Proptest with random operation sequences. Miri. |
| 8 | **Sparse field misclassification** | Medium | WP-2 | Runtime warning if Sparse field modified N consecutive ticks. DRL field classification table as guidance. |
| 9 | **PyO3 build complexity** | Low | WP-9 | Standard toolchain (maturin, abi3). Well-understood ecosystem (polars, ruff, pydantic-core use same approach). |
| 10 | **Arena fragmentation (RealtimeAsync)** | Medium | WP-12 | Long-running RealtimeAsync sessions with Sparse field churn may fragment the sparse slab (HLD §24 Risk #3). Add sustained-load memory profiling to WP-12 stress tests. Periodic compaction during low-load ticks if fragmentation exceeds threshold. |

---

## 6. Interface Contracts

### 6.1 murk-core (Leaf Crate)

```rust
// IDs
pub struct FieldId(u32);
pub struct SpaceId(u32);
pub struct TickId(u64);
pub struct WorldGenerationId(u64);
pub struct ParameterVersion(u64);

// Field model
pub struct FieldDef { /* scalar/vector/categorical, units, bounds, boundary */ }
pub enum FieldMutability { Static, PerTick, Sparse }
pub struct FieldSet { /* bitset of FieldId */ }

// Abstraction traits (implemented by arena, consumed by propagators)
pub trait FieldReader {
    fn read(&self, field: FieldId) -> Option<&[f32]>;
}
pub trait FieldWriter {
    fn write(&mut self, field: FieldId) -> Option<&mut [f32]>;
}

// Command/Receipt
pub struct Command {
    pub payload: CommandPayload,
    pub expires_after_tick: TickId,
    pub source_id: Option<u64>,
    pub source_seq: Option<u64>,
    pub priority_class: u8,       // lower = higher priority (0 = system, 1 = user default)
    pub arrival_seq: u64,         // set by ingress, monotonic
}

/// Parameter key for global simulation parameters (e.g., learning rate, reward scale).
/// Registered at world creation; invalid keys rejected at ingress.
pub struct ParameterKey(u32);

/// All command payloads. WorldEvent variants affect per-cell state;
/// GlobalParameter variants affect simulation-wide scalars.
pub enum CommandPayload {
    // --- WorldEvent variants ---
    /// Move entity to target coordinate. Rejected if entity_id unknown or target out of bounds.
    Move { entity_id: u64, target_coord: Coord },
    /// Spawn new entity at coordinate with initial field values.
    Spawn { coord: Coord, field_values: Vec<(FieldId, f32)> },
    /// Remove entity. Associated field values cleared at next tick.
    Despawn { entity_id: u64 },
    /// Set a single field value at a coordinate. For Sparse fields primarily.
    SetField { coord: Coord, field_id: FieldId, value: f32 },
    /// Extension point: domain-specific commands. type_id is user-registered.
    Custom { type_id: u32, data: Vec<u8> },

    // --- GlobalParameter variants ---
    /// Set a single global parameter. Takes effect at next tick boundary.
    SetParameter { key: ParameterKey, value: f64 },
    /// Batch-set multiple parameters atomically.
    SetParameterBatch { params: Vec<(ParameterKey, f64)> },
}

pub struct Receipt {
    pub accepted: bool,
    pub applied_tick_id: Option<TickId>,
    pub reason_code: Option<IngressError>,
    pub command_index: usize,     // index in the submitted batch
}

// Snapshot access (Decision N — ObsPlan reads through this, not ReadArena directly)
pub trait SnapshotAccess {
    fn read_field(&self, field: FieldId) -> Option<&[f32]>;
    fn tick_id(&self) -> TickId;
    fn world_generation_id(&self) -> WorldGenerationId;
    fn parameter_version(&self) -> ParameterVersion;
}

// Error model (§9.7 — all 16 error codes, including SHUTTING_DOWN and TICK_DISABLED)
pub enum StepError { /* ... */ }
pub enum PropagatorError { /* ... */ }
pub enum ObsError { /* ... */ }
pub enum IngressError { /* ... */ }
```

### 6.2 murk-propagator (User-Facing)

```rust
pub trait Propagator: Send + 'static {
    fn name(&self) -> &str;
    fn reads(&self) -> FieldSet;
    fn reads_previous(&self) -> FieldSet { FieldSet::empty() }
    fn writes(&self) -> Vec<(FieldId, WriteMode)>;
    fn max_dt(&self) -> Option<f64> { None }
    fn scratch_bytes(&self) -> usize { 0 }
    fn step(&self, ctx: &StepContext<'_, impl FieldReader, impl FieldWriter>, dt: f64)
        -> Result<(), PropagatorError>;
}

// Note: HLD §15 shows concrete StepContext<'a> with FieldReadSet/FieldWriteSet.
// This plan uses generics <R: FieldReader, W: FieldWriter> intentionally —
// enables MockFieldReader/MockFieldWriter for propagator unit tests without arena.
pub struct StepContext<'a, R: FieldReader, W: FieldWriter> {
    pub reads: R,           // current in-tick view (overlay)
    pub reads_prev: R,      // frozen tick-start view
    pub writes: W,          // staging arena
    pub scratch: &'a mut ScratchRegion,  // borrowed; pipeline reuses allocation, reset between propagators
    pub space: &'a dyn Space,  // Decision M: &dyn not Box; Space: Any + Send + 'static enables downcast_ref()
    pub tick_id: TickId,
    pub dt: f64,
}
```

### 6.3 murk-engine (Integration Point)

```rust
pub trait World {
    type SnapshotRef<'a>: AsRef<Snapshot> where Self: 'a;
    fn step(&mut self, commands: &[Command]) -> Result<Self::SnapshotRef<'_>, StepError>;
}

pub struct LockstepWorld { /* ... */ }
impl LockstepWorld {
    pub fn step_sync(&mut self, commands: &[Command]) -> StepResult<&Snapshot>;
    pub fn reset(&mut self, seed: u64) -> &Snapshot;
}

pub struct Snapshot {
    pub tick_id: TickId,
    pub world_generation_id: WorldGenerationId,
    pub parameter_version: ParameterVersion,
    // internal: FieldId -> FieldHandle mapping
}
```

### 6.4 murk-ffi (Stable C ABI)

```c
// Lifecycle
uint32_t murk_abi_version(void);
murk_status_t murk_lockstep_create(const MurkConfig* config, MurkWorld** out);
murk_status_t murk_lockstep_destroy(MurkWorld* world);
murk_status_t murk_lockstep_step(MurkWorld* world, const MurkCommand* cmds,
                                  size_t n, MurkSnapshot** out);
murk_status_t murk_lockstep_reset(MurkWorld* world, uint64_t seed, MurkSnapshot** out);
murk_status_t murk_lockstep_step_vec(MurkWorld* const* worlds,
                                      const MurkCommand* cmds, size_t n_worlds,
                                      MurkSnapshot* const* out);
murk_status_t murk_dt_range(MurkWorld* world, double* min_dt, double* max_dt);

// Observation
murk_status_t murk_obsplan_compile(MurkWorld* world, const uint8_t* spec,
                                    size_t len, MurkObsPlan** out);
murk_status_t murk_obsplan_execute(MurkObsPlan* plan, MurkSnapshot* snap,
                                    float* buf, size_t buf_len, uint8_t* mask,
                                    MurkObsResult* result);
murk_status_t murk_obsplan_destroy(MurkObsPlan* plan);

// Telemetry
murk_status_t murk_step_metrics(MurkWorld* world, MurkStepMetrics* out);

// Configuration (builder pattern — avoids complex struct across FFI)
MurkConfig* murk_config_create(void);
murk_status_t murk_config_set_space(MurkConfig* cfg, MurkSpaceType type,
                                     const int32_t* params, size_t n_params);
murk_status_t murk_config_add_field(MurkConfig* cfg, const char* name,
                                     MurkFieldMutability mut, uint32_t size);
murk_status_t murk_config_add_propagator(MurkConfig* cfg, MurkPropagatorHandle prop);
murk_status_t murk_config_set_dt(MurkConfig* cfg, double dt);
murk_status_t murk_config_set_seed(MurkConfig* cfg, uint64_t seed);
murk_status_t murk_config_set_ring_buffer_size(MurkConfig* cfg, size_t size);
murk_status_t murk_config_set_max_ingress_queue(MurkConfig* cfg, size_t size);
murk_status_t murk_config_set_tick_rate_hz(MurkConfig* cfg, double hz);
murk_status_t murk_config_destroy(MurkConfig* cfg);

// Consecutive rollback query (Decision J)
uint32_t murk_consecutive_rollbacks(MurkWorld* world);
```

### 6.5 Arena Implementation Notes

These internals are not part of the public API but are critical for WP-2 implementation correctness.

#### Handle Validation

```rust
/// FieldHandle encodes both location and generation for O(1) validity checking.
/// No lookup table needed — compare handle.generation against arena's live range.
pub struct FieldHandle {
    pub(crate) generation: u32,     // generation when allocated
    pub(crate) segment_index: u16,  // which 64MB segment
    pub(crate) offset: u32,         // byte offset within segment
    pub(crate) len: u32,            // allocation length in f32 elements
}

impl ReadArena {
    /// O(1) handle validation. Returns None if generation is outside
    /// the arena's current live range [current_gen - K, current_gen].
    /// K = ring buffer size (Lockstep: 1, RealtimeAsync: configurable, default 8).
    pub fn resolve(&self, handle: FieldHandle) -> Option<&[f32]> {
        let age = self.current_generation.wrapping_sub(handle.generation);
        if age > self.max_generation_age {
            return None; // stale handle
        }
        // Safety: segment_index and offset validated at allocation time
        Some(self.segment_slice(handle))
    }
}
```

#### Segment Growth Strategy

- **Initial allocation:** First segment (64MB) pre-allocated at `WorldConfig::validate()` time. This ensures OOM is caught at startup, not mid-tick.
- **Growth:** Grow-on-demand. When bump pointer exceeds current segment capacity, allocate a new 64MB segment and link it.
- **Limits:** `max_segments` configurable (default 16 = 1GB total arena capacity). Exceeding limit returns `AllocationError::CapacityExceeded`.
- **Segment size rationale:** 64MB fits L3 cache of most server CPUs. Segments are never freed during runtime (only at shutdown). This avoids fragmentation from segment-level alloc/free cycles.

#### Sparse Slab

```rust
/// Sparse field allocations use a free-list slab within the arena segments.
/// Each allocation records its creation generation for reclamation decisions.
struct SparseAllocation {
    generation_created: u32,
    offset: u32,               // within containing segment
    len: u32,                  // f32 element count
}

/// Sparse slab manages copy-on-write semantics for infrequently-modified fields.
struct SparseSlab {
    allocations: Vec<SparseAllocation>,
    free_list: Vec<u32>,       // indices into allocations vec
}
```

- **Modification:** On write to a Sparse field, allocate new space in the current generation's bump region. Old allocation added to free list.
- **Reclamation (Lockstep):** Immediate via `&mut self` — previous generation's sparse allocations are reclaimable after ping-pong swap.
- **Reclamation (RealtimeAsync):** Epoch-gated — sparse allocation is reclaimable only when no worker holds a pinned epoch referencing its generation. See `docs/design/epoch-reclamation.md`.

#### Static Arena

```rust
/// Separate from the generational arena. Wraps a single Vec<f32> in Arc
/// for sharing across vectorized environments (M2 quality gate).
/// Initialized at world creation, never modified, never reclaimed.
pub struct StaticArena {
    data: Vec<f32>,                    // all Static-mutability fields, contiguous
    field_offsets: IndexMap<FieldId, (usize, usize)>,  // field -> (offset, len)
}

/// Shared handle for vectorized training (128 LockstepWorld instances share one).
pub type SharedStaticArena = Arc<StaticArena>;
```

### 6.6 Region Types

Region types define spatial queries used by ObsPlan compilation, propagator neighbourhood iteration, and space-level operations.

```rust
/// Specifies a region of cells within a Space. Used for observation gathering,
/// propagator spatial queries, and region-scoped operations.
pub enum RegionSpec {
    /// Every cell in the space.
    All,
    /// Topology-aware disk: all cells within `radius` graph-distance of `center`.
    /// For Hex2D, this produces a hexagonal region. For Square4, a diamond.
    Disk { center: Coord, radius: u32 },
    /// Axis-aligned bounding box in coordinate space.
    Rect { min: Coord, max: Coord },
    /// BFS expansion from center to given depth.
    /// Equivalent to Disk for uniform-cost topologies but distinct for ProductSpace.
    Neighbours { center: Coord, depth: u32 },
    /// Explicit list of coordinates. For irregular regions or agent-specific masks.
    Coords(Vec<Coord>),
}

/// Compiled region plan — precomputed at ObsPlan::compile() or propagator setup.
/// All lookups during tick execution are O(1) index operations into these vectors.
pub struct RegionPlan {
    /// Number of cells in the region (may differ from coords.len() due to validity).
    pub cell_count: usize,
    /// Precomputed coordinates in canonical iteration order.
    /// Canonical order: Space::canonical_ordering() projected to region cells.
    pub coords: Vec<Coord>,
    /// Precomputed mapping: coords[i] -> flat tensor index for observation output.
    /// For rectangular bounding boxes, this is a dense index.
    /// For non-rectangular regions (hex disk), some tensor positions are invalid.
    pub tensor_indices: Vec<usize>,
    /// Precomputed validity mask: 1 = valid cell in tensor output, 0 = padding.
    /// Length = bounding_shape product (total tensor elements).
    pub valid_mask: Vec<u8>,
    /// Shape of the bounding tensor that contains this region.
    pub bounding_shape: BoundingShape,
}

/// Shape of the bounding tensor for a compiled region.
pub enum BoundingShape {
    /// N-dimensional rectangular bounding box. Dimensions = spatial dims of space.
    /// E.g., a Hex2D disk of radius 3 has bounding shape [7, 7] with ~75% valid cells.
    Rect(Vec<usize>),
}

impl RegionPlan {
    /// Fraction of tensor elements that are valid (non-padding).
    /// Used for valid_ratio threshold checks (< 0.5 warn, < 0.35 error).
    pub fn valid_ratio(&self) -> f64 {
        let total: usize = self.bounding_shape.total_elements();
        if total == 0 { return 0.0; }
        self.valid_mask.iter().filter(|&&v| v == 1).count() as f64 / total as f64
    }
}
```

**Compilation:** `Space::compile_region(spec: &RegionSpec) -> Result<RegionPlan, SpaceError>` is the primary entry point. Each Space backend implements this method. The RegionPlan is cached by (RegionSpec, WorldGenerationId) pair — recompiled only when the topology changes.

### 6.7 ReadResolutionPlan

The ReadResolutionPlan is the mechanism behind WP-5b's overlay resolution. It is built once at pipeline startup and consulted zero times per tick (it pre-configures the FieldReader instances).

```rust
/// Precomputed at pipeline startup. Maps (propagator_index, field_id) → read source.
/// Zero runtime conditionals: each propagator's FieldReader is constructed from this
/// plan before the propagator's step() is called.
pub struct ReadResolutionPlan {
    /// For each propagator (in execution order), which source provides each readable field.
    /// Outer vec length = propagator count. Inner map: field_id → ReadSource.
    routes: Vec<IndexMap<FieldId, ReadSource>>,
    /// Write-write conflicts detected during plan construction (if any).
    /// Non-empty → pipeline validation fails with PropagatorError::WriteConflict.
    conflicts: Vec<WriteConflict>,
}

/// Where a propagator reads a field from during tick execution.
#[derive(Debug, Clone, Copy)]
enum ReadSource {
    /// Read from base generation (tick-start snapshot). Used for:
    /// - Fields declared in reads_previous()
    /// - Fields in reads() that no prior propagator writes
    BaseGen,
    /// Read from staged write buffer of the propagator at writer_index.
    /// Used for fields in reads() where a prior propagator has written.
    Staged { writer_index: usize },
}

/// Detected write-write conflict (two propagators writing the same field).
#[derive(Debug)]
struct WriteConflict {
    field_id: FieldId,
    first_writer: usize,   // propagator index
    second_writer: usize,  // propagator index
}
```

**Construction Algorithm** (runs once at `TickEngine::new()`):

1. Initialize a `last_writer: IndexMap<FieldId, usize>` (empty).
2. For each propagator P at index `i` in execution order:
   a. For each `field_id` in `P.writes()`: if `last_writer` already contains `field_id`, record a `WriteConflict`. Otherwise, set `last_writer[field_id] = i`.
   b. For each `field_id` in `P.reads()`: look up `last_writer[field_id]`. If found at index `j`, route is `Staged { writer_index: j }`. If not found, route is `BaseGen`.
   c. For each `field_id` in `P.reads_previous()`: always route to `BaseGen` regardless of prior writers.
3. If `conflicts` is non-empty, return `Err(PropagatorError::WriteConflict(conflicts))`.

**Runtime usage:** Before calling `propagator[i].step()`, the engine constructs a `FieldReader` that, for each field, either returns the base generation slice or the staged write buffer from `routes[i]`. This is a simple index lookup — no conditionals in the hot path.

### 6.8 World Configuration

WorldConfig is the entry point for creating any Murk world. It is the single most user-facing type in the Rust API, and the C ABI builder pattern (§6.4) constructs one behind an opaque handle.

```rust
/// Complete configuration for a Murk world. Validated before world creation.
pub struct WorldConfig {
    /// Spatial topology. Boxed trait object — Space: Any + Send + 'static.
    pub space: Box<dyn Space>,
    /// Field definitions, registered in order. FieldId = index into this vec.
    pub fields: Vec<FieldDef>,
    /// Propagators in execution order. Order determines overlay resolution (§6.7).
    pub propagators: Vec<Box<dyn Propagator>>,
    /// Simulation timestep (seconds). Must satisfy all propagators' max_dt constraints.
    pub dt: f64,
    /// Random seed for deterministic initialization and reset.
    pub seed: u64,
    /// Snapshot ring buffer size (RealtimeAsync only). Default: 8.
    /// Lockstep ignores this (uses double-buffer ping-pong).
    pub ring_buffer_size: usize,
    /// Maximum ingress queue depth. Default: 1024.
    /// Commands beyond this limit are rejected with QUEUE_FULL.
    pub max_ingress_queue: usize,
    /// Target tick rate (RealtimeAsync only). Default: 60.0 Hz.
    /// Lockstep ignores this (caller-driven).
    pub tick_rate_hz: Option<f64>,
    /// Adaptive backoff parameters (RealtimeAsync only).
    /// See docs/design/epoch-reclamation.md §4.
    pub backoff: BackoffConfig,
}

/// Adaptive backoff configuration for RealtimeAsync command rejection.
pub struct BackoffConfig {
    pub initial_max_skew: u64,           // default: 2 ticks
    pub backoff_factor: f64,             // default: 1.5
    pub max_skew_cap: u64,              // default: 10 ticks
    pub decay_rate: u64,                 // default: 1 tick per 60 rejection-free ticks
    pub rejection_rate_threshold: f64,   // default: 0.20 (20%)
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial_max_skew: 2,
            backoff_factor: 1.5,
            max_skew_cap: 10,
            decay_rate: 60,
            rejection_rate_threshold: 0.20,
        }
    }
}

impl WorldConfig {
    /// Validate configuration before world creation. Runs R-PROP-5 validation
    /// (write conflicts, dt bounds, field refs exist, Incremental budget).
    /// Also validates: space is non-empty, at least one field defined,
    /// ring_buffer_size ≥ 2, max_ingress_queue ≥ 1.
    pub fn validate(&self) -> Result<(), ConfigError>;
}

impl LockstepWorld {
    /// Create a new Lockstep world. Validates config, pre-allocates arena,
    /// builds ReadResolutionPlan, returns ready-to-step world.
    pub fn new(config: WorldConfig) -> Result<Self, ConfigError>;
}

impl RealtimeAsyncWorld {
    /// Create a new RealtimeAsync world. Validates config, spawns TickEngine thread
    /// and egress worker pool, returns handle. World starts in Running state.
    pub fn new(config: WorldConfig) -> Result<Self, ConfigError>;
}
```

**C ABI builder pattern** (§6.4 extended): `MurkConfig` is an opaque handle wrapping a `WorldConfig` under construction. The builder functions (`murk_config_create`, `murk_config_add_field`, `murk_config_set_space`, etc.) populate it incrementally. `murk_lockstep_create` consumes the config handle (moves ownership), validates, and creates the world. This avoids passing complex nested structs across FFI boundaries.

**Python:** `MurkEnv(space=Square4(10, 10), fields=[...], propagators=[...], dt=0.016)` — PyO3 constructs a `WorldConfig` from keyword arguments, calls `validate()`, then `LockstepWorld::new()`.

### 6.9 Observation Types

Concrete types for the observation pipeline. ObsSpec is user-facing (defines what to observe). ObsPlan is compiled (defines how to observe efficiently).

```rust
/// User-facing observation specification. Defines which fields to observe,
/// from which spatial region, with what transform.
pub struct ObsSpec {
    pub entries: Vec<ObsEntry>,
}

/// A single observation entry: one field from one region with one transform.
pub struct ObsEntry {
    /// Which field to observe.
    pub field_id: FieldId,
    /// Spatial region to gather from (compiled to RegionPlan at ObsPlan::compile).
    pub region: RegionSpec,
    /// Transform applied after gathering. Identity for v1 Simple plan class.
    pub transform: ObsTransform,
    /// Output dtype. f32 for all v1 use cases.
    pub dtype: ObsDtype,
}

/// Transform applied to gathered field values before writing to output tensor.
pub enum ObsTransform {
    /// No transform. Raw field values copied to output.
    Identity,
    /// Linear normalization: output = (value - min) / (max - min).
    /// Clamps to [0, 1] range.
    Normalize { min: f64, max: f64 },
    // v1.5: Pool { kernel: PoolKernel, stride: usize }
    // v1.5: Foveate { shells: Vec<FoveationShell> }
}

/// Output element type.
pub enum ObsDtype {
    F32,
    // v1.5: F16, U8 (for categorical fields)
}

/// Result of ObsPlan::compile() — contains compiled plan + validation info.
pub struct ObsPlanResult {
    /// Shape of the output tensor (flattened across all entries).
    pub output_shape: Vec<usize>,
    /// Fraction of output tensor elements that are valid (non-padding).
    pub valid_ratio: f64,
    /// The compiled plan (opaque to callers; used by execute()).
    pub plan: ObsPlan,
    /// Metadata about the compilation.
    pub metadata: ObsMetadata,
}

/// Metadata attached to every observation execution result.
/// All 6 fields required by HLD R-OBS-7.
pub struct ObsMetadata {
    /// Tick ID of the snapshot this observation was gathered from.
    pub tick_id: TickId,
    /// How many ticks old the snapshot is relative to current engine tick.
    /// Lockstep: always 0. RealtimeAsync: may be > 0 due to ring buffer lag.
    pub age_ticks: u64,
    /// Fraction of requested cells that had valid data (valid_ratio).
    pub coverage: f64,
    /// World generation ID — used for PLAN_INVALIDATED detection.
    pub world_generation_id: WorldGenerationId,
    /// Parameter version — incremented when GlobalParameter commands applied.
    pub parameter_version: ParameterVersion,
}

impl ObsPlan {
    /// Execute the observation plan against a snapshot.
    /// Writes gathered values into caller-allocated buffer.
    /// Validity mask written to separate caller-allocated buffer (same spatial shape).
    pub fn execute(
        &self,
        snapshot: &dyn SnapshotAccess,
        buffer: &mut [f32],
        mask: &mut [u8],
    ) -> Result<ObsMetadata, ObsError>;

    /// Fill N observation buffers from a single snapshot traversal.
    /// Each agent has its own region (agent-relative) but shares the same spec.
    /// v1: may implement as N sequential execute() calls internally.
    /// v1.5: single traversal with batched gather for cache efficiency.
    pub fn execute_batch(
        &self,
        snapshot: &dyn SnapshotAccess,
        agent_centers: &[Coord],
        buffers: &mut [&mut [f32]],
        masks: &mut [&mut [u8]],
    ) -> Result<Vec<ObsMetadata>, ObsError>;

    /// Total number of f32 elements in the output buffer.
    pub fn output_size(&self) -> usize;
}
```

**Validity mask:** Separate `&mut [u8]` buffer, same spatial shape as output tensor. 1 = valid, 0 = padding/out-of-bounds. For rectangular grids, all 1s. For hex regions or boundary-clipped observations, some 0s. Consumers (Python/RL) can multiply obs × mask or use as attention mask.

**Plan caching:** ObsPlan is cached by `(ObsSpec, WorldGenerationId)` key. If world generation changes (topology reconfiguration), cached plan is invalidated and `ObsError::PlanInvalidated` is returned. Caller must recompile.

### 6.10 Gymnasium Spaces Derivation

How `MurkEnv` derives Gymnasium-compatible `observation_space` and `action_space`.

```python
# Python-side derivation (in murk-python PyO3 wrapper)

class MurkEnv(gymnasium.Env):
    def __init__(self, config, obs_spec, n_actions, action_space=None, **kwargs):
        """
        Args:
            config: WorldConfig equivalent (space, fields, propagators, dt, seed)
            obs_spec: ObsSpec defining observation structure
            n_actions: Number of discrete actions (used if action_space not provided)
            action_space: Optional override. If None, defaults to Discrete(n_actions).
        """
        # Build world
        self._world = LockstepWorld(config)

        # Compile observation plan
        plan_result = ObsPlan.compile(obs_spec, self._world)

        # observation_space: derived from compiled plan output shape
        # If fields have bounds in FieldDef, use those for low/high.
        # Otherwise default to (-inf, +inf).
        low = np.full(plan_result.output_shape, -np.inf, dtype=np.float32)
        high = np.full(plan_result.output_shape, np.inf, dtype=np.float32)
        for entry, field_def in zip(obs_spec.entries, ...):
            if field_def.bounds is not None:
                # Apply per-field bounds to the region of the output tensor
                ...
        self.observation_space = gym.spaces.Box(
            low=low, high=high,
            shape=plan_result.output_shape,
            dtype=np.float32,
        )

        # action_space: user-provided (not derivable from world config).
        # Actions are domain-specific — the engine processes Commands, not actions.
        # The user's step() override maps actions → Commands.
        if action_space is not None:
            self.action_space = action_space
        else:
            self.action_space = gym.spaces.Discrete(n_actions)
```

**Key design decision:** `action_space` is NOT derived from `WorldConfig`. Actions are domain-specific (the mapping from RL action integers to `CommandPayload` variants is user logic, not engine logic). The engine only knows about Commands. Users provide `n_actions` or a custom `action_space` at construction.

### 6.11 Telemetry

Telemetry uses a lightweight struct-based approach on the hot path (not tracing/logging). This preserves the 14K steps/sec target while providing essential performance visibility.

```rust
/// Returned alongside StepResult from every tick.
/// Lockstep: populated synchronously during step_sync().
/// RealtimeAsync: snapshot of last completed tick's metrics.
pub struct StepMetrics {
    /// Total tick duration in microseconds (wall clock).
    pub total_us: u64,
    /// Time spent processing commands (drain + validate + sort) in microseconds.
    pub command_processing_us: u64,
    /// Per-propagator execution time: (propagator_name, microseconds).
    pub propagator_us: Vec<(String, u64)>,
    /// Time spent publishing snapshot (arena swap or ring buffer push) in microseconds.
    pub snapshot_publish_us: u64,
    /// Current arena memory usage in bytes.
    pub memory_bytes: usize,
}

/// Extended metrics for RealtimeAsync mode (in addition to StepMetrics fields).
pub struct RealtimeMetrics {
    pub base: StepMetrics,
    /// Actual tick duration vs. budget (tick_rate_hz).
    pub tick_duration_us: u64,
    /// Current ingress queue depth.
    pub queue_depth: usize,
    /// Age of the most recent published snapshot in ticks.
    pub snapshot_age_ticks: u64,
    /// Cumulative command rejections since last reset.
    pub rejection_count: u64,
    /// Current adaptive max_tick_skew value.
    pub current_max_skew: u64,
}
```

**FFI:** `murk_step_metrics(MurkWorld* world, MurkStepMetrics* out)` copies the last step's metrics into a caller-allocated C struct. Non-blocking (reads cached value).

**Python:** `env.last_step_metrics` property returns a dict: `{"total_us": 42, "command_processing_us": 5, "propagator_us": [("Diffusion", 20), ...], ...}`.

**Tracing:** Available under `#[cfg(feature = "tracing")]` for diagnostic/debug builds. Not enabled by default. Uses `tracing` crate spans for per-propagator, per-command, and per-observation detail. Intended for development, not production hot path.

### 6.12 Replay Log Format

Binary replay log format for determinism verification (WP-13). Designed for append-only sequential writes, not random access.

```
┌──────────────────────────────────────────────────────┐
│ HEADER                                                │
│  magic: [u8; 4] = b"MURK"                           │
│  format_version: u32 = 1                              │
│  build_metadata_len: u32                              │
│  build_metadata: [u8; build_metadata_len]            │
│    - toolchain: length-prefixed string                │
│    - target_triple: length-prefixed string            │
│    - murk_version: length-prefixed string             │
│    - compile_flags: length-prefixed string            │
│  init_descriptor:                                     │
│    seed: u64                                          │
│    config_hash: u64  (deterministic hash of WorldConfig) │
│    field_count: u32                                   │
│    cell_count: u64                                    │
│    space_descriptor: length-prefixed bytes            │
├──────────────────────────────────────────────────────┤
│ FRAME 0                                               │
│  tick_id: u64                                         │
│  command_count: u32                                   │
│  commands: [SerializedCommand; command_count]         │
│    each: payload_type: u8, payload_len: u32,          │
│           payload: [u8; payload_len],                  │
│           priority_class: u8, source_id: u64,         │
│           source_seq: u64                              │
│  snapshot_hash: u64  (for verification, not replay)   │
├──────────────────────────────────────────────────────┤
│ FRAME 1                                               │
│  ...                                                  │
└──────────────────────────────────────────────────────┘
```

**Design decisions:**
- **Why not FlatBuffers/protobuf:** Replay logs are append-only sequential streams. A simple header + frame format is faster to write (no serialization overhead), simpler to implement (~200 lines), and has no schema dependency.
- **Snapshot hash:** `u64` hash computed over logical field values via `SnapshotAccess::read_field()` for each field, then combined. NOT byte-level arena comparison (arena layout may differ between builds). Hash comparison catches divergence; on mismatch, per-field byte-exact comparison identifies the divergent field and cell.
- **Variable-length frames:** No alignment requirements. Frames are read sequentially — seeking requires scanning from header.
- **Versioning:** `format_version` field in header. Backward compatibility: reader supports all versions ≤ current. Forward compatibility: reader rejects unknown versions.

### 6.13 Stress Test Parameters

Concrete parameters for §23 mandatory stress tests #15-17 (WP-15).

#### Test #15: Death Spiral Resistance

- **Workload:** Reference profile (10K cells / 100×100 Square4, 5 fields, 3 propagators, 16 agents).
- **Injection:** At tick 0, start 16 concurrent ObsPlan executions (normal load). At tick 100, increase to 32 concurrent executions (2× overload). Run at 80% CPU utilization baseline.
- **Duration:** 600 ticks (10 seconds at 60Hz).
- **Pass criterion:** Tick overrun rate converges. Measured as: overrun_rate(tick 500..600) ≤ 1.5 × overrun_rate(tick 200..300). The system must shed load, not amplify it.
- **Fail criterion:** overrun_rate(tick 500..600) > 2.0 × overrun_rate(tick 200..300) — indicates positive feedback (death spiral).

#### Test #16: Mass Plan Invalidation Recovery

- **Setup:** 200 compiled ObsPlans with varying regions (mix of All, Disk r=5, Rect 10×10).
- **Trigger:** At tick 100, trigger topology change (simulated via space reconfiguration that changes WorldGenerationId). All 200 plans receive `PlanInvalidated`.
- **Measurement:** Observation throughput (obs/sec) averaged over 100ms windows.
- **Pass criterion:** Throughput reaches 50% of pre-invalidation baseline within 500ms (30 ticks at 60Hz). All plans successfully recompiled.
- **Fail criterion:** Throughput below 50% after 500ms, or any plan fails to recompile.

#### Test #17: Rejection Oscillation Stability

- **Setup:** 50 agents submitting commands at 2× tick rate (120Hz submission vs 60Hz ticks). Each agent sends 1 command per submission cycle.
- **Duration:** 600 ticks (10 seconds at 60Hz). Total expected commands: 50 × 120 × 10 = 60,000.
- **Measurement:** Rejection rate per 60-tick window (1-second windows). Collect 10 windows.
- **Pass criterion:** Coefficient of variation (CV = stddev / mean) across windows < 0.3. Adaptive backoff should stabilize rejection rate, not oscillate.
- **Fail criterion:** CV ≥ 0.3 — indicates the adaptive mechanism is oscillating.

#### CI Infrastructure

- **Correctness tests:** GitHub Actions with `ubuntu-latest`. Run on every PR and push to main.
- **Benchmark regression:** Self-hosted runner with pinned hardware spec (as defined in R-PERF-3 reference profile). Benchmarks run nightly, not per-PR (too noisy/slow for PR feedback loops).
- **Regression thresholds:** -5% warns (annotation on PR), -10% blocks merge. Measured against rolling 7-day baseline.

---

## 7. Consensus Record

The following decisions were made through cross-challenge review with unanimous agreement across all 4 SME agents:

| # | Decision | Rationale |
|---|----------|-----------|
| 1 | 9 crates (fold field→core, ingress→engine) | Minimum count preserving abstraction boundaries |
| 2 | FieldReader/FieldWriter traits in murk-core | Decouples propagator from arena; enables mocking |
| 3 | murk-propagator does NOT depend on murk-arena | Removes transitive dep from user propagators |
| 4 | Blanket IndexMap ban in deterministic crates | Eliminates audit cost; negligible perf for <100 entries |
| 5 | Overlay resolution via precomputed ReadResolutionPlan | Zero runtime conditionals; O(1) per propagator |
| 6 | Overlay table lives in murk-engine | Pipeline execution concern, not allocation or trait |
| 7 | M0 without ObsPlan (direct field reads) | Shortens critical path; ObsPlan enters at M1 |
| 8 | PyO3 for v1 (not ctypes) | ctypes cannot release GIL (violates R-FFI-5) |
| 9 | step_vec MUST v1 (upgraded from SHOULD) | 128 GIL cycles = 128μs overhead without batching |
| 10 | FlatBuffers deferred to M3 | Rust-native ObsSpec sufficient for M0-M2 |
| 11 | 6 milestones (M0-M5) with dual feature+quality gates | Feature and quality gates must both pass |
| 12 | Layered determinism testing | Unit → pipeline → mode → full replay |
| 13 | FieldHandle internal to murk-arena | Never crosses FFI; opaque slot handles at FFI |
| 14 | PerTick for most RL fields; Sparse for events only | Mutability = write pattern, not value change frequency |
| 15 | Sparse runtime warning on consecutive modification | Prevents silent performance degradation |
| 16 | WP-0 (test infra) as first work package | Foundation for all testing; CI lint enforcement |
| 17 | forbid(unsafe_code) in all crates except murk-ffi and murk-arena | Zero unsafe by default; murk-arena has bounded deny + raw.rs (Decision B) |
| 18 | deny(missing_docs) workspace-wide | 95% MUST requirements automatically verifiable |
| 19 | PettingZoo SHOULD v1, MUST v1.5 | Multi-agent engine support from v1; Python wrapper later |
| 20 | Three-propagator overlay test as WP-5b acceptance | Highest-risk correctness item; named criterion |
| 21 | Static terrain field in M0 | Validates generation-0 arena + reset survival |
| 22 | Hex v1 shapes: rectangles + disks only | O(1) interior check; wrap-around hex deferred to v1.5 |
| 23 | Double-buffer ping-pong (not triple) for Lockstep | &mut self prevents concurrent readers |
| 24 | Reward as propagator + Python override bridge | Deterministic base reward; Python can override |
| 25 | Static field sharing via Arc across VecEnv | 128 envs share one copy; invisible to FFI |

---

## Appendix A: HLD Requirement Traceability

| HLD Requirement | Work Package | Milestone | Verification |
|----------------|-------------|-----------|-------------|
| R-ARCH-1 | WP-1, WP-2 | M0 | Compile-time (no &mut WorldState) |
| R-ARCH-2 | WP-5a, WP-5b | M0 | Integration test |
| R-ARCH-3 | WP-6, WP-12 | M0, M4 | Compile-time + runtime assert |
| R-MODE-1 | WP-6, WP-12 | M0, M4 | Compile-time (distinct types) |
| R-MODE-2 | WP-5a | M0 | Integration test |
| R-SPACE-0..7 | WP-3 | M0 | Unit + property tests |
| R-SPACE-8 | WP-10b | M3 | Unit test (worked examples) |
| R-SPACE-9 | WP-10b | M3 | Property test (BFS=geodesic) |
| R-SPACE-10 | WP-10b | M3 | Unit test (worked examples) |
| R-SPACE-11..12 | WP-10b | M3 | Integration test |
| R-FIELD-1..4 | WP-1 | M0 | Unit + compile-time |
| R-PROP-1..5 | WP-4 | M0 | Unit + property tests |
| R-OBS-1..9 | WP-7, WP-11 | M1, M3 | Unit + integration + property |
| R-CMD-1..2 | WP-5a | M0 | Unit + property tests |
| R-DET-1..6 | WP-13, WP-15 | M4, M5 | CI replay + catalogue |
| R-FFI-1..5 | WP-8, WP-9 | M1 | Proptest + Miri + GIL test |
| R-SNAP-1..3 | WP-6 (Lockstep), WP-12 (RealtimeAsync) | M0, M4 | Unit + stress tests |
| R-ACT-1..2 | WP-5a | M0 | Unit + property tests |
| R-OPS-1 | WP-6 (basic), WP-12 (full) | M0, M4 | Integration test |
| R-MIG-1 | WP-3c (deferred v1.5) | v1.5 | Space trait compliance tests (lattice backends validate trait at M0) |
| R-PERF-1..3 | WP-14, WP-15 | M4, M5 | Criterion + custom bench |
