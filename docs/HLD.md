# Murk World Engine — High-Level Design (v3.1)

This is the **authoritative** design document for the Murk World Engine. It incorporates:

* the Requirements & Architecture Specification (v2.5),
* the Architectural Design Review (2026-02-09), and
* the Domain Expert Review (2026-02-09) — 17 unanimous design decisions from systems engineering, deep reinforcement learning, and simulation architecture experts.

**Status:** Approved for implementation (pending mandatory revisions noted in section 24).

---

## Table of Contents

- [0. Context and Intent](#0-context-and-intent)
- [1. Normative Language](#1-normative-language)
- [2. Glossary](#2-glossary)
- [3. Normative Principles](#3-normative-principles)
- [4. Stakeholders and Primary Use Cases](#4-stakeholders-and-primary-use-cases)
- [5. Foundational Design Decision: Arena-Based Generational Allocation](#5-foundational-design-decision-arena-based-generational-allocation)
- [6. System Architecture (Three-Interface Model)](#6-system-architecture-three-interface-model)
- [7. Runtime Modes and Behavioral Contracts](#7-runtime-modes-and-behavioral-contracts)
- [8. Physical Threading Model](#8-physical-threading-model)
- [9. Error Model and Recovery](#9-error-model-and-recovery)
- [10. Observation-Action Alignment](#10-observation-action-alignment)
- [11. Spatial Model](#11-spatial-model)
- [12. Hex2D: Canonical Representation and Determinism](#12-hex2d-canonical-representation-and-determinism)
- [13. Field Model Requirements](#13-field-model-requirements)
- [14. Command Model Requirements](#14-command-model-requirements)
- [15. Propagators](#15-propagators)
- [16. Observation and ML Integration](#16-observation-and-ml-integration)
- [17. Snapshots and Retention Policy](#17-snapshots-and-retention-policy)
- [18. Language Bindings (C ABI + Python Fast Path)](#18-language-bindings-c-abi--python-fast-path)
- [19. Determinism, Numeric Policy, Replay](#19-determinism-numeric-policy-replay)
- [20. Performance and Operational Requirements](#20-performance-and-operational-requirements)
- [21. Stable IDs and Generation Policy](#21-stable-ids-and-generation-policy)
- [22. Migration and Deliverables](#22-migration-and-deliverables)
- [23. Mandatory v1 Test Set](#23-mandatory-v1-test-set)
- [24. Remaining Risks and Open Items](#24-remaining-risks-and-open-items)
- [Appendix A: Review Traceability](#appendix-a-review-traceability)

---

## Change Log

### v3.0 (this document)

* Consolidated DESIGN.md v2.5, architectural review, and domain expert review into a single authoritative HLD.
* Established **arena-based generational allocation** as the foundational ownership model (replaces CoW recommendation).
* Established **mode duality** as architectural: Lockstep = callable struct, RealtimeAsync = autonomous thread.
* Added concrete **Propagator trait** with `&self`, split-borrow `StepContext`, and `WriteMode`.
* Added **Error Model and Recovery** section (tick atomicity, all-or-nothing rollback).
* Added **Physical Threading Model** (mode-specific egress strategy).
* Added **Normative Principles** (Egress Always Returns, Tick-Expressible Time, Asymmetric Mode Dampening).
* Updated snapshot retention to arena-based model with epoch-based reclamation.
* Updated performance budgets to mode-specific targets with MuJoCo comparison.
* Added tiered ObsPlan targets replacing the single 200 obs/sec figure.
* Added `FieldMutability` (Static/PerTick/Sparse) and memory optimization analysis.
* Added plan-to-snapshot generation matching for ObsPlan invalidation.
* Added new requirements: R-FIELD-3, R-PROP-4, R-PROP-5, R-OBS-7, R-OBS-8, R-FFI-4, R-FFI-5, R-MODE-4.

### v3.1

* **Integrated Design Decisions v3.1** (5 decisions from 4-expert panel):
  * **Decision B:** Bounded unsafe in `murk-arena/raw.rs` with phased MaybeUninit migration and FullWriteGuard (§5.3).
  * **Decision E:** Graceful shutdown protocol — Lockstep `Drop`, RealtimeAsync 4-state drain-then-join ≤300ms (§9.7, §24).
  * **Decision J:** No re-enqueue on rollback; `tick_disabled` after 3 consecutive failures (§9.1, §9.7).
  * **Decision M:** `&dyn Space` with `Space: Any + Send + 'static` and `downcast_ref()` (referenced in Implementation Plan).
  * **Decision N:** `SnapshotAccess` trait in murk-core; murk-obs decoupled from murk-arena (referenced in Implementation Plan).
* Added `MURK_ERROR_SHUTTING_DOWN` and `MURK_ERROR_TICK_DISABLED` to §9.7 error code table.
* Updated I-6 (graceful shutdown) status from "Not addressed" to "Resolved" in §24.

### v3.0.1

* **Resolved CR-NEW-1:** ProductSpace default distance changed from L∞ to L1 (graph geodesic) to match per-component adjacency. Added dual distance API.
* **Resolved CR-NEW-2:** TTL made tick-based in both modes (`expires_after_tick`). Wall-clock TTL is ingress convenience only.
* **Added §5.6:** Lockstep arena recycling via double-buffer ping-pong with Sparse field promotion.
* **Added §8.3:** Epoch reclamation safety with stalled worker teardown.
* **Added §15.2:** StepContext read semantics (current in-tick view vs frozen tick-start view).
* **Clarified §21.1:** world_generation_id bumps only on plan-relevant changes; GlobalParameters tracked via separate `parameter_version`.
* **Corrected §12.3:** Hex valid_ratio asymptote from 0.83 to 0.75.
* **Scoped §19.1:** Determinism CI tests target Lockstep mode only.
* **Unified snapshot representation:** Handles (generation-scoped integers), not pointers.

### v2.5

* Merged requirements + architecture into one document.
* Clarified dimensionality support envelope (tested N = 1..=5).
* Fixed "mixed shapes per dimension" into ProductSpace of component spaces.
* Chose Determinism Tier B for v1.
* Standardised Hex2D (axial coordinates, deterministic ordering, tensor export mapping).
* Defined snapshot retention as latest pointer + ring buffer with bounded memory.
* Recommended FlatBuffers for ObsSpec schema.

---

## 0. Context and Intent

### 0.0 How to Read This Document

This is a 24-section design specification consolidating three prior documents. For efficient navigation:

- **Normative requirements** use MUST/SHOULD/COULD (defined in section 1). Requirements are labelled `R-XXX-N` throughout; search for `R-` to find all.
- **For implementation order**: start with section 5 (arena model), section 6 (three interfaces), section 7 (mode duality), section 15 (propagators).
- **For RL integration**: focus on sections 7.1 (Lockstep), 16 (observation export), 18 (C ABI), 20 (performance budgets).
- **For review traceability**: Appendix A maps all architectural review findings and domain expert decisions to their resolution sections.
- **Section 24** lists remaining open items that must be resolved before or during implementation.
- **Rust code snippets** throughout are illustrative of type-level contracts, not final API signatures.

### 0.1 Goal

Deliver **Murk** as a productised **world simulation engine** with:

* One authoritative world core.
* Multiple language bindings via a stable **C ABI**.
* Support for **discrete and continuous** spaces with **n-dimensionality** (N >= 1).
* Support for **alternative discrete lattices/topologies** (including **hex in 2D**).
* Support for **mixed topologies via composition** (e.g., Hex2D x Line1D layers).
* ML-native observation export (pre-vectorised, spec-driven) with fixed-shape tensors and masks.
* Operation in **RealtimeAsync** (best-effort) or **Lockstep** (tick-precise) modes, architecturally distinct but sharing a common propagator core.

### 0.2 Scope Boundary

**In scope**

* Single-host authoritative simulation.
* Discrete spaces (voxel/octree, lattices including hex).
* Observation export pipeline: ObsSpec -> ObsPlan -> tensor fill.
* Deterministic tick ordering and replay logging.

**Out of scope for v1**

* Aspatial mode.
* Distributed multi-host authoritative simulation.
* Hybrid coupling between heterogeneous spaces (research/experimental only, feature-flagged if present).

---

## 1. Normative Language

This document uses:

* **MUST**: required for compliance.
* **SHOULD**: strongly recommended; may be deferred with justification.
* **COULD**: optional.
* **RESEARCH/EXPERIMENTAL**: not required; not a product guarantee.

---

## 2. Glossary

* **Tick**: discrete simulation step boundary. All authoritative mutation happens within TickEngine during ticks.
* **TickEngine**: the sole authoritative mutator and time owner.
* **Snapshot**: immutable world state view published at tick boundaries. A lightweight descriptor mapping `FieldId -> FieldHandle` into a `ReadArena`. Handles are generation-scoped integers; `ReadArena::resolve()` provides `&[f32]` slice access.
* **ReadArena**: published, immutable arena generation. `Send + Sync`, safe for concurrent egress reads.
* **WriteArena**: staging arena for current tick. Exclusively owned by TickEngine via `&mut` access.
* **Ingress**: command intake interface (intents in).
* **Egress**: observation interface (snapshots/observations out).
* **ObsSpec**: declarative observation specification (portable schema).
* **ObsPlan**: compiled, bound, executable observation plan (hot-path primitive).
* **Space**: spatial domain backend (discrete or continuous) with topology/metric/region iteration rules.
* **Topology/Lattice**: pluggable discrete adjacency/metric/ordering definition.
* **ProductSpace**: composition of component spaces; overall coordinates are tuples.
* **Propagator**: stateless operator in the per-tick pipeline. Reads fields, writes fields, declared via trait.
* **StepContext**: split-borrow context providing read access to committed state and write access to staging arena.
* **WorldEvent**: ephemeral mutation intent (move/spawn/damage).
* **GlobalParameter**: persistent rules/config change (gravity, diffusion coefficients).
* **FieldMutability**: classification of field update frequency (Static/PerTick/Sparse) for arena optimization.
* **Determinism tier**: declared scope of repeatability (see section 19).

---

## 3. Normative Principles

These principles are binding on all subsystems. They emerged from cross-disciplinary review and address systemic failure modes that no single-subsystem requirement can prevent.

### P-1. Egress Always Returns (MUST, RealtimeAsync)

> **WorldEgress MUST always return a response within a bounded time when a snapshot of any generation is available. Responses MAY indicate staleness, degraded coverage, or plan invalidation via metadata, but MUST NOT block indefinitely or return no data.**

This principle applies to **RealtimeAsync mode only**. Lockstep mode has no asynchronous egress — observations are filled inline from the returned snapshot (see section 7.1).

This principle closes three RealtimeAsync failure modes simultaneously:
- **ObsPlan invalidation blackout** — stale-but-consistent plans serve data with metadata.
- **Observation starvation** — agents always get *some* data during system stress.
- **Egress blocking spiral** — bounded return time prevents cascading delays.

### P-2. Tick-Expressible Time References (MUST)

> **All engine-internal time references that affect state transitions MUST be expressible in tick-count.**

**TTL specifically:**
- All commands carry `expires_after_tick: u64` as the authoritative TTL.
- **Lockstep**: `expires_after_tick` set directly by submitter. Wall-clock time is forbidden in deterministic paths.
- **RealtimeAsync**: Ingress MAY accept `ttl_ms` as a convenience and convert to `expires_after_tick` at ingress-admit time: `expires_after_tick = current_tick_id + ceil(ttl_ms / configured_ms_per_tick)`. The configured tick period (e.g., 16.67ms for 60Hz) is used, not measured tick durations. The tick-based value is authoritative.
- TickEngine evaluates `expires_after_tick` during command drain. Expired commands are rejected with `STALE` reason code.
- Replay logs record `expires_after_tick`, not wall-clock TTL.
- Display metadata (`age_ms`) remains wall-clock (does not affect state transitions).

Covers: TTL, `age_ticks`, lockstep timeouts. Prevents replay-divergence bugs caused by wall-clock time in deterministic paths.

### P-3. Asymmetric Mode Dampening

Staleness and overload require different dampening per mode:
- **RealtimeAsync**: Egress worker threads + adaptive `max_tick_skew` with exponential backoff.
- **Lockstep**: Synchronous observation delivery at tick boundary (eliminates the staleness loop entirely).

A uniform dampening mechanism is incorrect. The modes have fundamentally different dynamics.

---

## 4. Stakeholders and Primary Use Cases

* **Realtime (RealtimeAsync mode)**: games/tools/dashboards needing low-latency best-effort interaction. TickEngine on dedicated thread, egress thread pool, 60Hz wall-clock deadline.
* **Lockstep mode**: deterministic training/replay experiments with tick boundary correctness. Callable struct, synchronous step, throughput-maximized.
* **Simulation backends**: headless authoritative world pumps serving many egress clients.

---

## 5. Foundational Design Decision: Arena-Based Generational Allocation

This is the most load-bearing architectural decision. It was identified by the architectural review as gating four downstream properties, and resolved by the domain expert panel with a specific mechanism.

### 5.1 How It Works

1. Each field is stored as a contiguous `[f32]` allocation in a generational arena.
2. At tick start, propagators write to **fresh allocations** in the new generation (no copies).
3. Unmodified fields share their allocation across generations (zero-cost structural sharing).
4. Snapshot publication = swap a ~1KB descriptor of field handles. Cost: **<2us** (target, 250x under the 500us budget). The descriptor maps `FieldId -> FieldHandle`; `ReadArena::resolve(handle)` provides `&[f32]` access.
5. Old generations remain readable until all snapshot references are released.

### 5.2 Comparison to Traditional CoW

| Property | Traditional CoW | Arena-Generational |
|----------|----------------|-------------------|
| Copy cost | Fault-driven, unpredictable | Zero (allocate fresh, write directly) |
| Snapshot publication | Clone or CoW fork | Atomic descriptor swap, <2us |
| Rollback | Undo log or full checkpoint | Free (abandon new generation) |
| `unsafe` required | Usually (page-level CoW) | Bounded: ≤5 audited functions in `murk-arena/src/raw.rs` (see Design Decisions v3.1, Decision B) |
| Memory predictability | Fault-driven = unpredictable | Bump allocation = predictable |

### 5.3 Rust Type-Level Properties

- `ReadArena` (published, immutable): `Send + Sync`, safe for concurrent egress reads.
- `WriteArena` (staging, exclusive to TickEngine): `&mut` access, no aliasing.
- **Bounded unsafe:** `#![deny(unsafe_code)]` crate-wide with per-function `#[allow(unsafe_code)]` in `crates/murk-arena/src/raw.rs` only (≤5 functions: `alloc_uninit`, `assume_init`, segment pointer math, bump-pointer reset). Each function has `// SAFETY:` comment and Miri coverage. Public API is 100% safe. Phase 1 (WP-2–WP-5): `Vec<f32>` zero-init. Phase 2 (after WP-4 delivers FullWriteGuard): `MaybeUninit<f32>`. See Design Decisions v3.1, Decision B.
- Snapshot descriptors contain `FieldHandle` values (generation-scoped integers), not raw pointers. `ReadArena::resolve(handle) -> &[f32]` provides the actual slice access. This indirection:
  - Makes FFI safe (handles can be validated; pointers cannot).
  - Enables generation invalidation (handle from gen N is invalid on gen N+2's arena after recycling).
  - Keeps the descriptor small (~1KB for 100 fields: `FieldId -> FieldHandle` map).
- Field access requires `&FieldArena` — borrow checker enforces arena liveness.
- Segmented arena (linked list of 64MB segments) ensures no reallocation.

### 5.4 Dependency Graph (All Four Downstream Properties Satisfied)

```
Arena-Based Generational Allocation
  |-- Snapshot publish overhead: <2us (exceeds 3% budget by 250x)
  |-- Tick rollback: free (abandon staging generation)
  |-- Concurrent egress: ReadArena is Send + Sync
  +-- v1.5 parallel propagators: disjoint field writes, no contention
```

### 5.5 Field Mutability Optimization (R-FIELD-3)

Fields are classified by update frequency to minimise arena allocation:

```
Static    -> Generation 0 forever, shared across snapshots and vectorized envs
PerTick   -> Arena-managed, new allocation each tick if modified
Sparse    -> Arena-managed, new allocation only when modified (rare)
```

For vectorized RL (128 envs x 2MB mutable + 8MB shared static): **264MB** vs 1.28GB without sharing.

**R-FIELD-3 FieldMutability (MUST)**

Each field MUST declare its mutability class (`Static`, `PerTick`, `Sparse`). The arena MUST use this classification to optimise allocation. `Static` fields MUST share a single allocation across all snapshots and vectorized environment instances.

### 5.6 Lockstep Arena Recycling (MUST)

In Lockstep mode, arena memory MUST be bounded regardless of episode length.

**Mechanism: double-buffer ping-pong.**

Two arena buffers, A and B, alternate roles each tick:

| Tick N | Read from | Write to | After publish |
|--------|-----------|----------|---------------|
| Even   | A (gen N-1) | B (gen N staging) | B becomes readable; A is reclaimable |
| Odd    | B (gen N-1) | A (gen N staging) | A becomes readable; B is reclaimable |

- `&mut self` on `step_sync()` guarantees the caller has released any `&Snapshot` borrows before the next step. The borrow checker enforces this at compile time.
- "Reclaim" = reset bump pointer to zero. O(1), no deallocation.
- `reset()` reclaims BOTH buffers (bump pointer reset on both). O(1).
- Static fields (FieldMutability::Static) live in a separate generation-0 arena that is never reclaimed (shared across all generations).

**Sparse field storage:** Sparse fields MUST NOT reside in the ping-pong buffers. Instead, Sparse fields are stored in a dedicated long-lived arena slab. When a Sparse field is modified, a new allocation is made in the Sparse slab (not in the PerTick ping-pong buffer). The old allocation is freed when no snapshot references it (trivially immediate in Lockstep due to `&mut self`). This avoids repeated promotion on buffer reclaim and preserves zero-copy semantics for unmodified Sparse fields.

**Memory bound:** At any point, Lockstep uses at most 2x the per-generation PerTick field footprint, plus 1x the Static field footprint, plus the Sparse slab. For the reference profile (10K cells, 5 fields): ~4MB PerTick + 2MB Static + <1MB Sparse = **<7MB total**, regardless of episode length.

**Triple-buffering is NOT needed** because Lockstep has no concurrent readers — the caller owns the only snapshot reference, and `&mut self` prevents aliasing.

---

## 6. System Architecture (Three-Interface Model)

### R-ARCH-1 Authoritative Mutation Invariant (MUST)

Only **TickEngine** may mutate authoritative world state. Everyone else reads **immutable snapshots** via `ReadArena`.

**Design decision (v1)**

* Single TickEngine owns `WorldState` exclusively.
* Ingress submits intents into a bounded queue.
* Egress reads immutable snapshots concurrently, without locks on the hot read path.

**Acceptance criteria**

* Public APIs prevent mutation via ingress/egress paths. Verification: static analysis of public API surface confirms no `&mut WorldState` exposure.
* Snapshot types are immutable (`ReadArena` is `Send + Sync`). Verification: Rust type system enforces at compile time.
* No publicly exposed shared-mutability world handle (no `Arc<Mutex<WorldState>>` style). Verification: code review + `grep` for `Mutex<WorldState>`.

---

### R-ARCH-2 Three-Interface Product Surface (MUST)

Expose:

#### WorldIngress (MUST)

* Accepts intents/commands to change world state or rules.
* Supports partial acceptance (batch-level: accept/reject per command in batch, receipt per command).
* Implements backpressure (bounded queue, TTL in tick-count per P-2, drop/reject rules).

#### WorldEgress (MUST)

* Returns observations from the most appropriate snapshot (P-1: always returns).
* Supports pre-vectorised ObsSpec/ObsPlan outputs.
* Supports stale/incomplete responses with explicit metadata:
  * `tick_id`, `age_ticks`, `coverage`, `validity_mask`, `world_generation_id`, `parameter_version`.

#### TickEngine (MUST)

* Owns authoritative time progression and mutation.
* Executes `step(dt)` with deterministic ordering.
* Publishes immutable snapshots at tick boundaries via arena generation swap.

**Acceptance criteria**

* TickEngine is the only code holding `&mut WorldState`. Verification: module privacy analysis.
* Egress does not mutate; it executes ObsPlans against `ReadArena` snapshots. Verification: `ReadArena` provides only `&self` methods.

---

### R-ARCH-3 Physical Instantiation of Authoritative Core (MUST)

Architecture MUST specify and enforce physical ownership model:

* Single authoritative core must be concrete (thread/process model).
* Snapshot publication must be safe and low overhead.

**Design decision (v1)**

* Mode-dependent instantiation (see section 7):
  * **Lockstep**: caller's thread becomes the TickEngine thread. No dedicated thread.
  * **RealtimeAsync**: single dedicated TickEngine thread per World instance.
* Snapshot publication via arena generation swap (~1KB descriptor, <2us).

**Acceptance criteria**

* Mechanical enforcement via type/module privacy. Verification: `LockstepWorld` takes `&mut self`, `RealtimeAsyncWorld` spawns exactly one TickEngine thread.
* Snapshot publish overhead <2us measured in CI benchmark. Verification: automated benchmark with regression threshold.

### 6.1 Reference Architecture Diagram

```
[Many producers]                 [Many consumers]
    |                                  ^
    v                                  |
 WorldIngress --(bounded queue)--> TickEngine --(publish)--> ReadArena (latest generation)
                                         \--(retain)------> Ring Buffer of ReadArenas (K)
                                                           ^
                                                           |
                                                    WorldEgress + ObsPlan
```

---

## 7. Runtime Modes and Behavioral Contracts

### R-MODE-1 Mode Duality Is Architectural (MUST)

RealtimeAsync and Lockstep are **different ownership topologies** that share a propagator pipeline, not different policies on the same architecture. The engine MUST provide distinct mode-specific entry points.

| Mode | Architecture | Ingress | Egress | Use |
|------|-------------|---------|--------|-----|
| RealtimeAsync | Autonomous thread | best-effort; non-blocking submit | latest suitable snapshot via egress thread pool | live games/tools |
| Lockstep | Callable struct | synchronous; applied at tick boundary | inline from returned snapshot | deterministic training/replay |

### 7.1 Lockstep: Callable Struct (RL Training)

```rust
impl LockstepWorld {
    pub fn step_sync(&mut self, commands: &[Command]) -> StepResult<&Snapshot> {
        // Caller's thread becomes the tick thread
        // No ring buffer, no egress threads, no epoch reclamation
        // Obs filled inline from snapshot, same thread
    }

    pub fn reset(&mut self, seed: u64) -> &Snapshot {
        // &mut self guarantees no outstanding snapshot borrows
        // Arena reset is O(1) -- bump pointer reset
    }
}
```

Properties:
- **No ring buffer needed** (K=1 suffices).
- **No egress thread pool** (obs filled inline).
- **No wall-clock deadline** (maximize throughput, not frame rate).
- **`&mut self` enforces RL lifecycle** — borrow checker prevents snapshot references surviving across step/reset.
- **Vectorized:** 16-128 independent `LockstepWorld` instances, each owned by one thread, `Send` not `Sync`.
- **Bounded memory via double-buffer ping-pong** (see §5.6). Two arena buffers alternate; `&mut self` guarantees the previous generation is reclaimable. Memory does not grow with episode length.

**Lockstep deadlock (C-9) cannot occur.** Lockstep mode has no `WorldEgress` interface. Observations are filled inline from the returned `&Snapshot`. The observe-decide-act loop is a single-threaded sequential call chain:

```rust
let snapshot = world.step_sync(&commands)?;
let obs = fill_obs(&snapshot, &plan)?;
let actions = agent.decide(obs);
// repeat
```

There is no way to "request a future tick" — the caller IS the tick driver. The circular dependency (Egress blocks -> TickEngine waits -> agent waits -> Egress blocks) is architecturally impossible.

### 7.2 RealtimeAsync: Autonomous Thread (Games/Tools)

- TickEngine on dedicated thread, 60Hz wall-clock deadline.
- Egress thread pool reads `&Snapshot` from ring buffer concurrently via `ReadArena` (`Send + Sync`).
- Epoch-based reclamation for snapshot lifetime management.
- Fallback snapshot selection on topology changes (see section 13.5).

### 7.3 Shared Core

Both modes share: propagator pipeline, command ordering, field model, Space trait, ObsPlan compilation/execution. The mode-specific shells are thin wrappers (~200 lines each) around the shared core.

### 7.4 Rust Type Expression

```rust
trait World {
    type SnapshotRef<'a>: AsRef<Snapshot> where Self: 'a;
    fn step(&mut self, commands: &[Command]) -> Result<Self::SnapshotRef<'_>, StepError>;
}
```

GAT (Generic Associated Type) lets each mode define its snapshot reference type. Lockstep returns `&Snapshot` (zero-cost borrow). RealtimeAsync returns an epoch-guarded handle. C ABI hides this behind opaque handles.

### R-MODE-2 Overload/Backpressure (MUST)

Ingress implements (in both modes):

* bounded queue,
* TTL: tick-based in both modes (`expires_after_tick`). RealtimeAsync ingress MAY accept wall-clock `ttl_ms` from callers and convert at admit time: `expires_after_tick = current_tick_id + ceil(ttl_ms / configured_ms_per_tick)`.
* deterministic drop/reject policy,
* metrics for overload events.

### R-MODE-4 PettingZoo Parallel API Compatibility (SHOULD v1, MUST v1.5)

Lockstep mode SHOULD support PettingZoo Parallel API semantics for multi-agent environments, enabling standard RL framework integration.

---

## 8. Physical Threading Model

### 8.1 Lockstep Threading

No dedicated threads. The caller's thread executes the full pipeline: command processing -> propagators -> snapshot publish -> observation gather. Thread count equals the number of vectorized environments (typically 16-128).

### 8.2 RealtimeAsync Threading

| Thread(s) | Role | Owns |
|-----------|------|------|
| TickEngine thread (1) | Tick loop: drain ingress, run propagators, publish snapshot | `&mut WorldState`, `WriteArena` |
| Egress thread pool (N) | Execute ObsPlans against snapshots | `&ReadArena` (shared, immutable) |
| Ingress acceptor (0-M) | Accept commands, assign `arrival_seq` | Write end of bounded queue |

Snapshot lifetime is managed by epoch-based reclamation, not refcount eviction. This avoids cache-line ping-pong from atomic reference counting under high obs throughput.

### 8.3 Epoch Reclamation Safety (RealtimeAsync, MUST)

Epoch-based reclamation MUST handle stalled egress workers to prevent unbounded memory growth.

**Mechanisms (all MUST):**

1. **Quiescent point requirement.** Each egress worker MUST reach a quiescent point (release its current epoch) at least once per `max_epoch_hold` duration (configurable; default: 100ms / 6 ticks at 60Hz). Workers that fail to reach quiescence are considered stalled.

2. **Stalled worker teardown.** The TickEngine (or a monitoring thread) MUST detect workers that exceed `max_epoch_hold` and:
   - Cancel the in-flight ObsPlan execution (cooperative cancellation via flag). Cancellation flag SHOULD be checked between spatial region iterations in ObsPlan execution, not between individual cell accesses. This balances cancellation latency (~1ms worst case for large regions) against branch overhead in the gather inner loop.
   - Treat the worker's epoch as quiesced for reclamation purposes.
   - Log the stall event with worker ID, held epoch, and duration.
   - The worker MAY be restarted or replaced in the egress thread pool.

3. **Ring buffer eviction takes precedence.** If the snapshot ring buffer evicts a generation (by count or byte budget), that generation is removed from epoch eligibility regardless of worker epoch state. Workers holding evicted epochs receive `PLAN_INVALIDATED` on their next resolve attempt.

4. **Memory bound.** At any time, the number of live arena generations is bounded by: `K (ring size) + max_stalled_workers`. Since `max_stalled_workers` is bounded by the egress thread pool size (typically 4-8), total memory is predictable.

**Interaction with P-1 (Egress Always Returns):** A stalled worker that is torn down returns `ObsError::ExecutionFailed` with reason `WORKER_STALLED`. P-1 is satisfied because the *system* always returns — individual worker failures are reported, not swallowed.

---

## 9. Error Model and Recovery

### 9.1 Tick Atomicity (MUST)

Tick execution is **all-or-nothing**. If any propagator fails during a tick, all staging writes are abandoned and the world state remains exactly as before the tick started.

```rust
match pipeline.execute(&mut state, &mut staging, commands, dt) {
    Ok(()) => {
        arena.publish(staging);  // ownership transfer, zero-copy
        generation += 1;
        consecutive_rollback_count = 0;
    }
    Err(TickError::PropagatorFailed { name, reason }) => {
        drop(staging);           // abandon -- state unchanged, zero-cost
        // DO NOT re-enqueue. Drop all commands with TICK_ROLLBACK.
        for cmd in commands {
            receipts.push(Receipt::rejected(cmd, ReasonCode::TICK_ROLLBACK));
        }
        consecutive_rollback_count += 1;
        if consecutive_rollback_count >= MAX_CONSECUTIVE_ROLLBACKS {
            tick_disabled.store(true, Ordering::Release);
            // Log CRITICAL, reject further commands with MURK_ERROR_TICK_DISABLED
        }
    }
}
```

* Rollback is free with the arena model (abandon staging generation).
* **Commands are dropped** (not re-enqueued) with `TICK_ROLLBACK` reason code. Re-enqueue is forbidden: same commands trigger same failure (infinite loop), stale `basis_tick_id`, TTL violations, and ordering ambiguity for replay.
* **RealtimeAsync `tick_disabled` mechanism:** After 3 consecutive rollbacks, TickEngine sets `tick_disabled: AtomicBool` and stops executing ticks (thread stays alive for shutdown). Ingress rejects with `MURK_ERROR_TICK_DISABLED`. Egress continues serving last good snapshot (P-1 satisfied). Recovery: `reset()` clears the flag, or destroy the world.
* **Lockstep:** `step_sync()` returns `Err(StepError::PropagatorFailed)`. Caller decides. No `tick_disabled` mechanism needed.
* **Self-healing:** Agents observe unchanged state (P-1) and naturally resubmit corrected commands.
* Escape hatch: `reset()` for unrecoverable states (standard Gymnasium pattern).
* See Design Decisions v3.1, Decision J.

### 9.2 Propagator Failure (MUST)

A propagator failure (panic, NaN detection, constraint violation) MUST:
- Abort the current tick (no partial application of propagator results).
- Preserve the previous snapshot as authoritative state.
- Report the failure via `StepError::PropagatorFailed { name, reason }`.

NaN detection: propagators SHOULD validate outputs. The pipeline MAY run a configurable NaN sentinel check on written fields.

### 9.3 Snapshot Creation Failure (MUST)

If arena allocation fails (OOM during generation staging):
- TickEngine MUST NOT publish a partial snapshot.
- TickEngine MUST report `StepError::AllocationFailed`.
- The previous snapshot remains valid.
- RealtimeAsync: TickEngine enters degraded mode (reduced tick rate, metrics emitted).
- Lockstep: error propagated to caller.

### 9.4 ObsPlan Execution Failure (MUST)

If ObsPlan execution fails mid-fill:
- Caller-allocated buffer contents are **undefined** (not partially valid).
- Error returned with `ObsError::ExecutionFailed { plan_id, reason }`.
- `PLAN_INVALIDATED` returned if generation mismatch detected.

### 9.5 Ingress Overflow Per Mode (MUST)

- **RealtimeAsync**: deterministic drop policy (oldest-first or lowest-priority-first, configurable). Drops are metered and reason-coded.
- **Lockstep**: queue overflow is a programming error (caller controls command submission). MUST return `IngressError::QueueFull`.

### 9.6 C ABI Handle Lifecycle (MUST)

- Use-after-destroy: MUST return error code `MURK_ERROR_INVALID_HANDLE` (not undefined behavior).
- Double-destroy: MUST be a safe no-op.
- Thread safety: documented per handle type. `MurkWorld` handles for Lockstep are `Send` not `Sync`. `MurkSnapshot` handles for RealtimeAsync are `Send + Sync`.
- Error string ownership: engine-owned, valid until next call on same handle.

### 9.7 Error Code Enumeration (Minimum Set)

The following error codes MUST be defined for v1. Full enumeration to be completed during implementation.

| Code | Subsystem | Meaning | When Returned |
|------|-----------|---------|---------------|
| `MURK_OK` | All | Success | Any successful call |
| `MURK_ERROR_INVALID_HANDLE` | FFI | Handle destroyed or invalid | Any API call on destroyed handle |
| `MURK_ERROR_PLAN_INVALIDATED` | Egress | ObsPlan generation mismatch | ObsPlan execution on wrong snapshot generation |
| `MURK_ERROR_TIMEOUT_WAITING_FOR_TICK` | Egress | Exact tick request timed out | RealtimeAsync exact-tick egress |
| `MURK_ERROR_NOT_AVAILABLE` | Egress | Requested tick evicted from ring | RealtimeAsync exact-tick egress |
| `MURK_ERROR_INVALID_COMPOSITION` | ObsPlan | valid_ratio below 0.35 threshold | ObsPlan compilation for degenerate ProductSpace |
| `MURK_ERROR_QUEUE_FULL` | Ingress | Command queue at capacity | Ingress submit when queue full |
| `MURK_ERROR_STALE` | Ingress | Command basis_tick_id too old | Stale action rejection |
| `MURK_ERROR_TICK_ROLLBACK` | TickEngine | Propagator failed, tick rolled back | Command re-enqueue after tick failure |
| `MURK_ERROR_ALLOCATION_FAILED` | TickEngine | Arena OOM during generation staging | step() when memory exhausted |
| `MURK_ERROR_PROPAGATOR_FAILED` | TickEngine | Propagator returned error | step() when propagator fails |
| `MURK_ERROR_EXECUTION_FAILED` | Egress | ObsPlan execution error | Mid-execution failure |
| `MURK_ERROR_INVALID_OBSSPEC` | ObsPlan | Malformed ObsSpec at compilation | Missing field, invalid region, shape overflow |
| `MURK_ERROR_DT_OUT_OF_RANGE` | Pipeline | dt exceeds propagator max_dt | World creation with incompatible dt |
| `MURK_ERROR_WORKER_STALLED` | Egress | Egress worker exceeded max_epoch_hold | ObsPlan execution on stalled worker |
| `MURK_ERROR_SHUTTING_DOWN` | Lifecycle | World is shutting down | Command submitted during Draining or Quiescing phase (Decision E) |
| `MURK_ERROR_TICK_DISABLED` | TickEngine | Tick disabled after consecutive rollbacks | Command submitted or tick attempted after 3 consecutive rollbacks (Decision J) |

---

## 10. Observation-Action Alignment (Latency Gap Controls)

### R-ACT-1 Tick-Referenced Actions (MUST)

Ingress commands MUST be able to include tick reference metadata:

* `basis_tick_id` (snapshot tick the agent used)
* `intended_for_tick` OR `valid_after_tick`
* optional `max_tick_skew`

**Design decision (v1)**

* All command types support this metadata envelope.
* TickEngine resolves an `apply_tick_id` deterministically.

### R-ACT-2 Stale Action Policy (MUST)

TickEngine MUST define policy for stale actions, with asymmetric dampening per mode (P-3):

**RealtimeAsync**: reject if `basis_tick_id` too old (default). Adaptive `max_tick_skew` with exponential backoff SHOULD be supported to prevent rejection oscillation (reject -> retry -> more load -> more rejection). The backoff window increases `max_tick_skew` temporarily when rejection rate exceeds a threshold, then decays.

**Lockstep**: tighter skew (0-1 tick); missing the intended window defaults to rejection. Synchronous observation delivery eliminates the staleness loop entirely.

**Acceptance criteria**

* Receipts report `accepted`, `applied_tick_id`, and `reason_code` (`NONE` on success; rejection reason otherwise).
* Under stress test with 50 agents at degraded tick rate, rejection rate coefficient of variation MUST be < 0.3 (no oscillation).

---

## 11. Spatial Model (nD + Mixed Topologies via ProductSpace)

### R-SPACE-0 Dimensionality Is First-Class (MUST)

System MUST support spaces with dimensionality **N >= 1** and not hard-coded to a small fixed set.

**Design decision (v1 support envelope)**

* Architecturally supports any N (no hard-coded maximum).
* **Supported/tested N = 1..=5.**
* **N > 5 is allowed but untested/unsupported** (may work; not guaranteed).
* Runtime-N architecture; const-generic specializations permitted as internal optimization for common 2D/3D cases (public API is runtime-N).

### R-SPACE-1 Space Abstraction (MUST)

Each Space backend MUST provide:

* region planning primitives: `compile_region(region_spec) -> RegionPlan`
* deterministic iteration: `iter_region(RegionPlan) -> Iterator<Coord>`
* discrete topology functions (for discrete spaces):
  * `neighbours(Coord) -> fixed ordered list`
  * `distance(a, b) -> scalar` (if defined)
* observation mapping hooks:
  * `map_coord_to_tensor_index(Coord, MappingPlan) -> idx|invalid`
  * `canonical_ordering_spec()`

### R-SPACE-2 Discrete Lattice/Topology Is Pluggable (MUST)

Discrete spaces MUST NOT assume square/cubic Cartesian grid semantics.

Each discrete space defines (directly or via topology component):

* coordinate representation,
* neighbour enumeration,
* distance metric/geodesics,
* region queries,
* deterministic cell ordering for observation export and replay.

### R-SPACE-3 Minimise Manhattan Artefacts (MUST)

Provide at least one 2D discrete lattice more isotropic than square-4-neighbour.

**Concrete minimum (v1):** Hex2D lattice backend. Square grids remain supported but are not the only option.

### R-SPACE-4 Mixed Shapes Per Dimension / Product Spaces (MUST)

Engine MUST support composing dimensions with different shapes/topologies into a single space.

**Binding design decision: ProductSpace of component spaces**

* Mixed topology is represented as a **ProductSpace**.
* Each component may be multi-dimensional (critical for Hex2D).
* Overall coordinate is a tuple across components.

Examples:
* **Hex2D x Line1D** (layered hex maps)
* **Ring1D x Square2D**
* **VoxelOctree3D x Ring1D**

**Acceptance criteria**

* A single ObsSpec can address regions/samplers in mixed spaces. Verification: integration test with Hex2D x Line1D ObsPlan.
* Propagators operate via the space/topology interfaces (no hard-coded adjacency). Verification: propagator test runs against both LatticeSpace and ProductSpace.

### R-SPACE-4.1 ProductSpace Tested Compositions (MUST)

v1 tested compositions MUST be capped at **3 components maximum**. Compositions with >3 components are allowed but untested/unsupported.

**R-OBS-7 valid_ratio in ObsPlan metadata (MUST):** ObsPlan MUST report `valid_ratio` (fraction of tensor cells that are valid vs padding). Compositions with `valid_ratio < 0.5` MUST emit a warning. ProductSpace Hex2D x Hex2D yields ~56% valid — above the 0.35 threshold but below 0.5, triggering a compilation warning per R-OBS-7. Compositions below 35% MUST fail with `INVALID_COMPOSITION`.

### R-SPACE-5 Required Initial Backends (MUST)

* Retain current voxel/octree system as **VoxelOctreeSpace v1**.
* Add at least one general discrete lattice space supporting non-square 2D topologies (hex).

**Design decision (v1 backends)**

* **VoxelOctreeSpace v1**
* **LatticeSpace v1** supporting:
  * Line1D
  * Ring1D
  * Square4 (2D)
  * Square8 (2D) — distance metric: Chebyshev (consistent with 8-connected semantics)
  * Hex2D
* **ProductSpace** to compose component spaces.

### R-SPACE-6 Planned Backends (SHOULD / v1.5+)

* ContinuousGridSpace<N> (N-dimensional; tested N <= 5).
* GraphMeshSpace.

### R-SPACE-7 Hybrid Coupling (RESEARCH / EXPERIMENTAL)

Hybrid coupling between heterogeneous spaces is experimental; if present it must be feature-flagged.

### 11.1 ProductSpace Composition Semantics

**R-SPACE-8 ProductSpace Neighbours (MUST)**

ProductSpace `neighbours()` MUST be defined as: per-component neighbours only. For K components, each component contributes its own neighbours while other components hold constant. Cross-component coupling (diagonal neighbours) is NOT supported in v1.

**Worked example — Hex2D x Line1D:**

Given coordinate `(h, l)` where `h = (2, 1)` in axial Hex2D and `l = 5` in Line1D:
- Hex2D neighbours of `(2, 1)`: 6 cells -> `{(3,1), (3,0), (2,0), (1,1), (1,2), (2,2)}` (standard axial offsets)
- Line1D neighbours of `5`: `{4, 6}`
- ProductSpace neighbours: `{((3,1), 5), ((3,0), 5), ((2,0), 5), ((1,1), 5), ((1,2), 5), ((2,2), 5), ((2,1), 4), ((2,1), 6)}` = **8 neighbours**

**Worked example — 3 components (Hex2D x Line1D x Ring1D):**

For coordinate `(h, l, r)`, neighbours = `{(h', l, r) | h' in hex_neighbours(h)}` union `{(h, l', r) | l' in line_neighbours(l)}` union `{(h, l, r') | r' in ring_neighbours(r)}`. Each component varies independently. Neighbour count = sum of per-component neighbour counts.

**R-SPACE-9 ProductSpace Distance (MUST)**

ProductSpace MUST provide two distance functions:

1. `distance(a, b) -> scalar` — the **graph-geodesic distance** (L1: sum of per-component distances). This is the default and primary metric. It MUST equal the shortest path length in the ProductSpace adjacency graph defined by R-SPACE-8. All subsystems that use distance for causal reasoning (observation shells, foveation, propagator influence ranges) MUST use this function.

2. `metric_distance(a, b, metric: ProductMetric) -> scalar` — configurable metric for region queries and user-specified geometry. Supported metrics: `L1` (sum), `LInfinity` (max), `Weighted(weights)`.

**Rationale:** Per-component adjacency (R-SPACE-8) produces L1 geodesics. The default distance MUST match the graph geodesic so that observation shell radii correspond to causal propagation cones. L∞ is available via `metric_distance()` for region queries that want Cartesian-product-shaped selections.

**Worked example — Hex2D x Line1D:**

`distance((h1, l1), (h2, l2)) = hex_distance(h1, h2) + line_distance(l1, l2)`

For `((2,1), 5)` to `((4,0), 8)`: hex_distance = 2, line_distance = 3, **distance = 5** (graph geodesic, L1).

`metric_distance(_, _, LInfinity)` for the same pair: max(2, 3) = **3**.

**R-SPACE-10 ProductSpace Iteration Ordering (MUST)**

ProductSpace deterministic iteration MUST use lexicographic ordering across components (leftmost component varies slowest). This ordering is a compatibility boundary for replay and observation export.

**Worked example — Hex2D x Line1D:**

For region Hex2D `{(0,0), (1,0), (0,1)}` x Line1D `{0, 1}`, iteration order:
```
((0,0), 0), ((0,0), 1),   // hex (0,0), all line values
((0,1), 0), ((0,1), 1),   // hex (0,1), all line values
((1,0), 0), ((1,0), 1)    // hex (1,0), all line values
```
Hex component (leftmost) varies slowest; Line component (rightmost) varies fastest. Within each component, ordering follows that component's canonical ordering (Hex2D: r-then-q ascending).

**R-SPACE-11 ProductSpace Region Queries (MUST)**

Region queries MUST compile per-component and combine via Cartesian product. A region `R_hex x R_line` iterates all `(h, l)` where `h in R_hex` and `l in R_line`, in lexicographic order per R-SPACE-10.

**Worked example — Hex2D x Line1D:**

Query: "all cells within hex radius 1 of (2,1) on layers 3-5."
- `R_hex = hex_disk((2,1), 1)` -> 7 cells (center + 6 neighbours)
- `R_line = range(3, 5)` -> 3 cells
- Product region: 7 x 3 = **21 coordinates**, iterated in lexicographic order.
- Tensor output: shape `[7, 3, num_fields]` with hex padding mask applied to first dimension.

**R-SPACE-12 ProductSpace Propagator Granularity (MUST)**

Propagator read/write conflict detection operates at **whole-field** granularity for v1. Per-component conflict detection is a v1.5 optimisation.

---

## 12. Hex2D: Canonical Representation and Determinism

### 12.1 Coordinate Representation

**Canonical:** axial coordinates `(q, r)`.

* Neighbours: 6 ordered direction offsets (documented order).
* Distance metric: implement via cube conversion internally:
  * `x=q, z=r, y=-x-z`
  * `dist = max(|dx|, |dy|, |dz|)`

### 12.2 Deterministic Ordering

All region iteration MUST be deterministic.

**Canonical region ordering for Hex2D (v1)**

* primary sort by `r` ascending
* secondary sort by `q` ascending

This ordering is part of the determinism contract and MUST be treated as a compatibility boundary.

### 12.3 Fixed-Shape Tensor Export Mapping (Hex -> Tensor)

Hex regions are non-rectangular. Export uses:

* a rectangular bounding box in a defined offset layout,
* a dense tensor for that box,
* a **validity mask** where `1 = valid cell`, `0 = padding/invalid`.

**Padding efficiency** (relates to R-OBS-7):
- Hex disk radius R: bounding box = (2R+1) x (2R+1), valid cells = 3R^2+3R+1, `valid_ratio` converges to **0.75** for large R (0.78 for R=1, monotonically decreasing).
- Hex rectangle WxH in offset coordinates: bounding box = W x H, `valid_ratio` = 1.0 (all cells valid by construction).
- Single-hex compositions (Hex2D x Line1D): preserves per-component hex `valid_ratio` >= 0.75.
- Hex2D x Hex2D: `valid_ratio` ~ 0.75^2 ~ **0.56** (above 0.35 threshold, passes; below 0.5, emits warning per R-OBS-7).

ObsPlan MAY additionally export an index map (tensor index -> axial coord) for debugging/traceability.

Branch-free hex padding: `memset(0)` + sparse valid-index gather, no per-cell branch.

---

## 13. Field Model Requirements

### R-FIELD-1 Typed First-Class Fields (MUST)

Support:

* Scalar
* Vector
* Categorical

Each field includes metadata:

* units (annotated; structure optional),
* bounds (optional),
* boundary behaviour: clamp/reflect/absorb/wrap,
* precision/storage policy (f32 minimum in v1; extensible).

### R-FIELD-2 Storage Layout and Bandwidth Awareness (SHOULD)

* Prefer **SoA** layouts for numerically processed fields.
* Support chunking/tiling strategies (esp. large lattices and continuous grids).
* Track or estimate bytes read/written per tick and bytes generated per observation.

### R-FIELD-3 Field Mutability (MUST)

Each field MUST declare its mutability class:

| Class | Arena Behavior | Sharing |
|-------|---------------|---------|
| `Static` | Generation 0 forever | Shared across all snapshots and vectorized envs |
| `PerTick` | New allocation each tick if modified | Per-generation |
| `Sparse` | New allocation only when modified | Shared until mutation |

The arena MUST use this classification to optimise allocation. `Static` fields MUST share a single allocation across all snapshots and vectorized environment instances.

`Sparse` fields: allocated and zero-initialized at world creation. Shared across generations until first modification, at which point a new allocation is made in the current generation. Read access to an unmodified `Sparse` field returns the zero-initialized generation-0 allocation.

**R-FIELD-4 Static Field Set (MUST)**

Fields MUST be defined at world creation. v1 MUST NOT support runtime field addition or deletion. Adding/removing fields requires world recreation. This simplifies generation ID management. Runtime field mutation is a v1.5 consideration.

---

## 14. Command Model Requirements (WorldEvents vs GlobalParameters)

### R-CMD-1 Command Taxonomy (MUST)

Ingress commands categorised as:

* **WorldEvents** — ephemeral mutation intents (move/spawn/damage).
* **GlobalParameters** — persistent rules/config changes (gravity, diffusion coefficients).

### R-CMD-2 Distinct Semantics and Logging (MUST)

* GlobalParameters are versioned and appear in snapshots (or attached config objects).
* Replay logs preserve:
  * original intent,
  * application result,
  * applied tick,
  * parameter-version transitions.

### 14.1 Deterministic Command Ordering (Binding Decision)

TickEngine drains and applies commands in deterministic order:

1. Resolve `apply_tick_id` for each command.
2. Group by `apply_tick_id`.
3. Sort within tick by:
   * `priority_class` (system > global > events; configurable but fixed)
   * then `(source_id, source_seq)` if both are provided
   * else TickEngine-assigned monotonic `arrival_seq` (assigned at ingress-admit time)

`arrival_seq` MUST come from a single world-local monotonic counter and MUST be unique per accepted command.

Lockstep deployments that require tight repeatability SHOULD provide `(source_id, source_seq)`; realtime deployments MAY rely on `arrival_seq`.

### 14.2 Receipts

Receipt fields (minimum):

* `accepted: bool`
* `applied_tick_id: u64 | null`
* `reason_code: enum` (including `NONE`, `STALE`, `QUEUE_FULL`, `TICK_ROLLBACK`, etc.)
* `basis_tick_id_echo: u64 | null`
* `parameter_version_after: u64 | null`

### 14.3 Replay Ordering Provenance (MUST)

Replay logs MUST carry sufficient ordering provenance so command application order is reconstructed exactly.

For each accepted command log record, include at minimum:

* resolved `apply_tick_id`
* resolved `priority_class`
* `source_id` and `source_seq` if present
* `arrival_seq` (always present)
* `expires_after_tick` (always present; resolved at ingress-admit time)

Replayers MUST use recorded resolved ordering metadata and MUST NOT recompute ordering from wall-clock intake timing.

### 14.4 Replay Log Format (MUST)

A minimum replay log record format MUST be defined before v1 ships. The format MUST support:

* Initial state descriptor (or deterministic init parameters + seed).
* Per-tick command records with full ordering provenance.
* Build metadata header (toolchain version, ISA, compiler flags) for Tier B verification.

---

## 15. Propagators

### R-PROP-1 Concrete Propagator Trait (MUST)

Propagators are modular, stateless operators in TickEngine's per-tick pipeline.

```rust
pub trait Propagator: Send + 'static {
    fn name(&self) -> &str;
    fn reads(&self) -> FieldSet;
    fn reads_previous(&self) -> FieldSet { FieldSet::empty() }
    fn writes(&self) -> Vec<(FieldId, WriteMode)>;  // called once at startup, not per-tick
    fn max_dt(&self) -> Option<f64> { None }
    fn scratch_bytes(&self) -> usize { 0 }
    fn step(&self, ctx: &StepContext, dt: f64) -> Result<(), PropagatorError>;
}

pub enum WriteMode {
    /// Fresh uninitialized buffer -- propagator fills completely
    Full,
    /// Seeded from previous generation via memcpy -- propagator modifies in-place
    Incremental,
}

pub struct StepContext<'a> {
    reads: FieldReadSet<'a>,       // current in-tick view: base gen + staged writes from prior propagators
    reads_prev: FieldReadSet<'a>,  // frozen tick-start view: base gen only, never sees staged writes
    writes: FieldWriteSet<'a>,     // &mut WriteArena: new generation staging
    scratch: &'a mut ScratchRegion,// bump allocator, reset between propagators
    space: &'a dyn SpaceAccess,
    tick_id: u64,
    dt: f64,
}
```

### R-PROP-2 Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| `&self` not `&mut self` | Stateless operators: `Send`, deterministic, mode-independent. Persistent state goes through fields. |
| `reads()` vs `reads_previous()` | Sequential-commit default (Euler); opt-in previous-generation reads (Jacobi/parallel). Makes dependency chain a verifiable DAG. |
| `WriteMode::Full` vs `Incremental` | Full = uninitialized buffer (diffusion); Incremental = seeded from gen N (sparse agent updates). Engine budgets seed cost at startup. |
| `max_dt()` | Pipeline validates `dt <= min(max_dt)` at startup. Exposed through C ABI as `dt_range()`. |
| `scratch_bytes()` | Pre-allocated bump region in StepContext, O(1) reset between propagators. No heap allocation in inner loop. |

### R-PROP-3 Deterministic Execution Guarantees (MUST)

* Execution order is a strict ordered list in v1 (not a dependency graph).
* Internal parallelism must not change results within Tier B determinism.

**v1 recommendation on parallelism:**

* Allow deterministic data-parallel updates without reductions.
* Treat reductions (sum/mean pooling, accumulation) as single-threaded or deterministic-tree reductions with fixed partitioning.

### R-PROP-4 max_dt Declaration (MUST)

Each propagator MUST declare its maximum stable timestep via `max_dt()`. The pipeline MUST validate at startup that the configured `dt` is within the intersection of all propagators' `max_dt` ranges. Exposed through C ABI as `murk_dt_range()` for RL users to query valid timestep ranges.

### R-PROP-5 Pipeline Startup Validation (MUST)

The propagator pipeline MUST be validated at startup. Validation includes:

* **No write-write conflicts**: two propagators writing the same field is an error.
* **Read-after-write ordering verified**: dependency DAG is acyclic given the declared execution order.
* **All field references valid**: every field ID in `reads()`, `reads_previous()`, and `writes()` exists.
* **`dt` within range**: configured `dt <= min(max_dt)` for all propagators.
* **WriteMode budget**: `Incremental` writes have their seed memcpy cost estimated and reported.

Validation failure MUST prevent world creation (not deferred to first tick).

### 15.2 StepContext Read Semantics (MUST)

- `reads` provides the **current in-tick view**: the base generation (tick-start state) overlaid with staged writes from all previously-executed propagators in this tick. If propagator A writes field X, then propagator B (executing after A) sees A's written values when reading field X via `reads`. This enables Euler-style sequential integration.

- `reads_previous` provides the **frozen tick-start view**: the base generation only, never reflecting staged writes from any propagator in the current tick. This enables Jacobi-style parallel integration where all propagators read the same consistent pre-tick state.

- A propagator that declares a field in both `reads()` and `writes()` with `WriteMode::Incremental` sees the previous generation's values in the seeded write buffer, and its own in-progress writes. It does NOT see other propagators' staged writes to the same field (write-write conflicts are forbidden by R-PROP-5).

In v1, the sequential visibility guarantee for `reads` is trivially satisfied by single-threaded execution. If v1.5 introduces parallel propagators (disjoint writes only per R-PROP-5), the overlay MUST provide acquire/release ordering so that a propagator reading field X sees the complete write from the propagator that wrote X.

### 15.3 Reward as a Propagator

Reward computation is a standard propagator that runs last in the pipeline (dependency constraint: reads all mutable fields it needs). Benefits:

* No special reward API — standard propagator interface.
* Acts as implicit cache prefetch for subsequent ObsPlan execution.
* Deterministic, replayable, field-stored.

---

## 16. Observation and ML Integration

### R-OBS-1 ObsSpec Contract (MUST)

ObsSpec defines:

* fields to sample,
* region selection primitives compatible with nD and mixed topologies,
* output shape contract,
* normalisation,
* masking,
* optional history.

### R-OBS-2 LOD/Subsampling/Pooling (MUST)

ObsSpec supports:

* pooling reducers (mean/max/min/sum where meaningful),
* foveation/shells,
* multi-resolution outputs,
* topology-aware neighbourhood pooling (hex-aware, etc.).

### R-OBS-3 ObsPlan Compilation (MUST)

ObsSpec MUST compile into ObsPlan before hot-path execution:

* validate spec (including malformed input: non-existent fields, out-of-bounds regions, overflow output shapes),
* resolve field IDs/offsets,
* precompute region iterators and index mappings,
* precompute pooling kernels,
* compute output shapes/strides.

**Acceptance criteria**

* Egress does not interpret ObsSpec schema per call; it executes an ObsPlan. Verification: profiling shows zero schema parsing in hot path.
* Malformed ObsSpec MUST be rejected at compilation with specific error codes per failure class (missing field, invalid region, shape overflow).

### R-OBS-4 Fixed-Shape Tensors + Masks (MUST)

* Export fixed-shape tensors suitable for RL frameworks.
* Non-rectangular domains map deterministically with padding and validity masks; mapping is documented.

### R-OBS-5 Freshness/Completeness Metadata (MUST)

Return:

* `tick_id`
* `age_ticks` (tick-count, per P-2; `age_ms` also available for RealtimeAsync display)
* `coverage`
* `validity_mask`
* `world_generation_id` (was `topology_generation`)
* `parameter_version`

### R-OBS-6 ObsPlan Validity and Generation Binding (MUST)

ObsPlan cache validity MUST be enforced with generation IDs.

**Design decision (v1):** Use a single `world_generation_id` (not three separate IDs). Split to separate field/topology IDs in v1.5 if profiling justifies it. This reduces the invalidation surface and simplifies caching.

**Critical:** Plans MUST be matched against **snapshot generation**, not world's current generation. A plan compiled at generation G is valid for any snapshot produced at G, even after the world advances to G+1. This prevents mass invalidation cascades.

* On snapshot-plan generation mismatch, egress MUST fail plan execution with `PLAN_INVALIDATED`.
* Rate-limited recompilation SHOULD be supported to prevent recompilation spikes.
* Field IDs and Space IDs MUST be stable under an unchanged world definition.

### R-OBS-7 valid_ratio Reporting (MUST)

ObsPlan MUST compute and report `valid_ratio` (fraction of output tensor cells that represent valid spatial data vs padding). Compositions with `valid_ratio < 0.5` MUST emit a warning at plan compilation. Compositions with `valid_ratio < 0.35` MUST fail with `INVALID_COMPOSITION`.

### R-OBS-8 Batch ObsPlan Execution (SHOULD v1, MUST v1.5)

ObsPlan SHOULD support batch execution for multi-agent environments: a single traversal fills N agent observation buffers. This is critical for vectorized RL throughput.

### 16.1 ObsPlan Plan Classes and Optimisation Stack

Smart ObsPlan compilation recognises simple specs and emits tight gather loops:

| Plan class | Inner loop | Latency (per obs) |
|------------|-----------|-------------------|
| Simple (direct reads + normalization) | Branch-free indexed gather | <= 100us (p99) |
| Standard (pooling + foveation) | Precomputed kernels | <= 5ms (p99) |

Optimisation stack:

1. **Precomputed relative index mappings**: agent position changes -> only base offset updates, zero recompilation.
2. **Branch-free hex padding**: `memset(0)` + sparse valid-index gather, no per-cell branch.
3. **FieldMutability caching**: Static fields cached once, only PerTick fields re-gathered per step.
4. **Batch execution**: single traversal fills N agent buffers (SHOULD v1, MUST v1.5).
5. **Interior/boundary dispatch**: O(1) check per agent, branch-free interior path, validated boundary path.
6. **Causal-consistent observation shells**: Foveation shells and distance-based region selection SHOULD default to `distance()` (graph-geodesic, L1) so observation geometry matches causal propagation cones. `metric_distance(LInfinity)` MAY be used when Cartesian-product-shaped shells are explicitly desired.

### 16.2 Topology Change Graceful Degradation (RealtimeAsync)

- Generation bump invalidates all cached ObsPlans.
- **Fallback snapshot selection**: egress scans ring for newest compatible snapshot (matching topology generation).
- Agents get slightly stale but valid observations while plans recompile on egress thread pool.
- Lockstep unaffected: topology changes happen during `reset()` with synchronous recompilation.

### R-OBS-9 Rate-Limited Plan Recompilation (SHOULD)

ObsPlan recompilation on topology change SHOULD be rate-limited to prevent thundering herd. When many plans are invalidated simultaneously, recompilation SHOULD be spread across egress thread pool over a bounded window rather than all recompiling at once.

### 16.3 ObsSpec Schema Format (MUST)

**Binding decision (v1): FlatBuffers**

* low allocation, portable, versionable.
* suitable for high-frequency cross-language use.
* Schema evolution strategy: versioned schemas with forward-compatible additions. Breaking changes require a new schema version.

---

## 17. Snapshots and Retention Policy

### R-SNAP-1 Retention Strategy (MUST)

Snapshot retention uses arena-based generational allocation (see section 5). Mode-specific strategies:

**Lockstep:**
* K=1 (only the latest snapshot). Returned directly as `&Snapshot` from `step_sync()`.
* `&mut self` on `step_sync()`/`reset()` guarantees no outstanding borrows.
* Arena reset is O(1) on `reset()` (bump pointer reset).

**RealtimeAsync:**
* Ring buffer of last **K** `ReadArena` generations for exact-tick reads (default K=8; configurable).
* Bounded by count and/or byte budget; eviction by **epoch-based reclamation** (not refcount).
* Epoch-based reclamation: egress threads register epochs; arena generations are freed when no egress thread holds a reference to that epoch.
* Each snapshot carries `world_generation_id` and `parameter_version` for ObsPlan compatibility checking and parameter tracking.

### R-SNAP-2 Lockstep Exact-Tick Egress Semantics (MUST)

In Lockstep mode, `step_sync()` returns the snapshot directly. No exact-tick request mechanism needed — the caller controls which tick is executed.

### R-SNAP-3 RealtimeAsync Exact-Tick Egress Semantics (MUST)

* Egress MAY request exact `tick_id`.
* If present in ring: return immediately.
* If future tick: may block up to policy timeout.
* If timeout elapses: return `TIMEOUT_WAITING_FOR_TICK` with `requested_tick_id`, `latest_tick_id`, and `waited_ms`.
* If too old/evicted: return `NOT_AVAILABLE` with `requested_tick_id` and `latest_tick_id`.
* Per P-1: if *any* snapshot is available, egress MUST return data (with staleness metadata) rather than blocking indefinitely.

All exact-tick egress responses MUST include `status_code` and `tick_id` (or requested tick metadata when unavailable).

---

## 18. Language Bindings (C ABI + Python Fast Path)

### R-FFI-1 Stable, Handle-Based C ABI (MUST)

* Versioned, handle-based C ABI.
* Opaque handles (World, ObsPlan, Snapshot, etc.).
* Explicit create/destroy.
* ABI version exposed via `murk_abi_version()` function.
* Handle lifecycle invariants enforced per section 9.6.

### R-FFI-2 Caller-Allocated Buffers (MUST)

Primary tensor export path uses caller-owned buffers:

* Python allocates NumPy buffer,
* FFI fills it.
* Output descriptors: dtype, shape, strides, alignment.
* Mask format: `uint8`, `1 = valid`, `0 = invalid/padding`, shape matches exported tensor spatial footprint.

### R-FFI-3 Portable ObsSpec Schema (MUST)

ObsSpec is serialisable and portable across bindings, with explicit versioning. FlatBuffers (see section 16.3).

### R-FFI-4 Batched step_vec C API (MUST v1)

Vectorized RL training requires batched stepping across multiple environments:

```c
murk_status_t murk_lockstep_step_vec(
    MurkWorld* const* worlds,    // array of world handles
    const MurkCommand* commands, // batched commands
    size_t world_count,
    MurkSnapshot* const* out_snapshots
);
```

This enables a single Python call to step all vectorized environments, minimizing FFI overhead.

### R-FFI-5 GIL Release (MUST)

Python bindings MUST release the GIL for the entire duration of C ABI calls. This enables true parallelism across vectorized environments from Python.

---

## 19. Determinism, Numeric Policy, Replay

### R-DET-1 Determinism Tier (MUST)

**Chosen for v1: Tier B**

Deterministic within:

* same build,
* same ISA/CPU family,
* fixed compiler/toolchain flags,
* same initial state + seed + applied command log.

Build metadata recording in replay headers is **MUST** (not recommended).

### R-DET-2 Numeric Strategy Gate (MUST)

If Tier C is required later, architecture must specify:

* fixed-point strategy and scaling, OR
* software float / strict-math library strategy,
* plus compilation flags and forbidden optimisations.

**v1 Tier B constraints**

* Prohibit fast-math-style reassociation for authoritative code paths.
* Record build metadata in replay headers (MUST).
* Known non-determinism sources and mitigations:
  * `HashMap` iteration order -> use `BTreeMap` or `IndexMap` for deterministic paths.
  * Float reassociation -> prohibit `-ffast-math` equivalent, use explicit operation ordering.
  * Allocation-dependent sort stability -> use stable sorts or tie-breaking by `arrival_seq`.

### R-DET-3 Replay Support (MUST)

Replay achievable from:

* initial snapshot/version (or deterministic init descriptor),
* seed,
* command log (including receipts and applied ticks),
* resolved command ordering provenance (`apply_tick_id`, priority class, source tuple if present, `arrival_seq`),
* determinism tier + numeric/config policy,
* build metadata (toolchain, ISA, flags).

### R-DET-4 Tick-Expressible Time References (MUST)

Per P-2: all engine-internal time references that affect state transitions MUST be expressible in tick-count. Specifically:

* Lockstep TTL: tick-count (`expires_after_tick`, set directly by submitter).
* RealtimeAsync TTL: tick-count (`expires_after_tick`). Ingress convenience layer converts wall-clock `ttl_ms` at admit time. Accept/drop decisions use tick-count only.
* Lockstep timeouts: tick-count.
* `age_ticks` in observation metadata: tick-count (always available). `age_ms` also available for RealtimeAsync display.

### 19.1 Determinism Verification

**v1 implementation requirement:** A CI replay-and-compare test MUST exist that:
- Runs **in Lockstep mode** using recorded command logs and fixed `dt`.
- Compares snapshots at tick N for bit-exact equality across two runs with identical inputs.
- Minimum replay length: 1000 ticks.
- Fails on any divergence.

**Mode scoping:** Determinism verification MUST target Lockstep mode. RealtimeAsync introduces thread scheduling non-determinism (command drain timing, ingress ordering under concurrent submission) that is outside the scope of Tier B determinism. RealtimeAsync determinism is a v1.5 investigation if needed.

**Minimum test coverage:**
- At least one scenario with propagators reading both `reads` and `reads_previous` fields (verifying sequential-commit vs Jacobi semantics per §15.2).
- At least one scenario with command ordering from multiple sources (`source_id` disambiguation).
- At least one scenario with `WriteMode::Incremental` propagators (seed + modify pattern).
- At least one scenario exercising arena double-buffer recycling (§5.6) across 1000+ ticks to verify no generation handle leakage.
- At least one scenario with Sparse field modification pattern (field shared for N ticks, then modified, verifying correct generation tracking and Sparse promotion).

### R-DET-5 Build Metadata in Replay (MUST)

Replay log headers MUST include build metadata: toolchain version, ISA, compiler flags. This is required (not recommended) for Tier B verification.

### R-DET-6 Determinism Source Catalogue (MUST)

Implementation MUST maintain a living catalogue of known non-determinism sources and mitigations (started in R-DET-2 above). Each new source discovered during development MUST be documented with its mitigation or marked as a tier-violation. The catalogue MUST be reviewed before v1 release. Systematic testing across ISAs is deferred to the implementation phase but MUST be addressed before v1 ships.

---

## 20. Performance and Operational Requirements

### R-PERF-1 Mode-Specific Performance Budgets (MUST)

#### RealtimeAsync (60Hz Wall-Clock Deadline)

| Phase | Budget | % of Frame |
|-------|--------|------------|
| Ingress drain + sort | 500us | 3% |
| Propagator pipeline | 12ms | 72% |
| Snapshot publish | 2us | 0.01% |
| Egress notification | 200us | 1% |
| Overhead | 1ms | 6% |
| **Headroom** | **2.97ms** | **18%** |

Obs generation: off-thread (egress pool), does not count against tick budget.

#### Lockstep (Throughput-Maximized, No Deadline)

| Phase | Target | Notes |
|-------|--------|-------|
| Command processing | target: 5us | Minimal for single-agent |
| Propagator pipeline | target: 50-80us | Depends on propagator count and cell count |
| Snapshot publish | target: <0.1us | Descriptor swap only |
| Obs generation (inline) | target: 16-27us | 16 agents x ~1.7us/obs |
| **Total** | **target: 70-115us** | |
| **Throughput** | **target: 8,700-14,300 steps/sec** | Per env |
| **Vectorized (16 cores)** | **target: 139K-229K steps/sec** | Aggregate |

#### MuJoCo Ant-v4 Comparison

MuJoCo: ~20,000 steps/sec per env -> 320,000 aggregate (16 cores).
Murk: ~14,000 steps/sec per env -> 224,000 aggregate -> **70% of MuJoCo throughput**.

Competitive for a general-purpose engine. Murk wins on scenarios MuJoCo can't serve (hex strategy, ecosystem, multi-topology).

#### Tiered ObsPlan Targets (Replaces Flat 200 obs/sec)

| Plan class | Target | Rationale |
|------------|--------|-----------|
| Simple (direct reads + normalization) | >= 500,000 obs/sec | Branch-free indexed gather, ~2us/obs |
| Standard (pooling + foveation) | >= 200 obs/sec | Precomputed kernels, <= 5ms/obs |

#### Memory Copy Scorecard (Full RL Training Loop)

| Copy | Size | Cost |
|------|------|------|
| GPU->CPU action | ~64B | ~10us (DMA) |
| Action -> WorldEvent | ~100B/agent | <1us |
| Field seed (Incremental writes) | ~40KB | ~0.5us |
| Field gather -> obs buffer | ~2.5KB/obs | ~0.5us/field |
| Validity mask | ~160B | <0.1us |
| CPU->GPU obs | ~20KB | ~20us (DMA) |

**Framework overhead: ~3us.** GPU DMA dominates. The architecture is not the bottleneck.

### R-PERF-2 Memory Bandwidth Metrics (MUST)

Performance reporting MUST include:

* bytes read/written per tick (or estimates),
* bytes generated per observation,
* cache-miss sensitive hotspots (profiling requirement).

### R-PERF-3 Reference Profile (MUST)

A concrete reference profile MUST be defined and published with benchmark results.

**v1 Reference Profile (binding):**
* **Baseline scenario**: 10K cells (100x100 Square2D), 5 fields (2 Scalar, 2 Vector, 1 Categorical), 3 propagators (diffusion, agent movement, reward), 16 agents.
* **Stress scenario**: 100K cells (316x316 Square2D), same fields/propagators/agents.
* **Hardware**: documented CPU model, core count, memory size and speed (e.g., AMD Ryzen 5950X, 16C/32T, 64GB DDR4-3200).
* CI benchmark MUST report tick time, obs generation time, memory usage against this profile with regression thresholds.
* Results kept stable for regression comparisons.

### R-OPS-1 Telemetry (SHOULD)

Metrics/logging for:

* tick duration/jitter,
* queue depth/TTL expiries/drops,
* snapshot age distributions,
* obs generation timings per plan,
* rejection rate and backoff state (RealtimeAsync).

---

## 21. Stable IDs and Generation Policy

### 21.1 Stable IDs and Generation Policy (MUST)

* Field IDs and Space IDs are stable under the same world definition.
* World configuration exposes a monotonic `world_generation_id`.
  * Bumped on **plan-relevant changes** only: topology changes (space dimensions, lattice type, boundary conditions), field layout changes (disallowed in v1 per R-FIELD-4, but future-proofed), and space configuration changes that affect coordinate-to-tensor mappings.
  * MUST NOT bump on GlobalParameter changes (gravity, diffusion coefficients, etc.) or entity spawn/despawn (these are field mutations, not layout changes).
  * v1.5: MAY split into `field_layout_generation_id` and `space_topology_generation_id` if profiling shows excessive invalidation.
* GlobalParameter changes are tracked separately via `parameter_version` (see §14.2). Propagators access parameters via `StepContext`, not via generation-gated lookups.
* Snapshot metadata MUST include `parameter_version` alongside `tick_id` and `world_generation_id`. This enables egress consumers to correlate observations with parameter regimes without requiring receipt log access.
* Plans bind to `world_generation_id`; snapshot-plan mismatch triggers `PLAN_INVALIDATED`.

**Rationale:** ObsPlan validity depends on field layout and spatial topology (tensor shapes, index mappings). Parameter tweaks don't affect tensor shapes — only field values change, which ObsPlan reads directly from snapshots. Bumping `world_generation_id` on parameter changes would cause mass plan invalidation during curriculum learning, defeating R-OBS-9.

### 21.2 Validity Mask Semantics (MUST)

* Mask dtype: `uint8`.
* `1 = valid`, `0 = invalid/padding`.
* Mask shape matches exported tensor spatial footprint.

---

## 22. Migration and Deliverables

### R-MIG-1 No Rewrite Cliff (MUST)

Retain current voxel/octree backend as v1 discrete backend and wrap via new space abstraction. Arena-based allocation keeps the concurrent-propagator path open for v1.5.

### v1 (MUST)

* Three-interface architecture with mode duality.
* RealtimeAsync + Lockstep as distinct ownership topologies.
* Arena-based generational allocation.
* Concrete propagator trait with pipeline validation.
* Error model with tick atomicity and all-or-nothing rollback.
* VoxelOctreeSpace v1 integrated.
* Hex-capable discrete lattice backend (Hex2D).
* ProductSpace composition model (tested up to 3 components).
* ObsSpec -> ObsPlan compilation + fixed-shape tensor export + masks + metadata.
* Handle-based C ABI + Python fast path with GIL release, including batched `step_vec`.
* Deterministic command ordering + replay log format.
* Tested/supported N = 1..=5.
* Reference profile defined with CI benchmarks.
* Determinism replay CI test.

### v1.5 (SHOULD)

* ContinuousGridSpace<N> + operator parity.
* Expanded ObsSpec history + richer pooling.
* Batch ObsPlan execution (MUST).
* Per-component propagator conflict detection in ProductSpace.
* Split generation IDs (field/topology) if profiling justifies.
* Dynamic LOD load-shedding for RealtimeAsync.
* Field->chunk arena migration for LOD (via `FieldStorage` trait abstraction).
* PettingZoo Parallel API compatibility (MUST).

### v2 (COULD / EXPERIMENTAL)

* GraphMeshSpace maturity.
* Hybrid coupling (feature-flagged research).
* Wave/radiative operators if prioritised.

---

## 23. Mandatory v1 Test Set

These tests MUST exist before v1 ships. Derived from architectural and domain reviews.

### Unit / Property Tests

1. Hex2D canonical iteration ordering for all region shapes.
2. ProductSpace region query for Hex2D x Line1D.
3. ProductSpace padding ratio >= 35% for all v1-tested compositions.
4. Propagator write-write conflict detection at startup.
5. Command deterministic ordering under concurrent multi-source ingress.
6. `FieldMutability` sharing: Static fields shared across vectorized envs.

### Integration Tests

7. Determinism replay (Lockstep mode): identical scenario twice -> bit-exact snapshot at tick N (minimum 1000 ticks). See §19.1 for minimum test coverage requirements.
8. ObsPlan generation invalidation -> `PLAN_INVALIDATED` error on snapshot-plan mismatch.
9. Snapshot ring eviction -> `NOT_AVAILABLE` response.
10. Ingress backpressure -> deterministic drop behavior.
11. Hex -> rectangular tensor export with validity mask for known geometries.
12. C ABI handle lifecycle: create/destroy in all valid and invalid orderings (use-after-destroy returns error, double-destroy is no-op).
13. Tick rollback: propagator failure -> world state unchanged, commands re-enqueued.
14. `valid_ratio` correctly computed for Hex2D x Line1D compositions.

### System Stress Tests

15. Tick budget death spiral: 2x obs load at 80% utilization -> overrun rate converges (not hockey-sticks).
16. ObsPlan mass invalidation: 200 plans invalidated -> throughput recovers to 50% within 500ms.
17. Stale action rejection oscillation: 50 agents under degraded tick rate -> rejection CV < 0.3.

---

## 24. Remaining Risks and Open Items

### Open Design Decisions

| # | Item | Status | Notes |
|---|------|--------|-------|
| CR-1 | ProductSpace composition semantics | **Specified with worked examples** (section 11.1) | Resolved in v3.0 |
| C-5 | Determinism verification strategy | **Partially addressed** (section 19.1) | R-DET-6 requires living catalogue; full strategy during implementation |
| C-8 | Full C ABI error code enumeration | **Minimum set defined** (section 9.7) | Full enum completed during implementation |
| I-6 | Graceful shutdown/lifecycle | **Resolved** (Design Decisions v3.1, Decision E) | Mode-specific: Lockstep = trivial Drop; RealtimeAsync = 4-state drain-then-join (Running→Draining→Quiescing→Dropped), ≤300ms, reuses §8.3 machinery |
| Q-4 | FlatBuffers schema evolution | **Direction set** (section 16.3) | Details during implementation |
| CR-NEW-1 | ProductSpace distance metric vs neighbours | **Resolved** (v3.0.1) | L1 graph geodesic as default; dual API with configurable metric_distance() |
| CR-NEW-2 | P-2 tick time vs RealtimeAsync TTL | **Resolved** (v3.0.1) | Tick-based authoritative TTL in both modes; wall-clock as ingress convenience only |

### Remaining Risks

| # | Risk | Severity | Mitigation |
|---|------|----------|------------|
| 1 | **Reference profile undefined** — all performance budgets unvalidatable without concrete cell/field/agent counts | Critical | Define before implementation (section 20, R-PERF-3) |
| 2 | **Scale behavior >100K cells** — architecture validated at 10K cells; 100K+ untested | Medium-High | Reference profile MUST include stress scenario |
| 3 | **Arena memory fragmentation** — long RealtimeAsync runs may fragment segmented arena | Medium | Periodic compaction during low-load; needs profiling |
| 4 | **Field->chunk arena migration for v1.5 LOD** — touches 4 subsystems | Medium | `FieldStorage` trait provides abstraction boundary |
| 5 | **`&self` propagator adoption friction** — ECS users expect mutable state | Low | 3 reference propagators covering common patterns; migration guide |

---

## Appendix A: Review Traceability

This section maps the architectural review's critical findings to their resolution in this document.

| Review Finding | Severity | Resolution | HLD Section |
|---------------|----------|------------|-------------|
| CR-1: ProductSpace semantics undefined | Critical | Specified: per-component neighbours, L1 graph-geodesic distance (v3.0.1), lexicographic iteration | 11.1 |
| CR-2: Egress threading model | Critical | Mode duality: inline for Lockstep, thread pool for RealtimeAsync | 7, 8 |
| CR-3: Snapshot creation strategy | Critical | Arena-based generational allocation | 5 |
| CR-4: No error model | Critical | Tick atomicity, all-or-nothing rollback, per-subsystem error semantics | 9 |
| C-5: No determinism verification | Critical | CI replay test specified; full strategy deferred to implementation | 19.1 |
| C-6: Propagator interface undefined | Critical | Concrete trait with StepContext, WriteMode, pipeline validation | 15 |
| C-7: ObsPlan mass invalidation | Critical | Plan-to-snapshot generation matching, fallback snapshot selection | 16, R-OBS-6 |
| C-8: C ABI error model | Critical | Handle lifecycle invariants, error codes, thread safety contracts | 9.6, 18 |
| C-9: Lockstep deadlock | Critical | Cannot occur: step_sync() is synchronous | 7.1 |
| P-1: Egress Always Returns | Principle | Adopted as normative principle | 3 |
| P-2: Tick-expressible time | Principle | Adopted as normative principle; TTL made tick-based in both modes (v3.0.1) | 3, R-DET-4 |
| P-3: Asymmetric dampening | Principle | Adopted, reflected in mode-specific designs | 3, R-ACT-2 |
| P-4: CoW dependency graph | Principle | All four properties satisfied by arena model | 5.4 |
| I-1: Stale action rejection oscillation | Important | Adaptive max_tick_skew with backoff | R-ACT-2 |
| I-2: Hex region bounding box strategy | Important | Bbox + validity mask, padding math quantified | 12.3 |
| I-3: TTL must be tick-based | Important | P-2 + R-DET-4; tick-based in both modes (v3.0.1) | 3, 19, R-DET-4 |
| I-4: Replay log format unspecified | Important | Minimum format specified | 14.4 |
| I-5: Collapse 3 generation IDs to 1 | Important | Single world_generation_id for v1 | 21.1 |
| I-6: No graceful shutdown/lifecycle | Important | **Resolved** (Design Decisions v3.1, Decision E) — mode-specific shutdown protocol | 9, 24 |
| I-7: ProductSpace padding explosion | Important | valid_ratio reporting, 35% threshold | R-OBS-7 |
| I-8: Acceptance criteria not testable | Important | Partially addressed (verification methods added to key criteria) | Throughout |
| I-9: No concurrency contract for ObsPlan | Important | Resolved by mode duality + epoch-based reclamation | 7, 8 |
| I-10: ObsSpec malformed input | Important | Validation at compilation with error codes per failure class | R-OBS-3 |
| I-11: Representative load undefined | Important | Reference profile required with concrete counts | R-PERF-3 |
| I-12: Partial acceptance semantics | Important | Batch-level: per-command accept/reject with receipt per command | R-ARCH-2 |
| I-13: Missing field lifecycle | Important | Fields static at world creation (R-FIELD-4) | 13, R-FIELD-4 |
| I-14: Snapshot refcount contention | Important | Resolved by epoch-based reclamation (no refcount) | 17 |

### Architectural Review Open Questions

| Question | Resolution | HLD Location |
|----------|-----------|-------------|
| Q-1: Which ProductSpace compositions are v1-tested? | Capped at 3 components | R-SPACE-4.1 |
| Q-2: Runtime-N vs const-generic? | Const-generic internal optimizations, runtime-N public API | R-SPACE-0 |
| Q-3: Square8 distance metric? | Chebyshev (consistent with 8-connected) | R-SPACE-5 |
| Q-4: FlatBuffers schema evolution? | Versioned schemas, forward-compatible additions | 16.3 |
| Q-5: C ABI versioning mechanism? | `murk_abi_version()` function | R-FFI-1 |
| Q-6: Propagator conflict granularity in ProductSpace? | Whole-field for v1, per-component v1.5 | R-SPACE-12 |

### Domain Expert Design Decisions

All 17 unanimous decisions from the domain expert review are incorporated:

| # | Decision | HLD Location |
|---|----------|-------------|
| 1 | Arena-based generational allocation | Section 5 |
| 2 | Field-level granularity, chunk-aware API for v1.5 | Section 13, R-FIELD-3 |
| 3 | Lockstep = callable struct, RealtimeAsync = autonomous thread | Section 7 |
| 4 | ObsPlan always executes against &Snapshot | Section 16, R-OBS-6 |
| 5 | Propagator &self + split-borrow StepContext | Section 15, R-PROP-1 |
| 6 | Sequential-commit default + opt-in reads_previous | Section 15, R-PROP-2 |
| 7 | Pre-allocated scratch via StepContext bump region | Section 15, R-PROP-1 |
| 8 | Branch-free ObsPlan with precomputed index mappings | Section 16.1 |
| 9 | Arena published/staging split-borrow (zero unsafe) | Section 5.3 |
| 10 | WriteMode: Full/Incremental | Section 15, R-PROP-1 |
| 11 | FieldMutability: Static/PerTick/Sparse | Section 13, R-FIELD-3 |
| 12 | dt_range on Propagator trait | Section 15, R-PROP-4 |
| 13 | Mode-specific performance budgets | Section 20 |
| 14 | Fallback snapshot selection for topology changes | Section 16.2 |
| 15 | Batched step_vec C API for vectorized RL | Section 18, R-FFI-4 |
| 16 | Reward/done as propagator-computed fields | Section 15.3 |
| 17 | Interior/boundary dispatch for agent-relative obs | Section 16.1 |
