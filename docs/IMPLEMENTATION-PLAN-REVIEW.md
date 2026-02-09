# Implementation Plan Review: CHANGES_REQUESTED

**Plan:** `/home/john/murk/docs/IMPLEMENTATION-PLAN.md`
**HLD:** `/home/john/murk/docs/HLD.md`
**Reviewed:** 2026-02-09
**Reviewers:** Reality, Architecture, Quality, Systems

---

## Verdict: CHANGES_REQUESTED

**Summary:** Plan has 6 blocking issues across 3 reviewers. Reality reviewer found no issues. Architecture identified 3 blockers (determinism versioning, ReadResolutionPlan under-specification, StepContext signature conflict). Quality identified 3 blockers (FlatBuffers fuzzing gap, graceful shutdown unspecified, two mandatory tests not explicitly covered). Systems identified 2 blockers (propagator failure retry loop, epoch reclamation failure mode exercise). After deduplication and conflict resolution, 7 distinct blocking issues remain (one Architecture+Systems overlap on epoch reclamation is consolidated).

---

## Blocking Issues (7) - Must Fix Before Execution

### B1. Determinism Compatibility Versioning Missing
**Source:** Architecture
**Priority Score:** 36 (Severity: Critical=4, Likelihood: Certain=3, Reversibility: Irreversible=3)

Three design choices are **compatibility boundaries** -- changing any of them breaks replay:
- ProductSpace lexicographic iteration order (R-SPACE-10)
- Hex2D canonical ordering r-then-q (HLD section 12.2)
- Command ordering tiebreak via `arrival_seq` (HLD section 14.1)

The plan has no mechanism to version these. If any boundary changes post-v1, all existing replay logs become silently invalid.

**Evidence:** HLD section 12.2 explicitly states "This ordering is part of the determinism contract and MUST be treated as a compatibility boundary." HLD section 19 requires Tier B determinism. Yet the replay log format (WP-13) and the implementation plan contain no versioning field for determinism-affecting choices.

**Resolution:** Add `determinism_version: u32` to:
1. Replay log header (WP-13 deliverables)
2. World descriptor / `WorldGenerationId` metadata
3. ObsPlan binding metadata

This is a small addition to WP-1 (add the field to core types) and WP-13 (include in replay format). Estimated effort: < 1 day. Impact if deferred: irreversible replay incompatibility on any boundary change.

---

### B2. ReadResolutionPlan Data Structure Not Specified
**Source:** Architecture
**Priority Score:** 24 (Severity: Critical=4, Likelihood: Certain=3, Reversibility: Difficult=2)

WP-5 lists "precomputed ReadResolutionPlan" as a deliverable and critical sub-task. The plan even states it is "~50-100 lines with the highest correctness criticality in the engine." Yet the plan does not specify:
- The data structure
- Construction algorithm
- Interaction with `StepContext<R, W>` generic parameters
- How overlay visibility chains work for N>2 propagators

**Evidence:** WP-5 deliverables (implementation plan line 149): "precomputed ReadResolutionPlan (per-propagator read routing built at startup)" -- but no further specification. Section 6.2 of the plan shows the `StepContext` generic signature but not how `ReadResolutionPlan` feeds into it. HLD section 15.2 describes read semantics but defers the data structure.

**Resolution:** Add a specification (either in the implementation plan or as a new HLD section 15.4) defining:
```
ReadResolutionPlan = Vec<PerPropagatorReads>
PerPropagatorReads = Vec<FieldResolution>
enum FieldResolution {
    BaseGen,                          // read from base generation
    StagedWrite(PropagatorIndex),     // read from specific prior propagator's staged output
}
```

Construction: iterate propagators in execution order, for each field in `reads()`, check if any prior propagator writes that field. If yes -> `StagedWrite(writer_index)`. If no -> `BaseGen`. `reads_previous()` fields always resolve to `BaseGen`.

This is the highest-risk correctness item in the entire engine. Leaving the data structure unspecified before implementation is unacceptable.

---

### B3. StepContext Signature Conflict
**Source:** Architecture
**Priority Score:** 18 (Severity: High=3, Likelihood: Certain=3, Reversibility: Difficult=2)

The plan contains two incompatible `StepContext` signatures:

1. Section 1.2 / 6.2 (implementation plan lines 65, 483): `StepContext<R: FieldReader, W: FieldWriter>` (generic over reader/writer)
2. HLD section 15, R-PROP-1 (HLD line 953): `StepContext<'a>` (concrete with lifetime, using `FieldReadSet<'a>` and `FieldWriteSet<'a>`)

These are architecturally incompatible. The generic version enables mock testing (propagator crate independent of arena) but makes `ReadResolutionPlan` integration harder. The concrete version simplifies overlay resolution but couples propagator to arena types.

**Evidence:** Implementation plan line 65: "consumed by murk-propagator via `StepContext<R: FieldReader, W: FieldWriter>`". Implementation plan line 483: `pub struct StepContext<R: FieldReader, W: FieldWriter>`. HLD line 953: `pub struct StepContext<'a>`.

**Resolution:** Choose one. Architecture reviewer recommends the concrete `StepContext<'a>` signature from the HLD, with mock testing achieved by having `FieldReadSet<'a>` and `FieldWriteSet<'a>` be trait objects or enums that can wrap both real arena and mock implementations. This preserves testability while simplifying overlay resolution.

However: the generic approach works too, via `impl FieldReader` in the `step()` signature (as shown in plan line 479). The key requirement is that **the plan must be internally consistent** and the choice must be documented.

If choosing generic: update HLD section 15 to match.
If choosing concrete: update implementation plan sections 1.2, 6.2, WP-4.

---

### B4. Propagator Failure Infinite Retry Loop
**Source:** Systems
**Priority Score:** 24 (Severity: Critical=4, Likelihood: Likely=2, Reversibility: Irreversible=3)

HLD section 9.1 specifies tick atomicity: on propagator failure, abandon staging and re-enqueue commands with `TICK_ROLLBACK`. But the plan has **no retry limit**. A deterministic propagator bug (e.g., NaN in a field that triggers a constraint violation every time) causes:

```
tick N: propagator fails -> rollback -> re-enqueue commands
tick N (retry): same commands -> same propagator -> same failure -> rollback -> re-enqueue
(infinite loop)
```

This is not a theoretical concern. In RL training, NaN propagation in reward functions is a common failure mode. A single NaN could lock the entire world permanently.

**Evidence:** HLD section 9.1 (line 514): "Commands are re-enqueued with `TICK_ROLLBACK` reason code." No mention of retry limits anywhere in the HLD or implementation plan. WP-5 and WP-6 acceptance criteria test rollback but not repeated rollback.

**Resolution:** Add to WP-5 (TickEngine Core) deliverables:
1. Per-command retry counter (default: 3). Commands that have been re-enqueued N times are dropped with `REPEATED_ROLLBACK` reason code.
2. World halt after M consecutive tick rollbacks (default: 10). Returns `StepError::RepeatedRollback` to the caller. In Lockstep mode, caller can `reset()`. In RealtimeAsync, TickEngine enters degraded mode.
3. Add `MURK_ERROR_REPEATED_ROLLBACK` to error code enumeration (section 9.7 of HLD).
4. Add acceptance test: "propagator that always fails -> world halts after M ticks, does not loop forever."

---

### B5. FlatBuffers Fuzzing Missing from WP-11
**Source:** Quality
**Priority Score:** 18 (Severity: Critical=4, Likelihood: Possible=1, Reversibility: Difficult=2... but Critical because it is a security boundary)

WP-11 introduces FlatBuffers ObsSpec as a cross-language serialization format. FlatBuffers has known vulnerabilities with malformed input (buffer overflows, out-of-bounds reads in crafted flatbuffers). The ObsSpec is received from Python (user-controlled input that crosses the FFI boundary).

**Evidence:** WP-11 deliverables include "FlatBuffers ObsSpec serialization for cross-language use." WP-11 acceptance criteria include "FlatBuffers round-trip test" but no fuzzing. HLD section 16.3 specifies FlatBuffers but does not address malformed input beyond R-OBS-3 which requires validation "at compilation."

**Resolution:** Add to WP-11 acceptance criteria:
- "FlatBuffers ObsSpec fuzz test: `cargo fuzz` target with `libfuzzer` exercising `ObsPlan::compile()` with arbitrary byte sequences. Must run for minimum 10 minutes with no crashes."

This is a MUST because ObsSpec crosses the FFI boundary (untrusted input). A malformed FlatBuffer that crashes the engine is a denial-of-service vulnerability in any deployment.

---

### B6. Graceful Shutdown Protocol Unspecified
**Source:** Quality
**Priority Score:** 12 (Severity: High=3, Likelihood: Likely=2, Reversibility: Difficult=2)

HLD section 24 explicitly lists I-6 (graceful shutdown) as "Not addressed -- MUST be specified during implementation." The implementation plan references "Graceful shutdown tested" in M5 quality gate (line 406) and WP-15 deliverables (line 244), but nowhere specifies what graceful shutdown means:
- What happens to in-flight commands on shutdown?
- What happens to pending ObsPlans being compiled?
- What happens to held snapshot references (RealtimeAsync egress workers)?
- What is the ordering guarantee (drain commands first? cancel immediately?)

**Evidence:** Implementation plan line 406: "Graceful shutdown tested" in M5 quality gate. Implementation plan line 244: "Graceful shutdown tests" in WP-15. HLD line 1483: "MUST be specified during implementation: in-flight commands, pending ObsPlans, held snapshot references on shutdown."

**Resolution:** Specify the shutdown protocol before M4 (RealtimeAsync). Add a section to the implementation plan or HLD covering:
1. Lockstep shutdown: `drop(LockstepWorld)` -- trivial, Rust's ownership handles it. Document that `&Snapshot` borrows must be released first (borrow checker enforces).
2. RealtimeAsync shutdown: `shutdown()` method that:
   a. Stops accepting new ingress commands (queue returns `SHUTTING_DOWN`)
   b. Drains remaining commands through one final tick (or discards with `SHUTDOWN` reason)
   c. Signals egress workers to finish current ObsPlan and exit
   d. Waits for egress workers with a timeout (cancels stalled workers per section 8.3)
   e. Releases all arena generations

Alternatively, defer the specification to WP-12 deliverables but make it an explicit deliverable, not an implicit M5 afterthought.

---

### B7. Two Mandatory Tests Not Explicitly Covered
**Source:** Quality
**Priority Score:** 12 (Severity: High=3, Likelihood: Certain=3, Reversibility: Easy=1... but blocking because they are MUST tests)

Two of the 17 mandatory tests from HLD section 23 are not explicitly mapped to work package acceptance criteria:

1. **Test #9: Ring eviction -> NOT_AVAILABLE** (HLD line 1459). This is a RealtimeAsync test. WP-12 mentions ring buffer eviction as a deliverable but the acceptance criteria do not explicitly list a "ring eviction returns NOT_AVAILABLE" test. It is implied by stress tests but not named.

2. **Test #10: Ingress backpressure -> deterministic drop** (HLD line 1460). WP-5 tests TTL rejection (expired -> STALE) but does not test queue-full drop (`QUEUE_FULL`). The plan covers TTL-based rejection but not capacity-based rejection.

**Evidence:** WP-12 acceptance criteria (plan lines 217-218) list stress tests #15-17 and epoch reclamation but not test #9 from section 23. WP-5 acceptance criteria (plan lines 151-152) test TTL rejection and tick atomicity but not queue-full behavior.

**Resolution:**
1. Add to WP-12 acceptance criteria: "Ring eviction test: fill ring buffer to capacity, advance ticks, request evicted tick -> receives NOT_AVAILABLE with requested_tick_id and latest_tick_id."
2. Add to WP-5 acceptance criteria: "Queue full test: fill ingress queue to capacity, submit additional command -> receives QUEUE_FULL rejection with receipt."

---

## Warnings (12) - Should Fix

### W1. Epoch Reclamation Needs Tabletop Failure Exercise
**Source:** Systems | **Priority Score:** 12

Epoch-based reclamation with stalled worker teardown (WP-12, HLD section 8.3) is a novel mechanism with no battle-tested reference implementation. Open questions include: cancellation timing for 100K-cell regions, restart vs. replace decisions, cascading stall risk, false positive cost of `max_epoch_hold`.

**Recommendation:** Before implementing WP-12, conduct a documented tabletop failure mode exercise covering at least: (a) worker stalls at 10%, 50%, 100% of pool, (b) cancellation during region iteration at various sizes, (c) ring eviction racing with epoch release, (d) memory bound calculation with max stalled workers. Write the results as a design note in the repo.

### W2. 9-Crate Structure May Cause Churn
**Source:** Architecture | **Priority Score:** 9

Creating all 9 crates from day 1 (WP-0) risks premature abstraction boundaries. Trait signatures and crate boundaries may need adjustment as implementation progresses, causing cross-crate refactoring churn.

**Recommendation:** Add M0.5 Crate Boundary Validation Gate after WP-1 and WP-2 are complete. At M0.5, review whether the crate boundaries match the actual dependency graph. This is lightweight (1-2 hours) and catches boundary mistakes before they propagate.

### W3. Telemetry SHOULD vs. MUST Inconsistency
**Source:** Architecture + Quality (consolidated) | **Priority Score:** 8

R-OPS-1 makes telemetry SHOULD, not MUST. But stress tests #15-17 (mandatory) require `tick_duration`, `queue_depth`, and `rejection_rate` metrics to verify convergence. WP-12 acceptance criteria also require telemetry (line 217: "telemetry (tick duration, queue depth, snapshot age)"). This is a contradiction: the tests are MUST but the data they depend on is SHOULD.

**Recommendation:** Promote a minimum telemetry subset to MUST: `tick_duration_us`, `queue_depth`, `rejection_rate`, `obs_generation_time_us`. These four are required by mandatory tests. The remaining R-OPS-1 metrics can stay SHOULD.

### W4. Hex2D Tensor Export Risk Underrated
**Source:** Architecture | **Priority Score:** 8

Risk #6 in the plan rates Hex2D tensor export as MEDIUM. The Architecture reviewer argues it should be HIGH. Branch-free gather with precomputed index tables for non-rectangular regions is deceptively hard, especially for compositions like Hex2D x Line1D where the padding pattern is multi-dimensional.

**Recommendation:** Upgrade Risk #6 from MEDIUM to HIGH. Add a spike task in WP-10a to prototype the index table generation before committing to the full implementation.

### W5. Overlay Resolution Test Coverage May Be Insufficient
**Source:** Systems | **Priority Score:** 8

WP-5 acceptance specifies "Three-propagator overlay visibility test (5 cases)." Systems reviewer recommends an 8-case matrix to cover all read/write/reads_previous combinations exhaustively:
1. A writes X, B reads X via reads() -- sees A's value
2. A writes X, B reads X via reads_previous() -- sees base gen
3. A writes X, B writes Y reads X via reads() -- sees A's value
4. A writes X, C reads X (A and C not adjacent) -- sees A's value through B
5. No prior write, B reads X -- sees base gen
6. A writes X (Incremental), B reads X -- sees A's modified value
7. A writes X (Full), B reads X -- sees A's full write
8. Three propagators chained: A writes X, B reads X writes Y, C reads Y -- sees B's value

**Recommendation:** Expand to 8 cases. The 5-case version risks missing edge cases in overlay chaining.

### W6. M3/M4 Could Run in Parallel
**Source:** Architecture | **Priority Score:** 6

M3 (Spatial Diversity: Hex2D, ProductSpace, foveation) and M4 (RealtimeAsync) are serialized in the plan. But WP-12 (RealtimeAsync) depends on WP-5 and WP-7, not on WP-10 or WP-11. With 2 developers, M3 and M4 could overlap, saving 6-8 weeks.

**Recommendation:** Restructure milestones to allow M3 and M4 in parallel: one developer on WP-10+WP-11, one on WP-12. Merge at M5 integration.

### W7. Performance Budgets Unvalidated Until M4
**Source:** Architecture | **Priority Score:** 6

The plan's critical-path milestones (M0, M1, M2) do not validate HLD performance budgets. Budget violations discovered at M4 could require fundamental rework of arena, propagator pipeline, or obs generation -- all M0 deliverables.

**Recommendation:** Add M1.5 Performance Gate between M1 and M2. Run reference profile benchmarks on the M1 codebase. If any phase exceeds 2x its budget, investigate before vectorization (M2) locks in the architecture.

### W8. No Overflow/Size Limits for ObsSpec and ProductSpace
**Source:** Quality | **Priority Score:** 6

No size or depth limits for ObsSpec (number of fields, region size) or ProductSpace (number of components beyond the "tested up to 3" note). A user could construct an ObsSpec requesting all fields across a ProductSpace with 10 components, causing combinatorial explosion in plan compilation.

**Recommendation:** Add explicit limits enforced at compilation: max ObsSpec fields (e.g., 256), max ProductSpace components (e.g., 8), max region cells per observation (e.g., 1M). Reject with `INVALID_OBSSPEC` when exceeded.

### W9. No Overflow Checks for Spatial Index Calculations
**Source:** Quality | **Priority Score:** 6

Hex bounding-box calculations (`(2R+1)^2` for disk radius R) and ProductSpace Cartesian products can overflow `u32` or `usize` for large inputs. No overflow checks are specified.

**Recommendation:** Add checked arithmetic in `compile_region()` and `iter_region()`. Return `ObsError` on overflow rather than panicking.

### W10. Static Field Arc Refcount Contention on NUMA
**Source:** Systems | **Priority Score:** 4

For 128-env vectorized scenarios, static fields are shared via `Arc`. On NUMA systems, 128 threads doing `Arc::clone()` and `Arc::drop()` on the same allocation causes cross-socket cache-line bouncing on the refcount.

**Recommendation:** This is a performance concern, not correctness. For v1, document as a known limitation. For v1.5, consider `Arc` per NUMA node or thread-local static field caching.

### W11. Sparse Field Consecutive-Modification Warning Has No Test
**Source:** Quality | **Priority Score:** 4

Risk #8 (sparse field misclassification) specifies a runtime warning when a Sparse field is modified N consecutive ticks. But no test verifies this warning fires.

**Recommendation:** Add to WP-2 acceptance criteria: "Sparse field consecutive-modification warning test: modify a Sparse field for N consecutive ticks -> warning emitted."

### W12. ProductSpace Complexity Risk Should Be Upgraded
**Source:** Systems | **Priority Score:** 4

Risk #5 (ProductSpace complexity) is rated HIGH. Systems reviewer argues it should be CRITICAL because ProductSpace composition errors can cause silent determinism violations (wrong iteration order, wrong distance metric) that are extremely hard to diagnose post-deployment.

**Recommendation:** Upgrade to CRITICAL. The HLD worked examples (section 11.1) serve as acceptance tests, but add property-based testing that verifies `distance(a,b) == BFS_shortest_path(a,b)` for random ProductSpace configurations. This catches silent metric bugs.

---

## Recommendations

### Tracer Bullet: MinimalArena (WP-2a)
**Source:** Architecture
**Type:** SHOULD

Add a tracer-bullet WP-2a that implements a minimal arena (single segment, no sparse slab, no double-buffer) sufficient to run one tick end-to-end. This shortens the critical path by allowing WP-5 to start with a real (non-mock) arena sooner. Full arena features (double-buffer, sparse slab, epoch reclamation) land in WP-2b.

### Existing Libraries
**Source:** Architecture
**Type:** CONSIDER

- **bumpalo** for `ScratchRegion` bump allocation (instead of custom)
- **crossbeam-epoch** as starting point for epoch-based reclamation (instead of from-scratch)
- **hexx** crate for Hex2D coordinate math (evaluate fit before building from scratch)

### M0.5 Skeleton for Mode Duality Validation
**Source:** Systems
**Type:** SHOULD

Add a minimal M0.5 gate where a `RealtimeAsyncWorld` skeleton (no real threading, just the type + a stub `step()`) compiles alongside `LockstepWorld`. This validates that the `World` trait with GAT works for both modes before Lockstep assumptions bake into the TickEngine core during M0-M1.

### RealtimeAsync Determinism Verification Gap
**Source:** Systems
**Type:** SHOULD

RealtimeAsync is deferred to M4, but Lockstep-specific assumptions may bake into the TickEngine core during M0-M2. Add a single-threaded RealtimeAsync determinism test at M2: run RealtimeAsync in single-threaded mode (1 egress thread, no wall-clock deadline) and verify tick-level determinism matches Lockstep for the same inputs.

### Hex Test Geometries
**Source:** Systems
**Type:** SHOULD

WP-10a acceptance criteria should specify concrete hex geometries for tensor export testing: disk R={0, 1, 5, 20}, rectangle {3x3, 7x5, 1x100}. These cover corner cases (R=0 is single cell, R=20 tests large bounding box, 1x100 tests degenerate aspect ratio).

---

## Conflicts Resolved

| Conflict | Architecture View | Quality View | Systems View | Resolution |
|----------|------------------|-------------|-------------|------------|
| **Hex2D tensor export risk severity** | HIGH (upgrade from MEDIUM) | Not mentioned | Not mentioned | Upgraded to HIGH per Architecture assessment. The branch-free gather + precomputed index table for non-rectangular regions is a known difficulty. |
| **ProductSpace complexity risk severity** | Not mentioned | Not mentioned | CRITICAL (upgrade from HIGH) | Kept as HIGH but added property-based BFS verification test (W12). Silent determinism violations are serious but the HLD worked examples provide good guard rails. Upgrade to CRITICAL if property tests reveal issues during implementation. |
| **StepContext signature (generic vs concrete)** | Recommends concrete `StepContext<'a>` | Not mentioned | Not mentioned | Flagged as blocking (B3). Either approach works; the plan must be internally consistent. The generic approach has testability advantages that the plan's crate design depends on (murk-propagator independent of murk-arena). |
| **Telemetry MUST vs SHOULD** | Subset should be MUST | Subset should be MUST | Not mentioned | Aligned: promote 4 metrics to MUST (tick_duration, queue_depth, rejection_rate, obs_generation_time). These are dependencies of mandatory tests. |
| **Epoch reclamation approach** | Consider crossbeam-epoch | Not mentioned | Tabletop exercise first | Compatible: tabletop exercise + evaluate crossbeam-epoch as starting point. Both recommendations strengthen the same deliverable. |

---

## Reviewer Summaries

| Reviewer | Verdict | Blocking | Warnings |
|----------|---------|----------|----------|
| **Reality** | APPROVED | 0 | 0 |
| **Architecture** | CHANGES_REQUESTED | 3 | 5 |
| **Quality** | CHANGES_REQUESTED | 3 | 8 |
| **Systems** | CHANGES_REQUESTED | 2 | 6 |

Reality reviewer confirmed: all HLD requirement references are valid (R-ARCH through R-MIG), crate DAG is valid, WP dependency chains are valid, no hallucinated symbols or methods. This is a strong foundation -- the issues are in under-specification and missing edge cases, not in incorrect references.

---

## Priority-Sorted Issue Table

| ID | Source | Issue | Priority Score | Type |
|----|--------|-------|---------------|------|
| B1 | Architecture | Determinism versioning missing | 36 | BLOCKING |
| B2 | Architecture | ReadResolutionPlan unspecified | 24 | BLOCKING |
| B4 | Systems | Propagator failure retry loop | 24 | BLOCKING |
| B3 | Architecture | StepContext signature conflict | 18 | BLOCKING |
| B5 | Quality | FlatBuffers fuzzing missing | 18 | BLOCKING |
| B6 | Quality | Graceful shutdown unspecified | 12 | BLOCKING |
| B7 | Quality | Two mandatory tests uncovered | 12 | BLOCKING |
| W1 | Systems | Epoch reclamation tabletop | 12 | WARNING |
| W2 | Architecture | 9-crate churn risk | 9 | WARNING |
| W3 | Arch+Quality | Telemetry MUST vs SHOULD | 8 | WARNING |
| W4 | Architecture | Hex tensor export risk underrated | 8 | WARNING |
| W5 | Systems | Overlay test coverage | 8 | WARNING |
| W6 | Architecture | M3/M4 parallelization | 6 | WARNING |
| W7 | Architecture | Late perf budget validation | 6 | WARNING |
| W8 | Quality | No ObsSpec/ProductSpace size limits | 6 | WARNING |
| W9 | Quality | No overflow checks for spatial math | 6 | WARNING |
| W10 | Systems | Arc refcount NUMA contention | 4 | WARNING |
| W11 | Quality | Sparse consecutive-mod test missing | 4 | WARNING |
| W12 | Systems | ProductSpace risk upgrade | 4 | WARNING |

---

## Next Steps

**Status: CHANGES_REQUESTED**

1. Fix the 7 blocking issues above. Estimated effort: 2-3 days of specification work (no code changes required -- these are all plan/spec amendments).

2. The most impactful fixes in order:
   - **B1** (determinism versioning): Add one `u32` field to three locations. Small change, prevents irreversible compatibility breakage.
   - **B2** (ReadResolutionPlan): Specify the data structure. This is the highest-risk correctness item -- it must not be left to implementation-time improvisation.
   - **B4** (retry loop): Add retry limits. Simple addition to WP-5, prevents infinite loops in production.
   - **B3** (StepContext): Pick one signature, update all references. Purely a consistency fix.
   - **B5** (fuzzing): Add one line to WP-11 acceptance criteria.
   - **B6** (shutdown): Write 10-15 lines of shutdown protocol. Can defer to WP-12 deliverables if explicitly listed.
   - **B7** (mandatory tests): Add two tests to existing WP acceptance criteria.

3. After fixing blockers, address warnings W1-W5 (priority score >= 8) before execution.

4. Then run `/review-plan` again.
