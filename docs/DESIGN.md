# Murk World Engine — Requirements, Architecture, and Design Specification (Merged v2.5)

This is a **stand-alone** merged document combining:

* the **Requirements & Design Questions Pack (Revised v2.3)**, and
* the **Expanded Architecture & Design Specification (Draft v2.4)**,

…into a single coherent v1+ specification with **binding design decisions**, **acceptance criteria**, and **answered design questions**.

---

## Change log

### v2.5 (this document)

* Merged requirements + architecture into one document; removed cross-references to prior packs.
* Clarified **dimensionality support envelope**:

  * architecturally supports **N ≥ 1** with no hard-coded max
  * **tested/supported N = 1..=5**
  * **N > 5 = allowed but untested/unsupported**
* Fixed “mixed shapes per dimension” into a concrete model: **ProductSpace of component spaces** (components may be multi-dimensional; e.g., Hex2D × Line1D).
* Chose **Determinism Tier B** for v1 and specified constraints.
* Standardised **Hex2D**:

  * axial coordinates canonical
  * deterministic iteration ordering
  * fixed-tensor export mapping + validity mask semantics
* Defined **snapshot retention** as latest pointer + ring buffer with bounded memory.
* Strengthened deterministic ordering rules for command intake/application and replay.
* Recommended **FlatBuffers** for ObsSpec schema (portable + low-overhead), with explicit plan invalidation via generation IDs.

---

## 0. Context and intent

### 0.1 Goal

Deliver **Murk** as a productised **world simulation engine** with:

* One authoritative world core.
* Multiple language bindings via a stable **C ABI**.
* Support for **discrete and continuous** spaces with **n-dimensionality** (N ≥ 1).
* Support for **alternative discrete lattices/topologies** (including **hex in 2D**).
* Support for **mixed topologies via composition** (e.g., Hex2D × Line1D layers).
* ML-native observation export (pre-vectorised, spec-driven) with fixed-shape tensors and masks.
* Operation in **RealtimeAsync** (best-effort) or **Lockstep** (tick-precise) modes via policy.

### 0.2 Scope boundary

**In scope**

* Single-host authoritative simulation.
* Discrete spaces (voxel/octree, lattices including hex).
* Observation export pipeline: ObsSpec → ObsPlan → tensor fill.
* Deterministic tick ordering and replay logging.

**Out of scope for v1**

* Aspatial mode.
* Distributed multi-host authoritative simulation.
* Hybrid coupling between heterogeneous spaces (research/experimental only, feature-flagged if present).

---

## 1. Normative language

This document uses:

* **MUST**: required for compliance.
* **SHOULD**: strongly recommended; may be deferred with justification.
* **COULD**: optional.
* **RESEARCH/EXPERIMENTAL**: not required; not a product guarantee.

---

## 2. Glossary

* **Tick**: discrete simulation step boundary. All authoritative mutation happens within TickEngine during ticks.
* **TickEngine**: the sole authoritative mutator and time owner.
* **Snapshot**: immutable world state view published at tick boundaries.
* **Ingress**: command intake interface (intents in).
* **Egress**: observation interface (snapshots/observations out).
* **ObsSpec**: declarative observation specification (portable schema).
* **ObsPlan**: compiled, bound, executable observation plan (hot-path primitive).
* **Space**: spatial domain backend (discrete or continuous) with topology/metric/region iteration rules.
* **Topology/Lattice**: pluggable discrete adjacency/metric/ordering definition.
* **ProductSpace**: composition of component spaces; overall coordinates are tuples.
* **WorldEvent**: ephemeral mutation intent (move/spawn/damage).
* **GlobalParameter**: persistent rules/config change (gravity, diffusion coefficients).
* **Determinism tier**: declared scope of repeatability (see §13).

---

## 3. Stakeholders and primary use-cases

* **Realtime**: games/tools/dashboards needing low-latency best-effort interaction.
* **Lockstep**: deterministic training/replay experiments with tick boundary correctness.
* **Simulation backends**: headless authoritative world pumps serving many egress clients.

---

## 4. System architecture (three-interface model)

### R-ARCH-1 Authoritative mutation invariant (MUST)

Only **TickEngine** may mutate authoritative world state. Everyone else reads **immutable snapshots**.

**Design decision (v1)**

* Single TickEngine thread owns `WorldState` exclusively.
* Ingress submits intents into a bounded queue.
* Egress reads immutable snapshots concurrently, without locks on the hot read path.

**Acceptance criteria**

* Public APIs prevent mutation via ingress/egress paths.
* Snapshot types are immutable and safe for concurrent reads.
* No publicly exposed shared-mutability world handle (no `Arc<Mutex<WorldState>>` style).

---

### R-ARCH-2 Three-interface product surface (MUST)

Expose:

#### WorldIngress (MUST)

* Accepts intents/commands to change world state or rules.
* Supports partial acceptance.
* Implements backpressure (bounded queue, TTL, drop/reject rules).

#### WorldEgress (MUST)

* Returns observations from the most appropriate snapshot.
* Supports pre-vectorised ObsSpec/ObsPlan outputs.
* Supports stale/incomplete responses with explicit metadata:

  * `tick_id`, `age_ms`, `coverage`, `validity_mask`.

#### TickEngine (MUST)

* Owns authoritative time progression and mutation.
* Executes `step(dt)` with deterministic ordering.
* Publishes immutable snapshots at tick boundaries.

**Acceptance criteria**

* TickEngine is the only code holding `&mut WorldState`.
* Egress does not mutate; it executes ObsPlans against snapshots.

---

### R-ARCH-3 Physical instantiation of authoritative core (MUST)

Architecture MUST specify and enforce physical ownership model:

* Single authoritative core must be concrete (thread/process model).
* Snapshot publication must be safe and low overhead.

**Design decision (v1)**

* **Single TickEngine thread per World instance**.
* Snapshot publication via **atomic swap** of a refcounted snapshot handle + optional ring retention.

**Acceptance criteria**

* Mechanical enforcement via type/module privacy.
* Snapshot publish overhead budget defined and measured (§14).

---

### 4.1 Reference architecture diagram

```
[Many producers]                 [Many consumers]
    |                                  ^
    v                                  |
 WorldIngress --(bounded queue)--> TickEngine --(publish)--> Latest Snapshot (atomic)
                                         \--(retain)------> Snapshot Ring Buffer (K)
                                                           ^
                                                           |
                                                    WorldEgress + ObsPlan
```

---

## 5. Observation–Action alignment (latency gap controls)

### R-ACT-1 Tick-referenced actions (MUST)

Ingress commands MUST be able to include tick reference metadata:

* `basis_tick_id` (snapshot tick the agent used)
* `intended_for_tick` OR `valid_after_tick`
* optional `max_tick_skew`

**Design decision (v1)**

* All command types support this metadata envelope.
* TickEngine resolves an `apply_tick_id` deterministically.

---

### R-ACT-2 Stale action policy (MUST)

TickEngine MUST define policy for stale actions:

* reject with reason, OR
* accept but reschedule, OR
* accept with degraded semantics (documented)

**Design decision (v1 defaults)**

* **RealtimeAsync**: reject if `basis_tick_id` too old (default), optional reschedule policy.
* **Lockstep**: tighter skew; missing the intended window defaults to rejection.

**Acceptance criteria**

* Receipts report `accepted`, `applied_tick_id`, and `reason_code` (`NONE` on success; rejection reason otherwise).

---

## 6. Runtime modes and behavioural contracts

### R-MODE-1 Supported runtime modes (MUST)

| Mode          | Ingress                                        | Egress                   | Use                           |
| ------------- | ---------------------------------------------- | ------------------------ | ----------------------------- |
| RealtimeAsync | best-effort; non-blocking submit               | latest suitable snapshot | live games/tools              |
| Lockstep      | blocks until applied at tick boundary (policy) | can block for exact tick | deterministic training/replay |

### R-MODE-2 Policy-driven blocking (MUST)

Blocking vs non-blocking is controlled by policy/config (not separate architecture).

### R-MODE-3 Overload/backpressure (MUST)

Ingress implements:

* bounded queue,
* TTL,
* deterministic drop/reject policy,
* metrics for overload events.

---

## 7. Spatial model (nD + mixed topologies via ProductSpace)

### R-SPACE-0 Dimensionality is first-class (MUST)

System MUST support spaces with dimensionality **N ≥ 1** and not hard-coded to a small fixed set.

**Interpretation of “unbounded”**

* Design must not require redesign to support higher N.
* Practical limits must be explicit per backend.

**Design decision (v1 support envelope)**

* Architecturally supports any N (no hard-coded maximum).
* **Supported/tested N = 1..=5.**
* **N > 5 is allowed but untested/unsupported** (may work; not guaranteed).

---

### R-SPACE-1 Space abstraction (MUST)

Define a minimal, stable space interface supporting:

* sampling fields over regions,
* integration into propagators,
* deterministic iteration/ordering guarantees where required.

**Design decision (stable per-space interface)**
Each Space backend MUST provide:

* region planning primitives:

  * `compile_region(region_spec) -> RegionPlan`
* deterministic iteration:

  * `iter_region(RegionPlan) -> Iterator<Coord>`
* discrete topology functions (for discrete spaces):

  * `neighbours(Coord) -> fixed ordered list`
  * `distance(a, b) -> scalar` (if defined)
* observation mapping hooks:

  * `map_coord_to_tensor_index(Coord, MappingPlan) -> idx|invalid`
  * `canonical_ordering_spec()`

---

### R-SPACE-2 Discrete lattice/topology is pluggable (MUST)

Discrete spaces MUST NOT assume square/cubic Cartesian grid semantics.

Each discrete space defines (directly or via topology component):

* coordinate representation,
* neighbour enumeration,
* distance metric/geodesics,
* region queries,
* deterministic cell ordering for observation export and replay.

---

### R-SPACE-3 Minimise Manhattan artefacts (MUST)

Provide at least one 2D discrete lattice more isotropic than square-4-neighbour.

**Concrete minimum (v1)**

* Include **Hex2D** lattice backend.
* Square grids remain supported but are not the only option.

---

### R-SPACE-4 Mixed shapes per dimension / product spaces (MUST)

Engine MUST support composing dimensions with different shapes/topologies into a single space.

**Binding design decision: ProductSpace of component spaces**

* Mixed topology is represented as a **ProductSpace**.
* Each component may be multi-dimensional (critical for Hex2D).
* Overall coordinate is a tuple across components.

Examples:

* **Hex2D × Line1D** (layered hex maps)
* **Ring1D × Square2D**
* **VoxelOctree3D × Ring1D**

**Acceptance criteria**

* A single ObsSpec can address regions/samplers in mixed spaces.
* Propagators operate via the space/topology interfaces (no hard-coded adjacency).

---

### R-SPACE-5 Required initial backends (MUST)

* Retain current voxel/octree system as **VoxelOctreeSpace v1**.
* Add at least one general discrete lattice space supporting non-square 2D topologies (hex).

**Design decision (v1 backends)**

* **VoxelOctreeSpace v1**
* **LatticeSpace v1** supporting:

  * Line1D
  * Ring1D
  * Square4 (2D)
  * Square8 (2D) (recommended for cheap isotropy improvement)
  * Hex2D
* **ProductSpace** to compose component spaces.

---

### R-SPACE-6 Planned backends (SHOULD / v1.5+)

* ContinuousGridSpace<N> (N-dimensional; tested N ≤ 5).
* GraphMeshSpace.

### R-SPACE-7 Hybrid coupling (RESEARCH / EXPERIMENTAL)

Hybrid coupling between heterogeneous spaces is experimental; if present it must be feature-flagged.

---

## 8. Hex2D: canonical representation and determinism

### 8.1 Coordinate representation

**Canonical:** axial coordinates `(q, r)`.

* Neighbours: 6 ordered direction offsets (documented order).
* Distance metric: implement via cube conversion internally:

  * `x=q, z=r, y=-x-z`
  * `dist = max(|dx|, |dy|, |dz|)`

### 8.2 Deterministic ordering

All region iteration MUST be deterministic.

**Canonical region ordering for Hex2D (v1)**

* primary sort by `r` ascending
* secondary sort by `q` ascending

This ordering is part of the determinism contract and must be treated as a compatibility boundary.

### 8.3 Fixed-shape tensor export mapping (hex → tensor)

Hex regions are non-rectangular. Export uses:

* a rectangular bounding box in a defined offset layout,
* a dense tensor for that box,
* a **validity mask** where `1 = valid cell`, `0 = padding/invalid`.

ObsPlan MAY additionally export an index map (tensor index → axial coord) for debugging/traceability.

---

## 9. Field model requirements

### R-FIELD-1 Typed first-class fields (MUST)

Support:

* Scalar
* Vector
* Categorical

Each field includes metadata:

* units (annotated; structure optional),
* bounds (optional),
* boundary behaviour: clamp/reflect/absorb/wrap,
* precision/storage policy (f32 minimum in v1; extensible).

### R-FIELD-2 Storage layout and bandwidth awareness (SHOULD)

* Prefer **SoA** layouts for numerically processed fields.
* Support chunking/tiling strategies (esp. large lattices and continuous grids).
* Track or estimate bytes read/written per tick and bytes generated per observation (§14).

---

## 10. Command model requirements (WorldEvents vs GlobalParameters)

### R-CMD-1 Command taxonomy (MUST)

Ingress commands categorised as:

* **WorldEvents**
* **GlobalParameters**

### R-CMD-2 Distinct semantics and logging (MUST)

* GlobalParameters are versioned and appear in snapshots (or attached config objects).
* Replay logs preserve:

  * original intent,
  * application result,
  * applied tick,
  * parameter-version transitions.

### 10.1 Deterministic command ordering (binding decision)

TickEngine drains and applies commands in deterministic order:

1. Resolve `apply_tick_id` for each command.
2. Group by `apply_tick_id`.
3. Sort within tick by:

   * `priority_class` (system > global > events; configurable but fixed)
   * then `(source_id, source_seq)` if both are provided
   * else TickEngine-assigned monotonic `arrival_seq` (assigned at ingress-admit time)

`arrival_seq` MUST come from a single world-local monotonic counter and MUST be unique per accepted command.

Lockstep deployments that require tight repeatability SHOULD provide `(source_id, source_seq)`; realtime deployments MAY rely on `arrival_seq`.

### 10.2 Receipts

Receipt fields (minimum):

* `accepted: bool`
* `applied_tick_id: u64 | null`
* `reason_code: enum`
* `basis_tick_id_echo: u64 | null`
* `parameter_version_after: u64 | null`

### 10.3 Replay ordering provenance (MUST)

Replay logs MUST carry sufficient ordering provenance so command application order is reconstructed exactly.

For each accepted command log record, include at minimum:

* resolved `apply_tick_id`
* resolved `priority_class`
* `source_id` and `source_seq` if present
* `arrival_seq` (always present)

Replayers MUST use recorded resolved ordering metadata and MUST NOT recompute ordering from wall-clock intake timing.

---

## 11. Propagators

### R-PROP-1 Pluggable propagators (MUST)

Propagators are modular operators in TickEngine’s per-tick pipeline with deterministic execution order.

### R-PROP-2 Declared read/write sets (MUST)

Each propagator MUST declare:

* fields/spaces it reads,
* fields/spaces it writes.

**Acceptance criteria**

* Engine validates pipeline legality at startup.
* No ambiguous write/write collisions without defined ordering.

### R-PROP-3 Deterministic execution guarantees (MUST)

* Execution order stable per tick.
* Internal parallelism must not change results within chosen determinism tier.

**v1 recommendation on parallelism**

* Allow deterministic data-parallel updates without reductions.
* Treat reductions (sum/mean pooling, accumulation) as single-threaded or deterministic-tree reductions with fixed partitioning.

---

## 12. Observation and ML integration

### R-OBS-1 ObsSpec contract (MUST)

ObsSpec defines:

* fields to sample,
* region selection primitives compatible with nD and mixed topologies,
* output shape contract,
* normalisation,
* masking,
* optional history.

### R-OBS-2 LOD/subsampling/pooling (MUST)

ObsSpec supports:

* pooling reducers (mean/max/min/sum where meaningful),
* foveation/shells,
* multi-resolution outputs,
* topology-aware neighbourhood pooling (hex-aware, etc.).

### R-OBS-3 ObsPlan compilation (MUST)

ObsSpec MUST compile into ObsPlan before hot-path execution:

* validate spec,
* resolve field IDs/offsets,
* precompute region iterators and index mappings,
* precompute pooling kernels,
* compute output shapes/strides.

**Acceptance criteria**

* Egress does not interpret ObsSpec schema per call; it executes an ObsPlan.

### R-OBS-4 Fixed-shape tensors + masks (MUST)

* Export fixed-shape tensors suitable for RL frameworks.
* Non-rectangular domains map deterministically with padding and validity masks; mapping is documented.

### R-OBS-5 Freshness/completeness metadata (MUST)

Return:

* `tick_id`
* `age_ms`
* `coverage`
* `validity_mask`

### R-OBS-6 ObsPlan validity and generation binding (MUST)

ObsPlan cache validity MUST be enforced with generation IDs.

* World configuration MUST expose monotonic generation IDs:

  * `world_config_generation_id`
  * `field_layout_generation_id`
  * `space_topology_generation_id`
* ObsPlan bind key MUST include these generation IDs.
* On mismatch, egress MUST fail plan execution with `PLAN_INVALIDATED`.
* Field IDs and Space IDs MUST be stable under an unchanged world definition.

---

## 13. Language bindings (C ABI + Python fast path)

### R-FFI-1 Stable, handle-based C ABI (MUST)

* Versioned, handle-based C ABI.
* Opaque handles (World, ObsPlan, etc.).
* Explicit create/destroy.
* Explicit error model.

### R-FFI-2 Caller-allocated buffers (MUST)

Primary tensor export path uses caller-owned buffers:

* Python allocates NumPy buffer,
* FFI fills it.

### R-FFI-3 Portable ObsSpec schema (MUST)

ObsSpec is serialisable and portable across bindings, with explicit versioning.

**Binding decision (v1 recommendation): FlatBuffers**

* low allocation, portable, versionable
* suitable for high-frequency cross-language use

---

## 14. Determinism, numeric policy, replay

### R-DET-1 Determinism tier must be explicitly chosen (MUST)

**Chosen for v1: Tier B**
Deterministic within:

* same build,
* same ISA/CPU family,
* fixed compiler/toolchain flags,
* same initial state + seed + applied command log.

### R-DET-2 Numeric strategy gate (MUST)

If Tier C is required later, architecture must specify:

* fixed-point strategy and scaling, OR
* software float / strict-math library strategy,
* plus compilation flags and forbidden optimisations.

**v1 Tier B constraints (recommended)**

* prohibit fast-math-style reassociation for authoritative code paths
* record build metadata in replay headers

### R-DET-3 Replay support (MUST)

Replay achievable from:

* initial snapshot/version (or deterministic init descriptor),
* seed,
* command log (including receipts and applied ticks),
* resolved command ordering provenance (`apply_tick_id`, priority class, source tuple if present, `arrival_seq`),
* determinism tier + numeric/config policy.

---

## 15. Snapshots and retention policy

### R-SNAP-1 Retention strategy (MUST)

Snapshots must be retained under a bounded memory policy.

**Binding decision (v1)**

* `latest` snapshot: atomic pointer swap to a refcounted snapshot handle.
* ring buffer of last **K** snapshots for lockstep exact tick reads.
* bounded by count and/or byte budget; evict oldest.

### R-SNAP-2 Lockstep exact-tick egress semantics (MUST)

* Egress MAY request exact `tick_id`.
* If present in ring: return immediately.
* If future tick: may block up to policy timeout.
* If timeout elapses before requested tick is published: return `TIMEOUT_WAITING_FOR_TICK` with `requested_tick_id`, `latest_tick_id`, and `waited_ms`.
* If too old/evicted: return `NOT_AVAILABLE` with `requested_tick_id` and `latest_tick_id`.

All exact-tick egress responses MUST include `status_code` and `tick_id` (or requested tick metadata when unavailable).

---

## 16. Performance and operational requirements

### R-PERF-1 Concrete performance targets (MUST)

Architect must define measurable targets for v1, including:

* tick rate under representative loads,
* snapshot publication overhead budget,
* obs generation throughput and tail latency,
* ingress latency and drop rates.

**Initial architectural budgets (v1 validation targets)**

* Tick loop (reference profile): target 60 Hz nominal (`dt=16.67 ms`) with `tick_duration_p99 <= 16.67 ms` and `tick_overrun_rate <= 1%`.
* Snapshot publish overhead: target `<= 3%` of tick time at p95 (`<= 5%` at p99) under representative load.
* Ingress submit latency (in-process): `p95 <= 1 ms`, `p99 <= 3 ms`.
* Ingress drops/TTL expiries: `0` under nominal load test; deterministic reason-coded drops under stress profile.
* ObsPlan execution: publish p50/p95/p99 per plan class; baseline plan class target `p99 <= 5 ms`.
* Obs generation throughput: baseline plan mix target `>= 200 obs/sec` on the same reference profile.

Reference profile (hardware + scenario + plan mix) MUST be published with benchmark results and kept stable for regression comparisons.

### R-PERF-2 Memory bandwidth metrics (MUST)

Performance reporting MUST include:

* bytes read/written per tick (or estimates),
* bytes generated per observation,
* cache-miss sensitive hotspots (profiling requirement).

### R-OPS-1 Telemetry (SHOULD)

Metrics/logging for:

* tick duration/jitter,
* queue depth/TTL expiries/drops,
* snapshot age distributions,
* obs generation timings per plan.

---

## 17. Migration and deliverables

### R-MIG-1 No rewrite cliff (MUST)

Retain current voxel/octree backend as v1 discrete backend and wrap via new space abstraction.

### v1 (MUST)

* Three-interface architecture.
* RealtimeAsync + Lockstep.
* VoxelOctreeSpace v1 integrated.
* Hex-capable discrete lattice backend (Hex2D).
* ProductSpace composition model.
* ObsSpec → ObsPlan compilation + fixed-shape tensor export + masks + metadata.
* Handle-based C ABI + Python fast path.
* Deterministic command ordering + replay log format.
* Tested/supported N = 1..=5.

### v1.5 (SHOULD)

* ContinuousGridSpace<N> + operator parity.
* Expanded ObsSpec history + richer pooling.

### v2 (COULD / EXPERIMENTAL)

* GraphMeshSpace maturity.
* Hybrid coupling (feature-flagged research).
* Wave/radiative operators if prioritised.

---

## 18. Updated design questions and binding answers

### A. Authoritative core instantiation

1. **Concrete authoritative model**
   **Answer:** single TickEngine thread per World; enforced by module privacy and only TickEngine owning `&mut WorldState`.

2. **Snapshot publication mechanism + overhead budget**
   **Answer:** atomic swap of refcounted snapshot handle + ring buffer retention; budget <1–3% tick time (measure and enforce).

---

### B. nD spaces and mixed shapes per dimension

1. **Dimensionality: compile-time vs runtime? tested N?**
   **Answer:** runtime-N architecture; specialised internal fast paths permitted. **Tested/supported N = 1..=5**; N>5 allowed but untested.

2. **Different shapes per level representation**
   **Answer:** **ProductSpace of component spaces**, components may be multi-dimensional (Hex2D is a 2D component).

3. **Coordinate transforms and region queries in mixed spaces**
   **Answer:** product coordinate is tuple; region queries compile per component and combine; complex region planning remains space-specific but under a stable interface.

---

### C. Discrete lattice/topology specifics

1. **Discrete lattices in v1**
   **Answer:** Line1D, Ring1D, Square4, Square8 (recommended), Hex2D + ProductSpace composition.

2. **Canonical hex representation and deterministic ordering**
   **Answer:** axial `(q,r)`; deterministic iteration order `r` then `q`; fixed export mapping uses bbox+mask.

3. **Neighbourhoods and distance metrics surfaced**
   **Answer:** topology exposes ordered neighbours + distance + deterministic region iteration; propagators and ObsPlan use these, never hard-coded adjacency.

---

### D. Observation–Action alignment policy

1. **Semantics of basis/intended/stale handling per mode**
   **Answer:**

* RealtimeAsync: apply next tick unless scheduled; reject stale by default.
* Lockstep: apply at specified tick; reject missed windows unless policy reschedules.

2. **Default max_tick_skew and rejection representation**
   **Answer:** policy defaults (e.g., realtime 1–3 ticks; lockstep 0–1 tick); structured receipts with `reason_code` enums.

---

### E. ObsSpec → ObsPlan

1. **Schema format + versioning**
   **Answer:** FlatBuffers recommended for v1; explicit schema version + world/config generation IDs.

2. **ObsPlan caching and invalidation**
   **Answer:** ObsPlan caches resolved field offsets, iterators, index maps, pooling kernels, output descriptors; invalidated via required generation mismatch (`PLAN_INVALIDATED`).

3. **Hex domain mapping to fixed tensors**
    **Answer:** bbox in offset layout + validity mask; optional index map.

---

### F. Propagators and determinism

1. **Strict list or dependency graph?**
    **Answer:** strict ordered list in v1.

2. **Read/write sets declared/validated?**
    **Answer:** mandatory declarations; engine validates pipeline legality at startup.

3. **Permitted parallelism**
    **Answer:** only where determinism preserved under Tier B; avoid nondeterministic reductions or enforce deterministic reduction topology.

---

### G. Determinism tier and numeric policy

1. **Tier required?**
    **Answer:** Tier B for v1.

2. **If Tier C later: numeric strategy?**
    **Answer:** fixed-point or soft-float strategy, applied at least to authoritative propagators and reducers; strict flags and forbidding fast-math.

3. **Build flags/toolchain constraints**
    **Answer:** fixed toolchain + flags recorded in replay header; strict settings for authoritative paths.

---

### H. Snapshots and retention

1. **Retention strategy**
    **Answer:** latest + ring buffer; bounded by count/bytes; eviction oldest; refcount safety.

2. **Exact tick_id request semantics**
    **Answer:** served if in ring; may block for future tick under policy; return not-available if evicted/too old.

---

### I. C ABI and bindings

1. **FFI error model**
    **Answer:** status codes + last-error string (handle- or thread-local).

2. **Caller-allocated buffer filling**
    **Answer:** explicit output descriptors (dtype, shape, strides, alignment). Mask format standardised.

3. **Minimum Python export path**
    **Answer:** NumPy pointer fill; DLPack optional later.

---

### J. Performance baselines

1. **Reference hardware/config**
    **Answer:** must be explicitly defined for budgets; used for regression testing.

2. **Target bytes/tick and bytes/obs envelopes**
   **Answer:** must be defined per representative scenarios; engine reports bytes moved and tail latencies, with concrete v1 baseline targets defined in §16.

---

## 19. Additional binding policies (normative v1 additions)

### 19.1 Stable IDs policy (MUST)

To make replay and plan caching sane:

* Field IDs and Space IDs are stable under the same world definition.
* World configuration exposes monotonic generation IDs:

  * `world_config_generation_id`
  * `field_layout_generation_id`
  * `space_topology_generation_id`
* Plans bind to generations; mismatch triggers `PLAN_INVALIDATED`.

### 19.2 Validity mask semantics (required for interoperability)

* Mask dtype: `uint8` (recommended) or documented alternative.
* `1 = valid`, `0 = invalid/padding`.
* Mask shape matches exported tensor spatial footprint.
