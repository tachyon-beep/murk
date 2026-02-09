# DESIGN.md Review Board — Round 1 Findings (Condensed)

## Reviewer A: Architecture Critic
**Verdict: Request Changes**

Key findings:
1. Three-interface model (Ingress/TickEngine/Egress) is structurally sound.
2. **CRITICAL: ProductSpace adjacency/distance/iteration composition undefined** — neighbours(), distance(), compile_region() for composed spaces like Hex2D x Line1D are never specified. Every propagator and ObsPlan depends on these.
3. **CRITICAL: Snapshot creation strategy unspecified** — budget is 1-3% tick time but no copy strategy (full clone vs CoW vs structural sharing). Large VoxelOctreeSpaces will blow this.
4. **CRITICAL: Propagator interface/trait not defined** — the most important extension point has the least specification. No function signature, no Space interaction, no error handling.
5. **CRITICAL: ObsPlan execution threading model missing** — where does egress work run? If on TickEngine thread, contention is the primary bottleneck.
6. Space vs Topology conflated in R-SPACE-2 ("directly or via topology component") — this ambiguity will produce inconsistent backends.
7. FlatBuffers schema evolution story is weak for a rapidly-evolving ObsSpec.
8. VoxelOctreeSpace adapter (R-MIG-1) is classic adapter-pattern debt — needs explicit retirement timeline.
9. Replay log format never defined — ad-hoc choices during implementation will be hard to change.
10. Three generation IDs (§19.1) may be over-engineered for v1 — single world_generation_id would be simpler.
11. Runtime-N dimensionality (R-SPACE-0) is YAGNI if realistic use cases are 1D-3D — const-generic approach would be simpler/faster.
12. R-FFI-2 caller-allocated buffer validation missing — who checks buffer matches ObsPlan output shape? Buffer overrun is memory safety issue.

Risk table:
| Risk | Severity | Likelihood |
|------|----------|------------|
| ProductSpace under-specification | High | High |
| Snapshot creation blows tick budget | High | Medium |
| Propagator interface ambiguity | Medium | High |
| ObsPlan execution contention | Medium | Medium |
| Lockstep deadlock (Egress blocking + Ingress) | High | Low |

---

## Reviewer B: Quality Assurance Engineer
**Verdict: Request Changes**

Key findings:
1. **ProductSpace composition edge cases all undefined**: region queries across heterogeneous components, iteration order for products, propagator adjacency in ProductSpace.
2. **Hex offset layout never specified** (flat-top vs pointy-top, odd-r vs even-r) — this is a determinism and interoperability boundary.
3. **11 requirements lack acceptance criteria entirely**: R-SPACE-4, R-FIELD-2, R-CMD-1, R-MODE-1/2/3, R-OBS-1/2/4/6, R-DET-2/3, R-OPS-1.
4. **Tier B determinism needs numeric tolerance spec** — bit-exact? epsilon? After how many ticks?
5. **R-SPACE-0 "supports any N" is philosophical, not testable** — needs concrete architectural limit.
6. **Snapshot ring K=0 case not addressed**. No maximum timeout for lockstep blocking.
7. **arrival_seq wraparound at u64::MAX** — no policy.
8. **coverage metadata listed but never defined**.
9. **TTL clock (wall vs tick)** not specified — TickEngine stalls cause spurious expiries if wall-clock.
10. **ObsPlan invalidation during execution on immutable snapshot** — spec implies fail on generation mismatch even when snapshot is consistent.
11. **Performance targets reference undefined "representative load"** — circular without published profile.
12. **Receipt reason_code enum not specified** — no TTL_EXPIRED, QUEUE_FULL, etc.
13. Proposed critical test strategy: deterministic iteration per backend (1000x), end-to-end replay (byte-exact at tick 1000), hex axial-tensor bijection, ProductSpace exhaustiveness (no duplicates/gaps), property-based fuzz on distance metrics.
14. Regression hotspots: determinism (any float/SIMD/parallel change), command ordering (HashMap iteration), ObsPlan cache invalidation, snapshot memory bounds.

---

## Reviewer C: Systems Thinking / Pattern Recognizer
**Verdict: Request Changes**

Key findings:
1. **CRITICAL: Tick budget math doesn't close** — 200 obs/sec at 60 Hz = 3.33 obs/tick. At p99=5ms each, worst case = 16.65ms on egress alone, leaving 0.02ms for simulation, propagators, commands, and snapshot publish. This is physically impossible if egress shares the TickEngine thread.
2. **Primary feedback loop (R1 — reinforcing death spiral)**: Complex ObsPlans → higher latency → staler agent data → more rejections → agents request more observations → more latency. Backpressure (B1) only triggers after queue saturation — too late.
3. **Archetype: "Growth and Underinvestment"** — heavy investment in features (nD, ProductSpace, hex, tensor export) but underinvestment in capacity constraint (single-threaded tick budget).
4. **Secondary archetype: "Shifting the Burden"** — ObsPlan compilation optimization (§12, 6 requirements) papers over the fundamental solution (concurrent architecture, §4, 0 requirements for concurrent snapshot access).
5. **Snapshot refcount contention**: atomic inc/dec at 200 ops/sec creates cache-line ping-ponging that violates "no locks on hot read path" claim. Recommend epoch-based GC (crossbeam-epoch).
6. **ProductSpace coordinate allocation**: 5-component tuples at 40+ bytes, 60 Hz x 10k cells = 24 MB/sec allocation churn.
7. **Hex padding scales multiplicatively in ProductSpace**: Hex2D x Hex2D = ~62% invalid tensor elements. No padding budget defined.
8. **Tier B determinism conflicts with "ML-native" goal** — ISA-specific replay prevents heterogeneous cloud training, cross-platform research reproduction.
9. **Stale action rejection causes oscillation**: reject → request fresh obs → slow pipeline → still stale → reject again. Needs exponential backoff or adaptive max_tick_skew.
10. **ObsPlan invalidation creates availability gap**: when generation ID bumps, all plans fail with PLAN_INVALIDATED. What do agents observe during recompilation?
11. **Historical parallels**: Unity DOTS (same single-thread bottleneck, added incremental copy), Source Engine (observation lag > 3 ticks = rubber-banding), ROS tf2 (transform composition expensive, cache-hostile).
12. **Highest-leverage intervention**: Separate tick budget from observation budget — dedicated egress worker pool. This breaks the death spiral.
