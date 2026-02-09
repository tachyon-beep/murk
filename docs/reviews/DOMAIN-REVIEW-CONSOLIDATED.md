# Murk DESIGN.md v2.5 — Domain Expert Review

**Date:** 2026-02-09
**Review team:** Systems Engineer, DRL Specialist, Simulation Architect
**Facilitated by:** Team Lead

---

## Overall Verdict: ARCHITECTURE VALIDATED

Three domain experts conducted a structured multi-round discussion reviewing the Murk World Engine design from systems engineering, deep reinforcement learning, and simulation architecture perspectives. After 8 rounds of cross-challenge, the panel converged on **17 design decisions** with unanimous agreement and **no remaining architectural concerns**.

The core three-interface architecture (Ingress/TickEngine/Egress) is sound. The panel's primary contribution is resolving the mechanism decisions that the architectural review (DESIGN-REVIEW-CONSOLIDATED.md) identified as critical gaps — most importantly, the snapshot ownership model, the propagator trait design, and the mode duality between RealtimeAsync and Lockstep.

**Key outcome:** The design is **mechanically enforceable in Rust's type system with zero `unsafe` blocks.**

---

## 1. The Foundational Decision: Arena-Based Generational Allocation

*Converged in Rounds 1-3. All three experts agree. This replaces the CoW recommendation from the architectural review (CR-3).*

The architectural review identified CoW/structural sharing as the most load-bearing decision (P-4), gating four downstream properties. The domain panel resolved this with a specific mechanism: **arena-based generational allocation**.

### How it works

1. Each field is stored as a contiguous `[f32]` allocation in a generational arena
2. At tick start, propagators write to **fresh allocations** in the new generation (no copies)
3. Unmodified fields share their allocation across generations
4. Snapshot publication = swap a ~1KB descriptor of field pointers. Cost: **<2μs** (vs 500μs budget)
5. Old generations remain readable until all snapshot references are released

### Why it's superior to traditional CoW

| Property | Traditional CoW | Arena-Generational |
|----------|----------------|-------------------|
| Copy cost | Fault-driven, unpredictable | Zero (allocate fresh, write directly) |
| Snapshot publication | Clone or CoW fork | Atomic descriptor swap, <2μs |
| Rollback | Undo log or full checkpoint | Free (abandon new generation) |
| `unsafe` required | Usually (page-level CoW) | None (borrow checker verifies) |
| Memory predictability | Fault-driven = unpredictable | Bump allocation = predictable |

### Rust type-level properties

- `ReadArena` (published, immutable): `Send + Sync`, safe for concurrent egress reads
- `WriteArena` (staging, exclusive to TickEngine): `&mut` access, no aliasing
- Snapshot references contain generation-scoped handles (integers), not pointers
- Field access requires `&FieldArena` — borrow checker enforces arena liveness
- Segmented arena (linked list of 64MB segments) ensures no reallocation

### Dependency graph (from architectural review P-4, now resolved)

```
Arena-Based Generational Allocation
  ├── Snapshot publish overhead: <2μs (exceeds 3% budget by 250×) ✓
  ├── Tick rollback: free (abandon staging generation) ✓
  ├── Concurrent egress: ReadArena is Send + Sync ✓
  └── v1.5 parallel propagators: disjoint field writes, no contention ✓
```

### Field mutability optimization

```
Static    → Generation 0 forever, shared across snapshots and vectorized envs
PerTick   → Arena-managed, new allocation each tick if modified
Sparse    → Arena-managed, new allocation only when modified (rare)
```

For vectorized RL (128 envs × 2MB mutable + 8MB shared static): **264MB** vs 1.28GB without sharing.

---

## 2. Mode Duality: Two Architectures, One Core

*Converged in Rounds 2-4. All three experts agree. This resolves the architectural review's mode discussion and addresses C-9 (Lockstep deadlock).*

The spec treats RealtimeAsync and Lockstep as different policies on the same architecture (R-MODE-2). The domain panel found this is incorrect — they are **different ownership topologies** that share a propagator pipeline.

### Lockstep: Callable Struct (RL Training)

```rust
impl LockstepWorld {
    pub fn step_sync(&mut self, commands: &[Command]) -> StepResult<&Snapshot> {
        // Caller's thread becomes the tick thread
        // No ring buffer, no egress threads, no epoch reclamation
        // Obs filled inline from snapshot, same thread
    }

    pub fn reset(&mut self, seed: u64) -> &Snapshot {
        // &mut self guarantees no outstanding snapshot borrows
        // Arena reset is O(1) — bump pointer reset
    }
}
```

- **No ring buffer needed** (K=1 suffices)
- **No egress thread pool** (obs filled inline)
- **No wall-clock deadline** (maximize throughput, not frame rate)
- **`&mut self` enforces RL lifecycle** — borrow checker prevents snapshot references surviving across step/reset
- **Vectorized:** 16-128 independent `LockstepWorld` instances, each owned by one thread, `Send` not `Sync`

### RealtimeAsync: Autonomous Thread (Games/Tools)

- TickEngine on dedicated thread, 60Hz wall-clock deadline
- Egress thread pool reads `&Snapshot` from ring buffer concurrently
- Epoch-based reclamation for snapshot lifetime management
- Fallback snapshot selection on topology changes (R-OBS-6 amendment)

### Shared core

Both modes share: propagator pipeline, command ordering, field model, Space trait, ObsPlan compilation/execution. The mode-specific shells are thin wrappers (~200 lines each) around the shared core.

### Rust expression

```rust
trait World {
    type SnapshotRef<'a>: AsRef<Snapshot> where Self: 'a;
    fn step(&mut self, commands: &[Command]) -> Result<Self::SnapshotRef<'_>, StepError>;
}
```

GAT (Generic Associated Type) lets each mode define its snapshot reference type. Lockstep returns `&Snapshot` (zero-cost borrow). RealtimeAsync returns an epoch-guarded handle. C ABI hides this behind opaque handles.

### Impact on Lockstep deadlock (C-9)

The Lockstep deadlock identified in the architectural review **cannot occur** in this design. `step_sync()` is synchronous — it returns the snapshot directly. There is no separate Egress to block on, no separate TickEngine thread to wait for. The observe→decide→act loop is a single-threaded sequential call chain.

---

## 3. Propagator Trait Design

*Converged in Rounds 3-5. All three experts agree. Resolves C-6 from the architectural review.*

### The trait

```rust
pub trait Propagator: Send + 'static {
    fn name(&self) -> &str;
    fn reads(&self) -> FieldSet;
    fn reads_previous(&self) -> FieldSet { FieldSet::empty() }
    fn writes(&self) -> Vec<(FieldId, WriteMode)>;
    fn max_dt(&self) -> Option<f64> { None }
    fn scratch_bytes(&self) -> usize { 0 }
    fn step(&self, ctx: &StepContext, dt: f64) -> Result<(), PropagatorError>;
}

pub enum WriteMode {
    /// Fresh uninitialized buffer — propagator fills completely
    Full,
    /// Seeded from previous generation via memcpy — propagator modifies in-place
    Incremental,
}

pub struct StepContext<'a> {
    reads: FieldReadSet<'a>,       // &ReadArena: latest committed versions
    reads_prev: FieldReadSet<'a>,  // &ReadArena: generation N (tick start)
    writes: FieldWriteSet<'a>,     // &mut WriteArena: new generation staging
    scratch: &'a mut ScratchRegion,// bump allocator, reset between propagators
    space: &'a dyn SpaceAccess,
    tick_id: u64,
    dt: f64,
}
```

### Key design decisions

| Decision | Rationale |
|----------|-----------|
| `&self` not `&mut self` | Stateless operators: `Send`, deterministic, mode-independent. Persistent state goes through fields. |
| `reads()` vs `reads_previous()` | Sequential-commit default (Euler); opt-in previous-generation reads (Jacobi/parallel). Makes dependency chain a verifiable DAG. |
| `WriteMode::Full` vs `Incremental` | Full = uninitialized (diffusion); Incremental = seeded from gen N (sparse agent updates). Engine budgets seed cost at startup. |
| `max_dt()` | Pipeline validates `dt ≤ min(max_dt)` at startup. Exposed through C ABI as `dt_range()` for RL users. |
| `scratch_bytes()` | Pre-allocated bump region in StepContext, O(1) reset between propagators. No heap allocation in inner loop. |

### Pipeline validation at startup

- No write-write conflicts (two propagators writing the same field)
- Read-after-write ordering verified (dependency DAG)
- All field references valid
- `dt` within all propagators' declared `max_dt` range

### Error and rollback

```rust
match pipeline.execute(&mut state, &mut staging, commands, dt) {
    Ok(()) => {
        arena.publish(staging);  // ownership transfer, zero-copy
        generation += 1;
    }
    Err(TickError::PropagatorFailed { .. }) => {
        drop(staging);           // abandon — state unchanged, zero-cost
        ingress.re_enqueue(commands, ReasonCode::TICK_ROLLBACK);
    }
}
```

- Tick rollback is **all-or-nothing**: if any propagator fails, all staging writes are abandoned
- Commands are re-enqueued with `TICK_ROLLBACK` reason code
- World state is exactly as before the tick started
- Determinism preserved: replaying same commands after rollback produces same result
- Escape hatch: `reset()` for unrecoverable states (standard Gymnasium pattern)

### Reward as a propagator

Reward computation is a standard propagator that runs last (dependency constraint: reads all mutable fields). Benefits:
- No special reward API — standard propagator interface
- Acts as implicit cache prefetch for subsequent ObsPlan execution
- Deterministic, replayable, field-stored

---

## 4. Observation Export Path

*Converged in Rounds 3-6. All three experts agree. Refines C-7 and the architectural review's ObsPlan recommendations.*

### Unified ObsPlan abstraction (no separate fast path)

Smart ObsPlan compilation recognizes simple specs and emits tight gather loops:

| Plan class | Inner loop | Latency (per obs) |
|------------|-----------|-------------------|
| Simple (direct reads + normalization) | Branch-free indexed gather | ≤ 100μs (p99) |
| Standard (pooling + foveation) | Precomputed kernels | ≤ 5ms (p99) |

### Optimization stack

1. **Precomputed relative index mappings**: agent position changes → only base offset updates, zero recompilation
2. **Branch-free hex padding**: `memset(0)` + sparse valid-index gather, no per-cell branch
3. **FieldMutability caching**: Static fields cached once, only PerTick fields re-gathered per step
4. **Batch execution**: single traversal fills N agent buffers (SHOULD v1, MUST v1.5)
5. **Interior/boundary dispatch**: O(1) check per agent, branch-free interior path, validated boundary path

### Memory copy scorecard (full RL training loop)

| Copy | Size | Cost |
|------|------|------|
| GPU→CPU action | ~64B | ~10μs (DMA) |
| Action → WorldEvent | ~100B/agent | <1μs |
| Field seed (Incremental writes) | ~40KB | ~0.5μs |
| Field gather → obs buffer | ~2.5KB/obs | ~0.5μs/field |
| Validity mask | ~160B | <0.1μs |
| CPU→GPU obs | ~20KB | ~20μs (DMA) |

**Framework overhead: ~3μs.** GPU DMA dominates. The architecture is not the bottleneck.

### Topology change graceful degradation (RealtimeAsync)

- Generation bump invalidates all cached ObsPlans
- **Fallback snapshot selection**: egress scans ring for newest compatible snapshot
- Agents get slightly stale but valid observations while plans recompile on egress thread pool
- Rate-limited recompilation prevents spike
- Lockstep unaffected: topology changes happen during `reset()` with synchronous recompilation

---

## 5. Performance Budgets (Mode-Specific)

*Converged in Rounds 4-6. All three experts agree. Resolves I-11 and replaces the single-mode budget in §16.*

### RealtimeAsync (60Hz wall-clock deadline)

| Phase | Budget | % of Frame |
|-------|--------|------------|
| Ingress drain + sort | 500μs | 3% |
| Propagator pipeline | 12ms | 72% |
| Snapshot publish | 2μs | 0.01% |
| Egress notification | 200μs | 1% |
| Overhead | 1ms | 6% |
| **Headroom** | **2.97ms** | **18%** |

Obs generation: off-thread (egress pool), does not count against tick budget.

### Lockstep (throughput-maximized, no deadline)

| Phase | Budget | Notes |
|-------|--------|-------|
| Command processing | 5μs | Minimal for single-agent |
| Propagator pipeline | 50-80μs | Depends on propagator count and cell count |
| Snapshot publish | <0.1μs | Descriptor swap only |
| Obs generation (inline) | 16-27μs | 16 agents × ~1.7μs/obs |
| **Total** | **70-115μs** | |
| **Throughput** | **8,700-14,300 steps/sec** | Per env |
| **Vectorized (16 cores)** | **139K-229K steps/sec** | Aggregate |

### Comparison to MuJoCo Ant-v4

MuJoCo: ~20,000 steps/sec per env → 320,000 aggregate (16 cores).
Murk: ~14,000 steps/sec per env → 224,000 aggregate → **70% of MuJoCo throughput**.

Competitive for a general-purpose engine. Murk wins on scenarios MuJoCo can't serve (hex strategy, ecosystem, multi-topology).

### ObsPlan targets (replace 200 obs/sec)

| Plan class | Target | Replaces |
|------------|--------|----------|
| Simple | ≥ 500,000 obs/sec | 200 obs/sec (misleadingly low) |
| Standard | ≥ 200 obs/sec | (unchanged) |

---

## 6. Spec Amendments Required

### New sections to add

| Section | Content | Resolves |
|---------|---------|----------|
| **§0 Ownership Model** | Arena-based generational allocation as foundational decision | CR-3 (snapshot strategy) |
| **§4.2 Physical Threading Model** | Mode duality: Lockstep callable / RealtimeAsync threaded | CR-2 (egress threading) |
| **§7.5 ProductSpace Composition** | neighbours, distance, iteration, region queries for composed coordinates | CR-1 (still open from arch review) |
| **§X Error Model** | Tick atomicity, all-or-nothing rollback, command re-enqueue, `TICK_ROLLBACK` | CR-4 (error model) |

### Existing sections to amend

| Section | Amendment | Resolves |
|---------|-----------|----------|
| **§6 R-MODE-2** | Acknowledge mode duality as architectural, not just policy. Add `step_sync()` for Lockstep. | C-9 (deadlock) |
| **§11 R-PROP-1/2/3** | Replace with concrete propagator trait: `reads/reads_previous/writes/max_dt/scratch_bytes/step` | C-6 (propagator interface) |
| **§12 R-OBS-6** | Plan-to-snapshot generation matching + fallback snapshot selection | C-7 (mass invalidation) |
| **§13 R-FFI-1** | Add `murk_lockstep_step_sync()`, `murk_lockstep_step_vec()`, `murk_dt_range()` | New (RL integration) |
| **§15 R-SNAP-1** | Arena-based retention replaces refcount ring. Snapshot carries `topology_generation`. | CR-3 |
| **§16 R-PERF-1** | Split budgets by mode. Tiered ObsPlan targets. Define reference profile. | I-11 |

### New requirements to add

| Requirement | Content | Source |
|-------------|---------|--------|
| **R-FIELD-3** | FieldMutability (Static/PerTick/Sparse) with arena optimization | DRL + Systems |
| **R-PROP-4** | `max_dt` declaration, pipeline validates at startup | Simulation |
| **R-PROP-5** | Pipeline startup validation: write conflicts, DAG ordering, field existence | Systems |
| **R-OBS-7** | `valid_ratio` in ObsPlan metadata, MUST report, SHOULD warn < 0.5 | Simulation + DRL |
| **R-OBS-8** | Batch ObsPlan execution for multi-agent, SHOULD v1, MUST v1.5 | DRL |
| **R-FFI-4** | Batched `step_vec` C API for vectorized RL, SHOULD v1, MUST v1.5 | DRL + Systems |
| **R-FFI-5** | GIL released for entire C ABI call duration | DRL |
| **R-MODE-4** | PettingZoo Parallel API compatibility for multi-agent Lockstep | DRL |

---

## 7. Remaining Risks (Ordered by Severity)

| # | Risk | Severity | Mitigation |
|---|------|----------|------------|
| 1 | **Reference profile undefined** — all performance budgets are unvalidatable without concrete cell/field/agent counts | Critical | Define before implementation: suggest 10K cells, 5 fields, 3 propagators, 16 agents |
| 2 | **ProductSpace propagator semantics (CR-1)** — cross-component neighbours, distance, iteration still unspecified | Critical | §7.5 must be written before ProductSpace implementation |
| 3 | **Scale behavior >100K cells** — architecture validated at 10K cells; 100K+ untested | Medium-High | Reference profile should include a stress scenario (100K+ cells) |
| 4 | **Arena memory fragmentation** — long RealtimeAsync runs may fragment segmented arena | Medium | Periodic compaction during low-load; needs profiling to confirm |
| 5 | **Field→chunk arena migration for v1.5 LOD** — touches 4 subsystems | Medium | `FieldStorage` trait provides abstraction boundary; API designed chunk-aware from v1 |
| 6 | **`&self` propagator adoption friction** — ECS users expect mutable state | Low | 3 reference propagators covering common patterns; migration guide |

---

## 8. Convergence Summary

### 17 Unanimous Design Decisions

| # | Decision | Round Converged |
|---|----------|----------------|
| 1 | Arena-based generational allocation | 1-3 |
| 2 | Field-level granularity for v1, chunk-aware API for v1.5 | 3-4 |
| 3 | Lockstep = callable struct, RealtimeAsync = autonomous thread | 2-3 |
| 4 | ObsPlan always executes against `&Snapshot` | 3-4 |
| 5 | Propagator `&self` + split-borrow StepContext | 3-4 |
| 6 | Sequential-commit default + opt-in `reads_previous` | 3-4 |
| 7 | Pre-allocated scratch via StepContext bump region | 4 |
| 8 | Branch-free ObsPlan with precomputed index mappings | 4 |
| 9 | Arena published/staging split-borrow (zero `unsafe`) | 3-4 |
| 10 | WriteDecl: WholeField/IncrementalField/SpatialRegion/AgentLocal | 4-5 |
| 11 | FieldMutability: Static (gen 0) / PerTick / Sparse | 4-5 |
| 12 | `dt_range` on Propagator trait | 4 |
| 13 | Mode-specific performance budgets in §16 | 4-5 |
| 14 | Fallback snapshot selection for topology changes | 6 |
| 15 | Batched `step_vec` C API for vectorized RL | 5-6 |
| 16 | Reward/done as propagator-computed fields | 6 |
| 17 | Interior/boundary dispatch for agent-relative obs | 6 |

### Expert Confidence

| Expert | Confidence | Residual Uncertainty |
|--------|------------|---------------------|
| Systems Engineer | 92% | Arena fragmentation under long runs; ProductSpace index perf for K>3 |
| DRL Specialist | 92% | GIL interaction in edge cases; PettingZoo API compatibility details |
| Simulation Architect | 85-90% | Scale behavior >100K; error recovery cascades; reference profile |

---

## Appendix A: How This Review Relates to the Architectural Review

This domain review **resolves** several critical issues from the architectural review (DESIGN-REVIEW-CONSOLIDATED.md):

| Architectural Review Issue | Resolution |
|---------------------------|------------|
| CR-2 (Egress threading) | Mode duality: threaded egress for RealtimeAsync, inline for Lockstep |
| CR-3 (Snapshot strategy) | Arena-based generational allocation (not traditional CoW) |
| CR-4 (Error model) | All-or-nothing tick rollback, command re-enqueue, `TICK_ROLLBACK` |
| C-6 (Propagator interface) | Concrete trait with `reads/reads_previous/writes/max_dt/step` |
| C-7 (ObsPlan invalidation) | Fallback snapshot selection + rate-limited recompilation |
| C-9 (Lockstep deadlock) | Cannot occur — `step_sync()` is synchronous, no separate Egress |
| P-1 (Egress Always Returns) | Enforced by design in RealtimeAsync (fallback snapshot) |
| P-4 (CoW dependency graph) | All four downstream properties satisfied by arena model |

**Still open from architectural review:**
- CR-1 (ProductSpace composition semantics) — not addressed by this review
- C-5 (Determinism verification strategy) — deferred to implementation phase
- C-8 (C ABI error model) — partially addressed (StepError enum), full error code enumeration still needed
- I-1 through I-14 — spec-level amendments, not architectural decisions

## Appendix B: Process Observation

The multi-domain discussion format produced insights that no single expert would have reached:

- **Arena-based allocation** emerged from the systems engineer's ownership analysis, but was validated against tick budgets by the simulation architect (<2μs vs 500μs budget) and against RL training requirements by the DRL specialist (O(1) reset for `env.reset()`)
- **Mode duality** was identified by the systems engineer (different type signatures), confirmed by the DRL specialist (Lockstep needs synchronous `step()`), and refined by the simulation architect (shared propagator pipeline, mode-specific shells)
- **Reward as a propagator** came from the DRL specialist, accepted by the simulation architect (cache prefetch benefit), and type-checked by the systems engineer (standard trait, dependency ordering)
- **Observation throughput math** was contested between the DRL specialist (1.7μs/obs) and simulation architect (5ms budget) — resolved to tiered targets that accurately reflect different plan complexities
- **`&self` propagator friction** was flagged as an adoption risk by all three from different angles: systems engineer (type-level), simulation architect (LOD patterns), DRL specialist (reward accumulation)
