# Improvement Roadmap (Next Body of Work)

This is a risk-first roadmap: correctness/security and “trust surfaces” (docs, packaging, FFI) come before expanding feature scope.

## Phase 1: Hardening + Trust (1–2 weeks)

- **[Resolved] FFI input validation pass (Critical):** unchecked arithmetic/buffer sizing paths are guarded with deterministic status returns.
- **[Resolved] Decide poisoning policy (Critical):** policy is documented and implemented with deterministic post-poison behavior.
- **[Resolved] Replay spec alignment (Critical):** replay doc/version drift is now CI-guarded.
- **[Resolved] Repo hygiene (Critical):** tracked artifact/caches are excluded and release workflow validates built artifacts.
- **[Resolved] Docs alignment (High):** install/version contract is aligned and CI-checked.

**Exit criteria:** “a new user can install and use Murk from docs without surprises” and “FFI cannot be trivially crashed or coerced into pathological allocations by bad inputs”.

## Phase 2: Realtime Reliability + Observability (2–3 weeks)

- **[Resolved] Telemetry/counters:** queue saturation (`QueueFull`), rollbacks, `tick_disabled` transitions, worker stall/unpin events, and ring “NotAvailable” rates are exposed through Rust + FFI + Python metrics.
- **[Resolved] Egress ergonomics:** non-blocking preflight/queue-depth visibility is available for realtime and binding-layer callers.
- **[Resolved] Ring behavior:** explicit ring retention/skew signaling is exposed via preflight and cumulative metrics (`evictions`, `stale reads`, `skew retries`) to diagnose eviction/staleness under load.

**Exit criteria:** realtime users can answer “are we dropping commands?” and “are observations stale/evicted?” from metrics alone.

## Phase 3: Scale-Up Performance (2–4 weeks)

- **[Resolved] Performance harness + budgets:** representative obs/space/arena benchmarks and regression thresholds are documented for Phase 3 gatekeeping.
- **[Resolved] `murk-obs`:** removed pooled-path per-call allocations and optimized `execute_batch`; multi-agent batch benchmark target is now met.
- **[Resolved] `murk-space`:** added mixed-radix rank stride caching and slice-based coordinate ranking to remove hot-path coordinate allocation and reduce coordinate→rank lookup overhead; Task 12 benchmark target is now met.
- **[Open] `murk-arena`:** reduce publish-time copying in owned snapshots (where possible) and optimize sparse reuse bookkeeping.

**Exit criteria:** batch training and multi-agent observation extraction show improved throughput without changing user code.

## Phase 4: v0.2 Feature Work (Echelon-Driven)

These are already outlined in `ROADMAP.md`; the highest-leverage “next body of work” features are:

- **[Open] Line-of-sight / visibility queries:** implement deterministic ray/visibility APIs in `murk-space` (and reuse them in observation planning) to unlock occlusion-aware sensors.
- **[Open] Render adapter interface:** `RenderSpec -> RenderPlan` mirroring obs, producing scene descriptions from snapshots without mutating state.
- **[Open] Heterogeneous observation composition:** multi-spec batching (agent-type loadouts) and richer batched stepping so training can stay on the single-GIL hot path.

**Exit criteria:** Echelon-like workloads (multi-agent, heterogeneous sensors, occlusion) can be expressed without bespoke per-project plumbing.

## If You Only Do 3 Things Next

1. [Resolved] Finish FFI hardening + poisoning strategy (Phase 1).
2. [Resolved] Fix replay/docs/packaging drift (Phase 1).
3. [Resolved] Add realtime telemetry and expose it everywhere (Phase 2).
