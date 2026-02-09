# Murk DESIGN.md v2.5 — Consolidated Design Review

**Date:** 2026-02-09
**Review team:** Architecture Critic, Quality Engineer, Systems Thinker
**Facilitated by:** Team Lead

---

## Overall Verdict: REQUEST CHANGES (Unanimous)

The three-interface model (Ingress/TickEngine/Egress) is structurally sound and the right decomposition for an authoritative simulation engine. The design demonstrates real engineering maturity — generation-ID invalidation, deterministic ordering provenance, and the immutable snapshot contract are all strong foundations.

However, the spec is thorough on *what* but dangerously thin on *how*. Critical mechanism decisions are deferred, the entire document is happy-path only, and several subsystems lack the specification depth needed to begin implementation. Implementing against this spec as-is would produce a system that is difficult to test, dangerous to operate, and structurally constrained at v1.5.

**The design needs 9 critical changes before implementation can begin.**

---

## 0. Cross-Review Emergent Principles

The following principles emerged from the cross-challenge debate between reviewers. They were not in any individual review — they were produced by the discussion itself.

### P-1. "Egress Always Returns" (Architect, endorsed by all three)

> **WorldEgress MUST always return a response within a bounded time when a snapshot of any generation is available. Responses MAY indicate staleness, degraded coverage, or plan invalidation via metadata, but MUST NOT block indefinitely or return no data.**

This single normative sentence closes three critical issues simultaneously:
- **Lockstep deadlock** (breaks the circular wait — agents never block on obs)
- **ObsPlan invalidation blackout** (stale-but-consistent plans serve data with metadata)
- **Observation starvation in death spiral** (agents always get *some* data)

Highest leverage-to-spec-effort ratio of any recommendation.

### P-2. "All Engine-Internal Time References Must Be Tick-Expressible" (QA, endorsed by all three)

> **All engine-internal time references that affect state transitions MUST be expressible in tick-count.**

Covers: TTL, age_ms, lockstep timeouts, and any future time-dependent logic. A single normative sentence in §14 that prevents an entire class of replay-divergence bugs (the TTL-Replay Divergence loop identified during cross-review).

### P-3. Asymmetric Mode Dampening (Architect + Systems Thinker)

The staleness death spiral requires different fixes per mode:
- **RealtimeAsync**: Egress worker threads + adaptive `max_tick_skew` with exponential backoff
- **Lockstep**: Synchronous observation delivery at tick boundary (eliminates the staleness loop entirely — stale obs cannot occur)

A uniform dampening mechanism is wrong. The modes have fundamentally different dynamics.

### P-4. CoW/Structural Sharing as a Dependency Graph (Architect + Systems Thinker)

The snapshot strategy (CR-3) is not an isolated performance decision. It gates four downstream properties:

```
CoW/Structural Sharing
  ├── Snapshot publish overhead (direct: meets 3% budget)
  ├── Tick rollback on propagator error (enables error recovery — CR-4)
  ├── Concurrent egress reads (enables threading model — CR-2)
  └── Future parallel propagators (v1.5 migration path — R-MIG-1)
```

**If CoW → all four enabled. If full clone → all four compromised.** This should be presented to the document owner with this dependency graph, not as an isolated performance item.

---

## 1. Consensus Risks (All Three Reviewers Flagged)

These issues were independently identified by all three reviewers. They represent the highest-confidence findings.

### CR-1. ProductSpace Composition Semantics Undefined
**Severity: CRITICAL | Flagged by: All three**

The single biggest gap in the spec. ProductSpace (§7 R-SPACE-4) is defined as a concept but never specified as a mechanism. Missing:
- `neighbours()` for composed coordinates — is it per-component? Cross-component?
- `distance()` for product coordinates — L1? L-inf? Component-weighted?
- Iteration ordering across components — lexicographic? Component-major?
- Region queries that span components
- Propagator read/write conflict granularity in product coordinates

Every propagator, every ObsPlan, and every replay log depends on these answers. Without them, independent implementers will make incompatible choices.

**Recommended change:** Add §7.5 "ProductSpace Composition Semantics" defining all five items above with worked examples for Hex2D × Line1D.

---

### CR-2. Egress Threading Model Must Be Explicit
**Severity: CRITICAL | Flagged by: All three**

The spec implies concurrent snapshot reads (R-ARCH-1: "safe for concurrent reads") but never commits to an egress threading model. This matters because:
- If ObsPlan runs on the TickEngine thread, the 200 obs/sec target at 60Hz is **physically impossible** (3.3 obs per tick, each budgeted at p99 ≤ 5ms = 16.5ms just for obs)
- The systems thinker identified two reinforcing loops (staleness spiral + complexity squeeze) that have no governor without separate egress threads
- The architect confirmed: egress separation is **surgically feasible** with zero architectural changes — the immutable snapshot contract already supports it

**Recommended change:** Add §4.2 "Physical Threading Model" making explicit: (a) TickEngine owns one thread, (b) Egress executes ObsPlans on a separate thread pool, (c) snapshot lifetime managed by epoch-based reclamation, not refcount eviction.

---

### CR-3. Snapshot Creation Strategy Missing
**Severity: CRITICAL | Flagged by: All three**

§15 specifies a 3% tick-time budget for snapshot publication but provides no mechanism. For a 512³ voxel world with 10 fields, naive cloning is gigabytes — the budget is impossible.

The architect identified this as **load-bearing for v1.5 scalability**: CoW/structural sharing keeps the concurrent-propagator path open for v1.5, while full-clone creates a rewrite cliff (contradicting R-MIG-1). The QA engineer identified that refcount-pinned snapshots can't be evicted, making the "bounded by count/bytes" policy unenforceable.

**Recommended change:** (a) Specify CoW/structural sharing as the v1 snapshot strategy, (b) replace refcount eviction with epoch-based reclamation, (c) define default ring size K and byte budget estimation formula, (d) define behavior when slow consumers block eviction.

---

### CR-4. No Error Model Anywhere in the Document
**Severity: CRITICAL | Flagged by: QA + Architect (adopted), Systems Thinker (implicit in death spiral)**

The entire spec is happy-path. No subsystem has defined failure behavior:
- Propagator panics/NaN: abort tick? skip? poison?
- Snapshot creation failure (OOM): stall? partial publish?
- ObsPlan mid-execution failure: partially filled caller buffer?
- Queue overflow in Lockstep mode: drops break determinism
- C ABI use-after-destroy/double-destroy: undefined behavior?

**Recommended change:** Add §X "Error Model and Recovery" defining: (a) tick atomicity — propagator failure aborts tick, rolls back to previous snapshot, (b) snapshot failure policy, (c) partial ObsPlan execution semantics, (d) ingress overflow per mode (Lockstep drops must be deterministic), (e) C ABI handle lifecycle invariants.

---

## 2. Critical Issues (Unique to One or Two Reviewers)

### C-5. No Determinism Verification Strategy
**Severity: CRITICAL | Source: QA Engineer**

Tier B determinism (§14) is the most important property for Lockstep/replay, yet there is zero guidance on verification:
- No replay comparison protocol (compare what? at what granularity?)
- No CI regression test specification
- No catalogue of non-determinism sources (HashMap ordering, float reassociation, allocation-dependent sort stability)
- Build metadata recording is "recommended" but not MUST

**Recommended change:** Add §14.4 "Determinism Verification" specifying: (a) mandatory CI replay-and-compare test with bit-exact snapshot comparison at tick N, (b) minimum replay length, (c) catalogue of known non-determinism sources and mitigations, (d) promote build metadata to MUST.

---

### C-6. Propagator Interface Undefined
**Severity: CRITICAL | Source: Architect**

The most important user extension point has no trait, no function signature, no lifecycle, no error model. §11 gives 3 vague requirements while §12 (ObsPlan) gets 6 detailed ones. Without a concrete interface, propagators can't be implemented, tested, or validated.

**Recommended change:** Define the propagator trait in §11: (a) `fn step(&self, ctx: &mut PropagatorContext, dt: f64) -> Result<(), PropagatorError>`, (b) `fn declared_reads() -> FieldSet`, (c) `fn declared_writes() -> FieldSet`, (d) lifecycle (init/step/teardown), (e) error semantics (ties to CR-4).

---

### C-7. ObsPlan Mass Invalidation Cascade
**Severity: CRITICAL | Source: Systems Thinker, validated by Architect + QA**

Any topology/config change bumps generation IDs → ALL cached ObsPlans invalidated simultaneously → mass recompilation spike → observation blackout → feeds the staleness death spiral.

The architect confirmed the fix is safe: **match plan generation against snapshot generation, not world's current generation.** A plan compiled at generation G is valid for any snapshot produced at G, even after the world advances to G+1. The QA engineer confirmed this is testable and provided a test design with pass/fail criteria.

**Recommended change:** (a) R-OBS-6 should specify plan-to-snapshot generation matching (not plan-to-world), (b) add rate-limited recompilation as a SHOULD requirement, (c) add invalidation cascade test to mandatory v1 test set.

---

### C-8. C ABI Error Model Dangerously Underspecified
**Severity: CRITICAL | Source: QA Engineer**

§13 says "explicit error model" but provides no error codes, no handle lifecycle invariants, no thread safety contracts, no error string ownership model. This is a trust boundary where bugs become security vulnerabilities.

**Recommended change:** Define (a) handle lifecycle invariants (use-after-destroy returns error code, double-destroy is safe no-op), (b) thread safety contract per handle type, (c) error string ownership (engine-owned, valid until next call on same handle), (d) minimum error code enumeration.

---

### C-9. Lockstep Deadlock in Primary RL Use Case
**Severity: CRITICAL (upgraded from Important during cross-review) | Source: All three**

Egress blocks for future tick → TickEngine waits for Lockstep command → agent waits for observation → circular dependency. This is not an edge case — the observe→decide→act loop is the **primary Lockstep use case** (RL training, §3). All three reviewers independently identified this; the systems thinker matched it to the **Escalation** archetype.

**Recommended change:** Adopt P-1 ("Egress Always Returns") which breaks the circular wait. Lockstep egress returns the latest available snapshot with staleness metadata rather than blocking indefinitely.

---

## 3. Important Issues (Prioritized)

| # | Issue | Source | Recommendation |
|---|-------|--------|----------------|
| I-1 | **Stale action rejection oscillation** — reject→retry→more load→more rejection. "Fixes that Fail" archetype. | Systems + Architect | Add adaptive `max_tick_skew` with backoff (asymmetric per mode — see P-3) as SHOULD in R-ACT-2 |
| I-3 | **TTL must be tick-based in Lockstep** — wall-clock TTL is non-deterministic, violates Tier B during replay | QA (adopted by Architect) | Specify TTL unit per mode: tick-count for Lockstep, wall-clock for RealtimeAsync |
| I-4 | **Replay log format unspecified** — without format, can't write replay tests; without tests, can't verify Tier B. Circular dependency. | QA + Architect | Define minimum replay log record format in §14 |
| I-5 | **Collapse 3 generation IDs to 1 for v1** — reduces invalidation surface, simplifies caching. Split to 3 in v1.5 if profiling justifies it. | Systems + Architect (QA dissents on testability grounds) | Use single `world_generation_id` for v1; document split as planned v1.5 optimization |
| I-6 | **No graceful shutdown/lifecycle** — in-flight commands, pending ObsPlans, held snapshot references on shutdown are unspecified | QA | Add shutdown ordering and resource cleanup requirements |
| I-7 | **ProductSpace padding explosion** — Hex2D × Hex2D ≈ 62-67% valid. No budget, no warning to ML consumers. | Systems + QA | Add `valid_ratio` to ObsPlan metadata; define v1 tested compositions with padding guarantees; mandate property test with ≥35% threshold |
| I-8 | **Acceptance criteria not testable** — many use qualitative language ("mechanical enforcement", "validate legality") without verification methods | QA | Each criterion needs: verification method, quantitative threshold, failure condition |
| I-9 | **No concurrency contract for ObsPlan execution** — can multiple plans run simultaneously? ABA on snapshot swap? | QA | Resolved by CR-2 (explicit threading model) + epoch-based reclamation |
| I-10 | **ObsSpec malformed input not addressed** — non-existent fields, out-of-bounds regions, overflow output shapes. Trust boundary via C ABI. | QA | Add validation requirements at ObsSpec compilation with error codes per failure class |
| I-11 | **"Representative load" undefined for benchmarks** — 60Hz is easy with 100 cells, hard with 10M cells | QA | Define at least one concrete reference scenario in spec |
| I-12 | **Partial acceptance semantics undefined** — batch partial? single command partial? receipt format? | QA | Define in §4 R-ARCH-2 |
| I-13 | **Missing field lifecycle** — can fields be added/deleted at runtime? What happens to referencing propagators/ObsPlans? | QA | Define field mutability rules; tie to generation ID invalidation |
| I-14 | **Snapshot refcount contention** — 200 obs/sec atomic ops = cache-line ping-pong | Systems | Resolved by CR-3 (epoch-based reclamation) |

---

## 4. Open Questions Requiring Design Decisions

| # | Question | Raised by | Options | Recommendation |
|---|----------|-----------|---------|----------------|
| Q-1 | ProductSpace: which compositions are v1-tested vs "allowed but untested"? | Architect + QA | (a) Test all, (b) Define a tested set like N≤5 | Define tested set; cap at 3 components for v1 |
| Q-2 | Runtime-N vs const-generic for common 2D/3D cases? | Architect | (a) Runtime only, (b) Const-generic fast paths | Const-generic specializations as internal optimization, runtime-N as public API |
| Q-3 | Square8 distance metric: Chebyshev or Euclidean? | QA | (a) Chebyshev, (b) Euclidean, (c) Both available | Chebyshev (consistent with 8-connected semantics); document trade-off |
| Q-4 | FlatBuffers schema evolution strategy for ObsSpec? | Architect + QA | (a) Versioned schemas, (b) Forward-compatible only | Define schema versioning policy before v1 ships |
| Q-5 | C ABI versioning mechanism? | QA | (a) Semantic versioning, (b) ABI version in handle, (c) Symbol versioning | ABI version number exposed via `murk_abi_version()` function |
| Q-6 | Propagator conflict granularity in ProductSpace? | Architect + Systems | (a) Whole-field, (b) Per-component, (c) Per-region | Whole-field for v1; per-component as v1.5 optimization |

---

## 5. System Dynamics (Cross-Review Final)

### 5.1 Feedback Loop Inventory

The cross-review identified 8 feedback loops — 4 ungoverned reinforcing loops and 1 missing balancing loop:

| Loop | Type | Governor? | Fix |
|------|------|-----------|-----|
| **R1: Staleness spiral** | Reinforcing | No | Egress concurrency (CR-2) + adaptive skew (I-1) |
| **R2: Complexity squeeze** | Reinforcing | No | Egress concurrency (CR-2) |
| **R3: Feature demand** | Reinforcing | No (long-term) | ProductSpace composition caps (Q-1) |
| **R4: TTL-replay divergence** | Reinforcing | No | Tick-based time refs (P-2) |
| B1: Ingress backpressure | Balancing (degraded) | Bad equilibrium | Stability metrics (new R-PERF-X) |
| B2: Ring eviction | Balancing | Partially | Epoch-based GC (CR-3) |
| B3: Mass invalidation | Triggers R1 | No | Graceful degradation ramp (C-7) |
| **B4: Adaptive LOD** | **MISSING** | Doesn't exist | Dynamic LOD load-shedding (SHOULD v1, MUST v1.5) |

**Dominant dynamic:** The system transitions through three phases:
1. **Phase 1 (v1 launch)**: Stable, B1 dominates, tick budget has headroom
2. **Phase 2 (v1 maturity)**: R2 begins to dominate, occasional overruns
3. **Phase 3 (v1.5+)**: R1 dominates, system oscillates — **needs redesign, not optimization**

### 5.2 System Archetypes (9 identified, cross-validated)

| Archetype | Location | Intervention |
|-----------|----------|-------------|
| **Limits to Growth** | Single-thread tick budget vs feature expansion | Explicit egress concurrency (CR-2) |
| **Growth and Underinvestment** | 6 obs requirements, 0 execution model requirements | Physical Threading Model in §4 (CR-2) |
| **Shifting the Burden (1)** | ObsPlan caching masks concurrency need | Address fundamental: concurrency |
| **Shifting the Burden (2)** | Sequential propagators vs validated parallel | Spec R-PROP-2 acceptance criteria |
| **Fixes that Fail (1)** | Stale action rejection → retry storms | Adaptive max_tick_skew (I-1, P-3) |
| **Fixes that Fail (2)** | TTL-replay divergence | Tick-based time references (P-2) |
| **Escalation** | Lockstep deadlock cycle | "Egress Always Returns" (P-1, C-9) |
| **Success to the Successful** | Snapshot ring fast/slow consumers | Epoch-based GC (CR-3) |
| **Tragedy of the Commons** | ProductSpace tensor padding | Padding budget + valid_ratio (I-7) |

9 distinct archetype matches is unusually high for a single system. This indicates significant structural tensions that will produce emergent failures under real-world conditions. The good news: the interventions are well-understood for each archetype.

---

## 6. Mandatory v1 Test Set (Cross-Reviewer Consensus)

These tests MUST exist before v1 ships. Derived from all three reviews:

### Unit / Property Tests
1. Hex2D canonical iteration ordering for all region shapes
2. ProductSpace region query for Hex2D × Line1D
3. ProductSpace padding ratio ≥ 35% for all v1-tested compositions
4. Propagator write-write conflict detection at startup
5. Command deterministic ordering under concurrent multi-source ingress

### Integration Tests
6. Determinism replay: identical scenario twice → bit-exact snapshot at tick N
7. ObsPlan generation invalidation → `PLAN_INVALIDATED` error
8. Snapshot ring eviction → `NOT_AVAILABLE` response
9. Ingress backpressure → deterministic drop behavior
10. Hex → rectangular tensor export with validity mask for known geometries
11. C ABI handle lifecycle: create/destroy in all valid and invalid orderings

### System Stress Tests
12. Tick budget death spiral: 2× obs load at 80% utilization → overrun rate converges (not hockey-sticks)
13. ObsPlan mass invalidation: 200 plans invalidated → throughput recovers to 50% within 500ms
14. Stale action rejection oscillation: 50 agents under degraded tick rate → rejection CV < 0.3

---

## 7. Prioritized Change List (Final — Cross-Review Refined)

### Phase 0: Normative Principles (Add to spec preamble — highest leverage)
1. **P-1** — "Egress Always Returns" requirement in R-ARCH-2 (closes C-9 deadlock, C-7 blackout, R1 starvation)
2. **P-2** — "Tick-expressible time references" principle in §14 (closes R4, prevents entire class of replay bugs)

### Phase 1: Unblock Implementation (Before any code)
3. **CR-3** — Specify CoW/structural sharing for snapshots + epoch-based reclamation (§15) — **this is the most load-bearing decision; gates CR-2, CR-4 rollback, and v1.5 migration (see P-4 dependency graph)**
4. **CR-2** — Define egress threading model in §4.2 "Physical Threading Model"
5. **CR-4** — Add error model (new section) — tick atomicity, propagator rollback (enabled by CoW)
6. **C-6** — Define propagator trait (§11)

### Phase 2: Unblock Subsystem Design (Before subsystem implementation)
7. **CR-1** — Specify ProductSpace composition semantics (§7.5)
8. **C-7** — Fix ObsPlan generation matching to snapshot-based + rate-limited recompilation (§12 R-OBS-6)
9. **C-5** — Add determinism verification strategy (§14.4)
10. **C-8** — Specify C ABI error model and handle lifecycle (§13)

### Phase 3: Unblock Testing (Before v1 validation)
11. **I-1** — Asymmetric stale action dampening (P-3): adaptive skew for RealtimeAsync, sync delivery for Lockstep
12. **I-3 + I-4** — TTL tick-based units + replay log format (unblocks determinism CI tests)
13. **I-5** — Collapse to single `world_generation_id` for v1
14. **I-7** — ProductSpace padding budget + `valid_ratio` reporting
15. **I-8** — Make acceptance criteria testable with verification methods
16. **I-11** — Define reference benchmark scenario
17. Remaining important issues as capacity allows

### Cross-Review Top 3 Interventions by Loop Coverage
These three changes address 7 of 8 feedback loops and mitigate all 9 archetypes:
1. **Explicit egress concurrency** — addresses R1, R2, unlocks B4
2. **Graceful ObsPlan degradation with rate-limited recompilation** — addresses B3→R1 cascade
3. **R-PERF-X stability metrics** (monotonic-increase detection for all reinforcing loops) — detects all spirals

---

## Appendix A: Reviewer Confidence and Caveats

- **Overall confidence: High (85-90%)**. The document is detailed enough for thorough review. Gaps identified are structural, not ambiguities in reading. Loop identification and archetype matching cross-validated by all three reviewers.
- **Risk assessment: Medium-High**. The missing error model and threading model are the kind of gaps that become architectural debt — expensive to retrofit.
- **Information gap**: No source code exists yet, so we cannot assess whether implementation already addresses some concerns.
- **Prior review concern** (Architect): The architect notes these same critical issues (ProductSpace, snapshot strategy, ObsPlan threading, propagator interface) were flagged in a prior review round. If v2.5 incorporated that feedback, the incorporation is not visible. The systems thinker identifies this as a **Drifting Goals** dynamic.
- **Dissent**: QA engineer dissents on I-5 (generation ID collapse), arguing 3 IDs are more independently testable. The majority position (Architect + Systems) favors 1 ID for v1 simplicity. Both positions are defensible.

## Appendix B: Cross-Review Process Observation

The three-reviewer cross-challenge model produced materially better results than individual reviews alone:

- The systems thinker's migration lock-in finding was correctly refined from "critical" to "conditional on snapshot strategy" by the architect
- The QA engineer's TTL catch (R4 loop) was invisible to both the architect and systems thinker
- The systems thinker's loop analysis gave the architect's egress threading recommendation structural justification for #1 priority
- The architect's "Egress Always Returns" principle unified four separate findings into one spec sentence
- The Lockstep deadlock was independently flagged by all three, but only upgraded to critical through cross-challenge (the systems thinker showed it maps to the primary RL use case)

Each reviewer's blind spots were covered by another's strengths. Worth considering as a repeatable process for future design reviews.
