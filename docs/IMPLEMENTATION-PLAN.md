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
  ├── murk-obs         (core, arena, space)
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
| **murk-core** | `FieldId`, `SpaceId`, `TickId`, `WorldGenerationId`, `ParameterVersion`, `Command`, `Receipt`, error enums (§9.7), `FieldDef`, `FieldMutability`, `FieldSet`, `FieldReader`/`FieldWriter` traits | Semi-stable |
| **murk-arena** | `ReadArena` (Send+Sync), `WriteArena` (&mut), `FieldHandle`, segmented arena (64MB segments), double-buffer ping-pong, sparse slab, static gen-0 arena, `ScratchRegion`, implements `FieldReader`/`FieldWriter` | Internal |
| **murk-space** | `Space` trait, `LatticeSpace` (Line1D, Ring1D, Square4, Square8, Hex2D), `ProductSpace`, `VoxelOctreeSpace` wrapper, region queries, dual distance API | Semi-stable |
| **murk-propagator** | `Propagator` trait, `StepContext<R,W>`, `WriteMode`, pipeline validation (DAG, write conflicts, dt, Incremental budget), `PropagatorError` | Semi-stable |
| **murk-obs** | `ObsSpec`, `ObsPlan`, plan compilation, tensor fill, validity masks, plan classes (Simple/Standard), generation binding, `ObsError` | Semi-stable |
| **murk-engine** | `TickEngine`, `LockstepWorld`, `RealtimeAsyncWorld`, `World` trait (GAT), ingress queue, command ordering, TTL evaluation, receipt generation, tick atomicity, snapshot publication, overlay resolution tables | Internal |
| **murk-replay** | Replay log format, `ReplayWriter`, `ReplayReader`, build metadata header | Internal |
| **murk-ffi** | `extern "C"` functions, opaque handles (`MurkWorld`, `MurkSnapshot`, `MurkObsPlan`), `murk_abi_version()`, error code mapping, `murk_lockstep_step_vec()` | **Stable** (versioned) |
| **murk-python** | PyO3 module, `MurkEnv(gymnasium.Env)`, `MurkVecEnv(gymnasium.vector.VectorEnv)`, GIL release, NumPy buffer integration | **Stable** (versioned) |

### 1.4 Determinism Enforcement

- `#![forbid(unsafe_code)]` in all crates except murk-ffi
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
- **Deliverables:** Workspace `Cargo.toml`, crate stubs with `#![forbid(unsafe_code)]` and `#![deny(missing_docs)]`, CI pipeline (`cargo check` + `cargo test` + `cargo clippy` + `cargo miri` for murk-arena), `murk-test-utils` with `TestWorldBuilder` scaffold and `MockFieldReader`/`MockFieldWriter`, proptest configured, Criterion harness configured, `clippy.toml` per-crate with `disallowed_types`, PR checklist template.
- **Milestone:** Pre-M0

### WP-1: Core Types and Error Model
- **Description:** All shared types, IDs, error enums, field definitions, and FieldReader/FieldWriter traits.
- **Complexity:** S
- **Hard deps:** WP-0
- **HLD refs:** R-ARCH-1, R-ARCH-2, §2 (glossary), §9 (error codes), §13 (field model)
- **Deliverables:** `murk-core` crate with: `FieldId`, `SpaceId`, `TickId`, `WorldGenerationId`, `ParameterVersion`, `Command` (with `expires_after_tick`), `Receipt`, all error enums from §9.7, `FieldDef` (scalar/vector/categorical, units, bounds, boundary behavior), `FieldMutability` enum (Static/PerTick/Sparse), `FieldSet`, `FieldReader` trait, `FieldWriter` trait.
- **Acceptance:** All types compile. Error code enum is exhaustive per §9.7. Property test: `FieldSet` operations (union, intersection, difference) satisfy set algebra axioms.

### WP-2: Arena Allocator
- **Description:** Generational arena with segmented bump allocation, double-buffer ping-pong, sparse slab, static gen-0 arena.
- **Complexity:** L
- **Hard deps:** WP-1
- **HLD refs:** §5.1-5.6, R-FIELD-3
- **Deliverables:** `murk-arena` crate with: `ReadArena` (Send+Sync), `WriteArena` (&mut), `FieldHandle` (generation-scoped integer: generation u32, segment_index u16, offset u32, len u32), segmented arena (64MB segments, linked list), double-buffer ping-pong for Lockstep (§5.6), sparse slab with promotion on reclaim, static generation-0 arena, `ScratchRegion` bump allocator, implements `FieldReader` for `ReadArena` and `FieldWriter` for `WriteArena`.
- **Acceptance:** Property tests: arbitrary field counts × sizes × generation sequences round-trip correctly. Handle from gen N invalid after gen N+2 recycling. Static fields share allocation across generations (pointer equality). Sparse fields share until modified. Memory bound: after N Lockstep ticks, arena size ≤ 2× PerTick + 1× Static + Sparse slab. Miri clean. Zero `unsafe` blocks. Criterion micro-benchmark: allocate/publish/resolve cycle.
- **Risk:** HIGH — handle validation, epoch reclamation interaction (RealtimeAsync deferred to WP-10).

### WP-3: Space Trait and Simple Backends
- **Description:** Space abstraction + Line1D, Ring1D, Square4, Square8.
- **Complexity:** M
- **Hard deps:** WP-1
- **HLD refs:** R-SPACE-0 through R-SPACE-7
- **Sub-packages:**
  - **WP-3a:** Space trait definition + Line1D + Ring1D (S)
  - **WP-3b:** Square4 + Square8 (S)
- **Deliverables:** `murk-space` crate with: `Space` trait (ndim, cell_count, neighbours, distance, compile_region, iter_region, map_coord_to_tensor_index, canonical_ordering), `Coord` (SmallVec<[i32; 4]>), 4 lattice backends with boundary handling (clamp, wrap, absorb).
- **Acceptance:** Per-topology unit tests (neighbours, distance, iteration). Property tests: distance is a metric (d(a,a)=0, symmetry, triangle inequality). Neighbours symmetric (b in neighbours(a) iff a in neighbours(b)). Iteration deterministic (call twice → same sequence).

### WP-4: Propagator Pipeline
- **Description:** Propagator trait, StepContext with split-borrow semantics, pipeline validation, execution.
- **Complexity:** L
- **Hard deps:** WP-1, WP-3a (Space trait)
- **Soft deps:** WP-2 (for real arena testing; use mocks initially)
- **HLD refs:** R-PROP-1 through R-PROP-5, §15.2, §15.3
- **Deliverables:** `murk-propagator` crate with: `Propagator` trait (&self, Send + 'static), `StepContext<R: FieldReader, W: FieldWriter>` with `reads` (in-tick overlay view) and `reads_previous` (frozen tick-start view), `WriteMode` enum (Full/Incremental), pipeline validation (write-write conflict detection, DAG consistency with user-provided order, dt validation, Incremental budget estimation, all field refs exist), `PropagatorError`. 3 validation fixtures: `IdentityPropagator`, `ConstPropagator`, `FailingPropagator` in murk-test-utils.
- **Acceptance:** Pipeline validation rejects write-write conflicts. Rejects unresolvable field IDs. Rejects dt > min(max_dt). Property test: arbitrary propagator DAGs validate iff no conflicts and all refs exist. StepContext mock tests verify split-borrow semantics (reads sees staged, reads_previous doesn't). Criterion: pipeline validation time for 10 propagators.

### WP-5: TickEngine Core
- **Description:** Central tick execution: drain ingress → validate → run propagators → publish snapshot.
- **Complexity:** L
- **Hard deps:** WP-2, WP-4
- **Soft deps:** WP-3 (need at least Square4 for testing)
- **HLD refs:** R-ARCH-1, R-ARCH-3, §9.1 (tick atomicity)
- **Deliverables:** Core tick pipeline in `murk-engine`: command drain with `expires_after_tick` evaluation, deterministic ordering (priority_class → source_id/source_seq → arrival_seq, stable sort), receipt generation, **precomputed ReadResolutionPlan** (per-propagator read routing built at startup), propagator execution in dependency order with overlay resolution, tick atomicity (all-or-nothing via arena abandon on propagator failure), snapshot descriptor publication.
- **Critical sub-task:** Overlay resolution — the ReadResolutionPlan that routes `reads()` to base gen or staged writes from prior propagators. This is ~50-100 lines with the highest correctness criticality in the engine.
- **Acceptance:** Command ordering unit tests (priority, source disambiguation, arrival_seq tiebreak). TTL rejection test (expired → STALE). Tick atomicity test (propagator failure → no snapshot, state unchanged). **Three-propagator overlay visibility test** (5 cases: A writes X, B reads X via reads() sees A's value; B reads X via reads_previous() sees base gen; C reads X sees B's staged value if B wrote; etc.). Criterion: full tick cycle on reference profile.
- **Risk:** HIGH — overlay resolution is the single highest-risk correctness item.

### WP-6: Lockstep Mode
- **Description:** `LockstepWorld` callable struct with step_sync(), reset(), &mut self lifecycle.
- **Complexity:** M
- **Hard deps:** WP-5
- **Soft deps:** WP-7 (ObsPlan for obs, but M0 uses direct field reads)
- **HLD refs:** R-MODE-1, §7.1, §5.6, §8.1
- **Deliverables:** `LockstepWorld` in `murk-engine`: `step_sync(&mut self, commands) -> StepResult<&Snapshot>`, `reset(&mut self, seed) -> &Snapshot`, double-buffer ping-pong arena recycling, Send not Sync.
- **Acceptance:** 1000-step determinism (2 runs, same seed+commands → bit-exact snapshots at every tick). Memory bound assertion (RSS at tick 1000 ≈ tick 10). Reset reclaims both buffers. &mut self prevents snapshot aliasing (compile-time test).
- **M0 exit:** This WP + reference propagators = M0 complete.

### WP-7: ObsSpec/ObsPlan (Simple Plan Class)
- **Description:** Observation specification, compilation to executable plan, tensor fill, validity masks.
- **Complexity:** L
- **Hard deps:** WP-1, WP-2, WP-3
- **Soft deps:** WP-5 (need snapshots to test against)
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
- **Deliverables:** `murk-ffi` crate: opaque handles (slot-based with generation counter, not raw pointers), create/destroy lifecycle (double-destroy safe, use-after-destroy returns INVALID_HANDLE), `murk_abi_version()`, `murk_dt_range()`, caller-allocated buffers, `murk_lockstep_step()`, `murk_lockstep_reset()`, `murk_obsplan_compile()`, `murk_obsplan_execute()`, `murk_lockstep_step_vec()` (MUST v1), all error codes from §9.7.
- **Acceptance:** Handle lifecycle proptest (random create/step/observe/destroy sequences → no UB). Miri clean. Error code coverage (every error path returns defined code). double-destroy no-op. null-handle → error code. Criterion: FFI overhead per call.

### WP-9: Python Bindings
- **Description:** PyO3 wrapping C ABI with GIL release, NumPy integration, Gymnasium interface.
- **Complexity:** M
- **Hard deps:** WP-8
- **HLD refs:** R-FFI-2, R-FFI-5, §18
- **Deliverables:** `murk-python` crate (PyO3): `MurkEnv(gymnasium.Env)` class with step/reset/observation_space/action_space, `MurkVecEnv(gymnasium.vector.VectorEnv)` with auto-reset, GIL released via `py.allow_threads()` on all C ABI calls, NumPy buffer integration (caller-allocated, zero-copy pointer pass), context manager for handle lifecycle.
- **Acceptance:** GIL release verified (N concurrent Python threads make progress during step). Gymnasium compliance (step/reset contract). NumPy buffer contains correct observation data. PPO smoke test (100K steps, learning curve shows improvement — soft gate).

### WP-10: Hex2D and ProductSpace
- **Description:** Hex2D lattice backend + ProductSpace composition with dual distance API.
- **Complexity:** XL
- **Hard deps:** WP-3 (Space trait established)
- **Soft deps:** WP-7 (ObsPlan for hex tensor export testing)
- **HLD refs:** R-SPACE-4 through R-SPACE-12, §11.1, §12
- **Sub-packages:**
  - **WP-10a:** Hex2D (M) — axial coords, 6 neighbours, cube-distance, canonical ordering (r-then-q), hex tensor export (bounding box + validity mask, branch-free gather with precomputed index tables)
  - **WP-10b:** ProductSpace (L) — composition, per-component neighbours (R-SPACE-8), L1 graph-geodesic distance (R-SPACE-9), `metric_distance()` with configurable metrics, lexicographic iteration (R-SPACE-10), region queries as Cartesian products (R-SPACE-11), `valid_ratio` computation (product of per-component ratios)
- **Acceptance:** Hex2D: 6 neighbours in documented order, distance matches BFS, valid_ratio converges to 0.75 for large R. ProductSpace: worked examples from HLD §11.1 (Hex2D×Line1D distance, neighbours, iteration). Property tests: distance metric axioms, neighbour symmetry, BFS = geodesic. valid_ratio ≥ 0.35 for all v1 compositions. Hex2D×Hex2D ≈ 0.56 (warns, doesn't fail).
- **Risk:** MEDIUM — Hex2D tensor export mapping (branch-free gather with precomputed index tables) is deceptively hard. ProductSpace lexicographic iteration with mixed-dimensionality components needs careful nesting.

### WP-11: Foveation and Standard ObsPlan
- **Description:** Agent-centred observation windows, pooling, Standard plan class, FlatBuffers ObsSpec.
- **Complexity:** L
- **Hard deps:** WP-7, WP-10
- **HLD refs:** §16.1, §16.3
- **Deliverables:** Agent-relative regions with interior/boundary dispatch (O(1) check, ~90% interior path for radius < grid/4), pooling operations, Standard plan class, ObsPlan caching with rate-limited recompilation, FlatBuffers ObsSpec serialization for cross-language use.
- **Acceptance:** Interior/boundary dispatch produces identical results (functional equivalence test). Hex foveation correct (hex disk region + validity mask). FlatBuffers round-trip test.

### WP-12: RealtimeAsync Mode
- **Description:** RealtimeAsyncWorld with TickEngine thread, egress pool, epoch reclamation.
- **Complexity:** XL
- **Hard deps:** WP-5, WP-7
- **HLD refs:** R-MODE-1, §7.2, §8.2, §8.3, P-1, P-3
- **Deliverables:** `RealtimeAsyncWorld` in `murk-engine`: TickEngine on dedicated thread, snapshot ring buffer (count + byte-budget eviction), egress thread pool, epoch-based reclamation with stalled worker teardown (§8.3: max_epoch_hold, cancellation flag, cooperative check between region iterations), 60Hz wall-clock deadline, fallback snapshot selection, `ttl_ms → expires_after_tick` conversion at ingress, adaptive max_tick_skew, backpressure policy (R-MODE-2), telemetry (tick duration, queue depth, snapshot age).
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
- **Deliverables:** All 17 §23 mandatory tests (unit/property, integration, stress). CI benchmark pipeline with Criterion (15 micro-benchmarks) + custom system harness (throughput, memory growth). Regression detection: -5% warns, -10% blocks. Reference profile CI artifact. VoxelOctreeSpace integration (R-MIG-1). Graceful shutdown tests. NaN detection (configurable sentinel check). Determinism catalogue complete and reviewed.
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
        │     └─→ WP-10a (Hex2D) ─────────────────┤                    │
        │           └─→ WP-10b (ProductSpace) ────┤                    │
        │                                           │                    │
        └─→ WP-4 (Propagator Pipeline) ───────────┤                    │
              [uses mocks, no arena dep]            │                    │
                                                    │                    │
        ┌───────────────────────────────────────────┘                    │
        v                                                                │
  WP-5 (TickEngine Core) ──────────────────────────────────────────────┤
    ├─→ WP-6 (Lockstep Mode) ──────────────────────┐                   │
    │                                                │                   │
    ├─→ WP-7a (ObsPlan Simple) ────────────────────┤                   │
    │     └─→ WP-7b (Generation binding) ─────────┤                   │
    │                                                │                   │
    ├─→ WP-8 (C ABI / FFI) ───────────────────────┤                   │
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

### Critical Path (M0)

**WP-0 → WP-1 → WP-2 + WP-3a + WP-4 (parallel) → WP-5 → WP-6**

This is the shortest path to a working Lockstep engine with direct field reads.

### Critical Path (M1 — first Python training run)

**M0 path + WP-7a → WP-8 → WP-9**

### Parallelizable Tracks

| Track | Packages | Can Start After |
|-------|----------|----------------|
| **A: Engine core** | WP-2 → WP-5 → WP-6 | WP-1 |
| **B: Spaces** | WP-3a → WP-3b, WP-10a → WP-10b | WP-1 |
| **C: Observations** | WP-7a → WP-7b, WP-11 | WP-1 (schema), WP-5 (execution) |
| **D: Replay** | WP-13 | WP-1 |
| **E: Propagator** | WP-4 (with mocks) | WP-1 + WP-3a |

Tracks A, B, D, and E are independent until they converge at WP-5.

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
- [ ] Three-propagator overlay visibility test (5 cases)
- [ ] ReadResolutionPlan unit test (precomputed routing correct)
- [ ] Rollback negative test (propagator failure → state unchanged)
- [ ] Arena property tests pass
- [ ] No `unsafe` blocks
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

**WPs required:** M1 + `step_vec` in WP-8, rayon thread pool

**Quality gate:**
- [ ] ≥80% per-core throughput scaling (95% CI method)
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
- [ ] VoxelOctreeSpace integrated (R-MIG-1)
- [ ] Graceful shutdown tested
- [ ] Documentation complete (API docs, error reference, replay format)
- [ ] Cumulative: all prior tests still pass

---

## 5. Risk Register

| # | Risk | Severity | WP | Mitigation |
|---|------|----------|-----|-----------|
| 1 | **Overlay resolution correctness** | Critical | WP-5 | Precomputed ReadResolutionPlan (zero runtime conditionals). Three-propagator overlay test as WP-5 acceptance criterion. Determinism replay as safety net. |
| 2 | **Epoch-based reclamation** | Critical | WP-12 | Implement LAST (after Lockstep proven). Property-based tests with arbitrary worker timing. Stress tests §23 #15-17. Consider crossbeam-epoch as starting point. |
| 3 | **Arena allocator correctness** | High | WP-2 | Property-based tests (arbitrary field counts/sizes/generations). Miri for memory safety. Criterion for performance validation. Formal review before WP-5. |
| 4 | **StepContext split-borrow ergonomics** | High | WP-4 | FieldReader/FieldWriter traits enable mock testing. Pre-slicing based on declared reads/writes (validated at startup). Zero unsafe. |
| 5 | **ProductSpace complexity** | High | WP-10b | Start with 2-component (Hex2D×Line1D). Add 3-component only after 2 is solid. HLD §11.1 worked examples as TDD acceptance tests. |
| 6 | **Hex2D tensor export** | Medium | WP-10a | Branch-free gather with precomputed index tables. Limit v1 hex shapes to rectangles + disks (O(1) interior check). Wrap-around hex deferred to v1.5. |
| 7 | **FFI lifecycle safety** | Medium | WP-8 | Slot-based handles with generation counter (not raw pointers). Proptest with random operation sequences. Miri. |
| 8 | **Sparse field misclassification** | Medium | WP-2 | Runtime warning if Sparse field modified N consecutive ticks. DRL field classification table as guidance. |
| 9 | **PyO3 build complexity** | Low | WP-9 | Standard toolchain (maturin, abi3). Well-understood ecosystem (polars, ruff, pydantic-core use same approach). |

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
    // ...
}
pub struct Receipt { /* accepted, applied_tick_id, reason_code, ... */ }

// Error model (§9.7 — all 14 error codes)
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
    fn step(&self, ctx: &StepContext<impl FieldReader, impl FieldWriter>, dt: f64)
        -> Result<(), PropagatorError>;
}

pub struct StepContext<R: FieldReader, W: FieldWriter> {
    pub reads: R,           // current in-tick view (overlay)
    pub reads_prev: R,      // frozen tick-start view
    pub writes: W,          // staging arena
    pub scratch: ScratchRegion,
    pub space: Box<dyn Space>,
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
```

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
| 17 | forbid(unsafe_code) in all crates except murk-ffi | Zero unsafe design goal; CI enforced |
| 18 | deny(missing_docs) workspace-wide | 95% MUST requirements automatically verifiable |
| 19 | PettingZoo SHOULD v1, MUST v1.5 | Multi-agent engine support from v1; Python wrapper later |
| 20 | Three-propagator overlay test as WP-5 acceptance | Highest-risk correctness item; named criterion |
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
| R-ARCH-2 | WP-5 | M0 | Integration test |
| R-ARCH-3 | WP-6, WP-12 | M0, M4 | Compile-time + runtime assert |
| R-MODE-1 | WP-6, WP-12 | M0, M4 | Compile-time (distinct types) |
| R-MODE-2 | WP-5 | M0 | Integration test |
| R-SPACE-0..7 | WP-3 | M0 | Unit + property tests |
| R-SPACE-8 | WP-10b | M3 | Unit test (worked examples) |
| R-SPACE-9 | WP-10b | M3 | Property test (BFS=geodesic) |
| R-SPACE-10 | WP-10b | M3 | Unit test (worked examples) |
| R-SPACE-11..12 | WP-10b | M3 | Integration test |
| R-FIELD-1..4 | WP-1 | M0 | Unit + compile-time |
| R-PROP-1..5 | WP-4 | M0 | Unit + property tests |
| R-OBS-1..9 | WP-7, WP-11 | M1, M3 | Unit + integration + property |
| R-CMD-1..2 | WP-5 | M0 | Unit + property tests |
| R-DET-1..6 | WP-13, WP-15 | M4, M5 | CI replay + catalogue |
| R-FFI-1..5 | WP-8, WP-9 | M1 | Proptest + Miri + GIL test |
| R-PERF-1..3 | WP-14, WP-15 | M4, M5 | Criterion + custom bench |
