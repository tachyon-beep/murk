# Improvement Roadmap (Next Body of Work)

This is a risk-first roadmap: correctness/security and “trust surfaces” (docs, packaging, FFI) come before expanding feature scope.

## Phase 1: Hardening + Trust (1–2 weeks)

- **FFI input validation pass (Critical):** eliminate unchecked arithmetic and unbounded allocation pressure in all entrypoints; start with `murk_obsplan_execute_agents` (`crates/murk-ffi/src/obs.rs`).
- **Decide poisoning policy (Critical):** clarify whether a caught panic is “process-fatal”, “world-fatal”, or “call-fatal”, and implement a recovery path consistent with that decision (`crates/murk-ffi/src/lib.rs`).
- **Replay spec alignment (Critical):** reconcile `docs/replay-format.md` with `murk_replay::FORMAT_VERSION` and add a CI check that prevents future drift (`docs/replay-format.md`, `crates/murk-replay/src/lib.rs`).
- **Repo hygiene (Critical):** remove tracked build artifacts/caches from the Python tree and rely on the release pipeline artifacts; add `.gitignore` coverage for generated files (`crates/murk-python/python/murk/_murk.abi3.so`, `crates/murk-python/.pytest_cache`).
- **Docs alignment (High):** unify Python version + install-from-source vs published messaging across README/book/pyproject/release workflow.

**Exit criteria:** “a new user can install and use Murk from docs without surprises” and “FFI cannot be trivially crashed or coerced into pathological allocations by bad inputs”.

## Phase 2: Realtime Reliability + Observability (2–3 weeks)

- **Telemetry/counters:** queue saturation (`QueueFull`), rollbacks, `tick_disabled` transitions, worker stall/unpin events, ring “NotAvailable” rates; expose through Rust + FFI + Python metrics.
- **Egress ergonomics:** add a non-blocking observe/preflight API (or explicit queue-depth reporting) so realtime consumers can avoid “mystery latency”.
- **Ring behavior:** add explicit retention/skew signals and tighten the story around snapshot eviction and staleness for callers.

**Exit criteria:** realtime users can answer “are we dropping commands?” and “are observations stale/evicted?” from metrics alone.

## Phase 3: Scale-Up Performance (2–4 weeks)

- **`murk-obs`:** remove per-call pooling allocations; optimize `execute_batch` to avoid redundant work and scale better with `num_envs` (`crates/murk-obs/src/plan.rs`, `crates/murk-obs/src/pool.rs`).
- **`murk-space`:** add caching/indexing for canonical ordering and coordinate→tensor mapping; reduce default O(n) scans (`crates/murk-space/src/space.rs`, `crates/murk-space/src/product.rs`).
- **`murk-arena`:** reduce publish-time copying in owned snapshots (where possible) and optimize sparse reuse bookkeeping (`crates/murk-arena/src/read.rs`, `crates/murk-arena/src/sparse.rs`).

**Exit criteria:** batch training and multi-agent observation extraction show improved throughput without changing user code.

## Phase 4: v0.2 Feature Work (Echelon-Driven)

These are already outlined in `ROADMAP.md`; the highest-leverage “next body of work” features are:

- **Line-of-sight / visibility queries:** implement deterministic ray/visibility APIs in `murk-space` (and reuse them in observation planning) to unlock occlusion-aware sensors.
- **Render adapter interface:** `RenderSpec -> RenderPlan` mirroring obs, producing scene descriptions from snapshots without mutating state.
- **Heterogeneous observation composition:** multi-spec batching (agent-type loadouts) and richer batched stepping so training can stay on the single-GIL hot path.

**Exit criteria:** Echelon-like workloads (multi-agent, heterogeneous sensors, occlusion) can be expressed without bespoke per-project plumbing.

## If You Only Do 3 Things Next

1. Finish FFI hardening + poisoning strategy (Phase 1).
2. Fix replay/docs/packaging drift (Phase 1).
3. Add realtime telemetry and expose it everywhere (Phase 2).
