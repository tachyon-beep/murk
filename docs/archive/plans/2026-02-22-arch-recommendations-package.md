# Architecture Recommendations Work Package Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Execute the architecture recommendations in a risk-first package that restores trust surfaces (FFI/docs/packaging), improves realtime observability, and then unlocks performance and v0.2 features.

**Architecture:** This package is split into four phase-gated workstreams: Phase 1 hardening/trust, Phase 2 realtime reliability/observability, Phase 3 scale-up performance, and Phase 4 v0.2 feature unlocks. Each phase has explicit exit criteria and can be merged as independent PRs. Phase 2 cannot start until Phase 1 exit criteria pass; Phase 4 cannot start until Phase 3 regression budgets are green.

**Tech Stack:** Rust workspace (`murk-*` crates), PyO3/maturin (`crates/murk-python`), GitHub Actions, pytest, cargo test/bench.

**Prerequisites:**
- Branch from latest `main` and create a tracking epic for this plan.
- Confirm target release branch and version for this package (recommended: `v0.1.x` hardening train, then `v0.2.0` feature train).
- Ensure CI secrets are available for artifact smoke jobs that install built wheels/sdists.

---

## Package Summary

| Phase | Duration | Focus | Deliverables |
|---|---|---|---|
| 1 | 1-2 weeks | Hardening + trust surfaces | FFI validation/poisoning policy, replay/doc alignment, packaging hygiene |
| 2 | 2-3 weeks | Realtime reliability + observability | Counters and telemetry through Rust + FFI + Python, preflight visibility |
| 3 | 2-4 weeks | Scale-up performance | `murk-obs`, `murk-space`, `murk-arena` throughput improvements with benchmark gates |
| 4 | 2-4 weeks | v0.2 feature unlock | LOS/render/heterogeneous obs implementation behind quality gates |

## Program-Level Gates

- Gate A (after Phase 1): docs/install/replay contract passes automated checks; FFI bad-input fuzz/negative tests pass.
- Gate B (after Phase 2): realtime health can be answered from metrics only (`QueueFull`, `tick_disabled`, worker stalls, ring misses).
- Gate C (after Phase 3): benchmark deltas meet targets with no regression in determinism or CI stability.
- Gate D (after Phase 4): v0.2 workloads run without bespoke project glue.

## Task 1: Re-baseline and Scope Lock

**Files:**
- Modify: `docs/arch-analysis-2026-02-22-1219/05-architecture-assessment.md`
- Modify: `docs/arch-analysis-2026-02-22-1219/06-technical-debt-catalog.md`
- Modify: `docs/arch-analysis-2026-02-22-1219/09-improvement-roadmap.md`
- Create: `docs/plans/2026-02-22-arch-recommendations-package-baseline.md`

**Step 1: Re-verify each recommendation against current HEAD**

Run:
```bash
rg -n "FORMAT_VERSION|Current version|requires-python|QueueFull|tick_disabled|MurkStepMetrics" docs/replay-format.md crates/murk-replay/src/lib.rs README.md book/src/getting-started.md crates/murk-python/pyproject.toml crates/murk-engine/src crates/murk-ffi/src
```

Expected output:
```
Matches for all recommendation surfaces with at least one hit per target.
```

**Step 2: Record what is still open vs already resolved**

Run:
```bash
git diff -- docs/arch-analysis-2026-02-22-1219
```

Expected output:
```
Only classification changes (open/resolved/deferred), no recommendation removals.
```

**Step 3: Capture baseline validation metrics**

Run:
```bash
cargo test --workspace
```

Expected output:
```
All workspace tests pass; failures are documented in baseline file with owner.
```

**Step 4: Commit baseline**

```bash
git add docs/arch-analysis-2026-02-22-1219 docs/plans/2026-02-22-arch-recommendations-package-baseline.md
git commit -m "docs(plan): baseline architecture recommendations against current HEAD"
```

**Definition of Done:**
- [ ] Every recommendation is tagged `Open`, `PartiallyResolved`, or `Resolved`.
- [ ] Baseline command outputs and risks are recorded.
- [ ] Phase scope is locked for implementation.

## Task 2: FFI Input Validation Hardening

**Files:**
- Modify: `crates/murk-ffi/src/obs.rs`
- Modify: `crates/murk-ffi/src/status.rs` (only if new error code is required)
- Test: `crates/murk-ffi/src/obs.rs` (unit tests module)
- Test: `crates/murk-ffi/src/world.rs` (integration-like FFI behavior tests)

**Step 1: Add failing tests for overflow and oversized allocations**

Run:
```bash
cargo test -p murk-ffi -- obsplan_execute_agents
```

Expected output:
```
FAIL for new tests covering n_agents*ndim overflow and oversized output/mask requirements.
```

**Step 2: Implement checked arithmetic and allocation bounds**

Implementation requirements:
- Replace unchecked `n * dim` and derived buffer math with checked multiply/add.
- Reject invalid sizes early with deterministic status code (`InvalidArgument` unless a new dedicated status is introduced).
- Add explicit upper bounds for agent count and dimensionality to prevent pathological allocations.

**Step 3: Re-run crate tests**

Run:
```bash
cargo test -p murk-ffi
```

Expected output:
```
PASS with new negative tests and no regressions.
```

**Step 4: Commit**

```bash
git add crates/murk-ffi/src/obs.rs crates/murk-ffi/src/status.rs crates/murk-ffi/src/world.rs
git commit -m "fix(ffi): harden obsplan_execute_agents size arithmetic and bounds"
```

**Definition of Done:**
- [ ] No unchecked multiplication/allocation in `murk_obsplan_execute_agents`.
- [ ] Invalid inputs return stable error codes, never panic.
- [ ] Tests exercise overflow and pathological-size paths.

## Task 3: FFI Poisoning Policy and Recovery Path

**Files:**
- Create: `docs/design/ffi-poisoning-policy.md`
- Modify: `crates/murk-ffi/src/lib.rs`
- Modify: `crates/murk-ffi/src/world.rs`
- Test: `crates/murk-ffi/src/lib.rs`
- Docs: `docs/error-reference.md`

**Step 1: Write and approve policy**

Policy decision options (record one in doc):
- `process-fatal` (strict fail-closed)
- `world-fatal` (recommended)
- `call-fatal` (recover and continue)

Run:
```bash
rg -n "InternalError|poison|ffi_lock!" crates/murk-ffi/src/lib.rs crates/murk-ffi/src/world.rs docs/error-reference.md
```

Expected output:
```
All poisoning paths and caller-facing semantics are referenced.
```

**Step 2: Implement behavior aligned with policy**

Implementation requirements:
- Make poisoning handling explicit and documented.
- Ensure repeated calls after poison produce deterministic behavior.
- Add one recovery mechanism if policy is not `process-fatal` (for example world reset/recreate flow).

**Step 3: Add tests for post-poison behavior**

Run:
```bash
cargo test -p murk-ffi -- poison
```

Expected output:
```
PASS with deterministic outcomes for each poison scenario.
```

**Step 4: Commit**

```bash
git add docs/design/ffi-poisoning-policy.md crates/murk-ffi/src/lib.rs crates/murk-ffi/src/world.rs docs/error-reference.md
git commit -m "feat(ffi): define and implement mutex poisoning policy"
```

**Definition of Done:**
- [ ] Poisoning behavior is a documented product decision.
- [ ] Runtime behavior matches policy in tests.
- [ ] `InternalError` semantics are clear in docs.

## Task 4: Replay Spec Alignment and Drift Guard

**Files:**
- Modify: `docs/replay-format.md`
- Modify: `crates/murk-replay/src/lib.rs` (if constant/comment cleanup needed)
- Create: `scripts/check_replay_format_version.py`
- Modify: `.github/workflows/ci.yml`
- Test: `crates/murk-replay/tests/determinism.rs`

**Step 1: Add failing contract check**

Run:
```bash
python scripts/check_replay_format_version.py
```

Expected output:
```
FAIL when docs version != murk_replay::FORMAT_VERSION.
```

**Step 2: Align docs to code**

Implementation requirements:
- Update header/current-version sections in `docs/replay-format.md`.
- Update version-history section to include current version behavior.

**Step 3: Wire check into CI**

Run:
```bash
cargo test -p murk-replay && python scripts/check_replay_format_version.py
```

Expected output:
```
PASS locally and in CI check job.
```

**Step 4: Commit**

```bash
git add docs/replay-format.md crates/murk-replay/src/lib.rs scripts/check_replay_format_version.py .github/workflows/ci.yml
git commit -m "docs(replay): align replay format spec and add CI drift check"
```

**Definition of Done:**
- [ ] `docs/replay-format.md` matches `FORMAT_VERSION`.
- [ ] CI fails on future drift.
- [ ] Replay tests remain green.

## Task 5: Packaging Hygiene and Artifact-First Validation

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `.github/workflows/ci.yml`
- Modify: `.gitignore`
- Modify: `crates/murk-python/pyproject.toml` (only if packaging metadata changes)
- Docs: `README.md`

**Step 1: Ensure generated artifacts are never source-of-truth**

Run:
```bash
git ls-files | rg -n "(_murk\\.abi3\\.so|\\.pytest_cache|__pycache__|dist/)"
```

Expected output:
```
No tracked generated binary/cache artifacts.
```

**Step 2: Add artifact smoke validation job**

Implementation requirements:
- After wheel/sdist build, create isolated install smoke step that installs produced artifact.
- Run a minimal import + basic API call smoke test on installed package.
- Keep editable/develop-mode tests, but do not rely on them exclusively.

**Step 3: Validate release workflow**

Run:
```bash
act -W .github/workflows/release.yml -j build-sdist
```

Expected output:
```
Artifact build job succeeds locally or documented if `act` unavailable in environment.
```

**Step 4: Commit**

```bash
git add .github/workflows/release.yml .github/workflows/ci.yml .gitignore crates/murk-python/pyproject.toml README.md
git commit -m "ci(python): validate wheel/sdist artifacts before publish"
```

**Definition of Done:**
- [ ] Source tree is free of tracked generated artifacts.
- [ ] CI validates built artifacts, not only editable installs.
- [ ] Release pipeline preserves reproducibility story.

## Task 6: Docs Install and Version Contract Alignment

**Files:**
- Modify: `README.md`
- Modify: `book/src/getting-started.md`
- Modify: `crates/murk-python/pyproject.toml` (if version requirement changes)
- Create: `scripts/check_install_docs_consistency.py`
- Modify: `.github/workflows/ci.yml`

**Step 1: Normalize published-vs-source guidance**

Run:
```bash
rg -n "Python 3\\.|pip install murk|maturin develop|from source|published" README.md book/src/getting-started.md crates/murk-python/pyproject.toml
```

Expected output:
```
Consistent minimum Python version and install mode wording.
```

**Step 2: Add docs consistency check**

Run:
```bash
python scripts/check_install_docs_consistency.py
```

Expected output:
```
PASS when README/book/pyproject version and install guidance align.
```

**Step 3: Commit**

```bash
git add README.md book/src/getting-started.md crates/murk-python/pyproject.toml scripts/check_install_docs_consistency.py .github/workflows/ci.yml
git commit -m "docs: enforce install/version consistency across user surfaces"
```

**Definition of Done:**
- [ ] User-facing install guidance is internally consistent.
- [ ] CI catches future divergence.

## Task 7: Realtime Health Counters in Engine

**Files:**
- Modify: `crates/murk-engine/src/metrics.rs`
- Modify: `crates/murk-engine/src/ingress.rs`
- Modify: `crates/murk-engine/src/tick.rs`
- Modify: `crates/murk-engine/src/tick_thread.rs`
- Modify: `crates/murk-engine/src/ring.rs`
- Test: `crates/murk-engine/src/ingress.rs`
- Test: `crates/murk-engine/src/tick.rs`
- Test: `crates/murk-engine/src/tick_thread.rs`

**Step 1: Add failing tests for realtime health counters**

Run:
```bash
cargo test -p murk-engine -- queue_full tick_disabled rollback worker ring
```

Expected output:
```
FAIL for new metric assertions before implementation.
```

**Step 2: Implement counters**

Minimum counters to add:
- `queue_full_rejections`
- `tick_disabled_rejections`
- `rollback_count`
- `tick_disabled_transitions`
- `worker_stall_events`
- `ring_not_available_events`

**Step 3: Validate in engine tests**

Run:
```bash
cargo test -p murk-engine
```

Expected output:
```
PASS with counter increments verified in deterministic tests.
```

**Step 4: Commit**

```bash
git add crates/murk-engine/src/metrics.rs crates/murk-engine/src/ingress.rs crates/murk-engine/src/tick.rs crates/murk-engine/src/tick_thread.rs crates/murk-engine/src/ring.rs
git commit -m "feat(engine): add realtime reliability counters to step metrics"
```

**Definition of Done:**
- [ ] Realtime drop/fail-stop modes are represented in metrics.
- [ ] Counters are deterministic and test-covered.

## Task 8: Expose Realtime Counters Through FFI and Python

**Files:**
- Modify: `crates/murk-ffi/src/metrics.rs`
- Modify: `crates/murk-ffi/include/murk.h`
- Modify: `crates/murk-python/src/metrics.rs`
- Modify: `crates/murk-python/python/murk/_murk.pyi`
- Modify: `crates/murk-python/python/murk/env.py`
- Test: `crates/murk-ffi/src/world.rs`
- Test: `crates/murk-python/tests/test_world.py`
- Test: `crates/murk-python/tests/test_obs.py`

**Step 1: Extend ABI-safe metrics struct**

Run:
```bash
cargo test -p murk-ffi -- metrics
```

Expected output:
```
FAIL for new fields before struct conversion is updated.
```

**Step 2: Update Python wrapper and typing**

Run:
```bash
python -m pytest crates/murk-python/tests/test_world.py -q
```

Expected output:
```
PASS with new metrics fields available in Python object and typing surface.
```

**Step 3: Commit**

```bash
git add crates/murk-ffi/src/metrics.rs crates/murk-ffi/include/murk.h crates/murk-python/src/metrics.rs crates/murk-python/python/murk/_murk.pyi crates/murk-python/python/murk/env.py crates/murk-ffi/src/world.rs crates/murk-python/tests/test_world.py crates/murk-python/tests/test_obs.py
git commit -m "feat(metrics): surface realtime reliability counters via FFI and Python"
```

**Definition of Done:**
- [ ] New counters are visible in Rust, C, and Python.
- [ ] ABI/layout checks remain stable.
- [ ] Python tests validate new fields.

## Task 9: Realtime Preflight and Queue-Depth Visibility

**Files:**
- Modify: `crates/murk-engine/src/realtime.rs`
- Modify: `crates/murk-engine/src/egress.rs`
- Modify: `crates/murk-ffi/src/world.rs`
- Modify: `crates/murk-python/src/world.rs`
- Modify: `crates/murk-python/python/murk/env.py`
- Modify: `crates/murk-python/python/murk/_murk.pyi`
- Test: `crates/murk-engine/src/realtime.rs`
- Test: `crates/murk-python/tests/test_vec_env.py`

**Step 1: Add preflight API in engine**

Implementation target:
- Add non-blocking visibility API (recommended: queue-depth + ring-age/readiness snapshot).

Run:
```bash
cargo test -p murk-engine -- realtime
```

Expected output:
```
PASS with tests asserting stable preflight values under load.
```

**Step 2: Expose via FFI/Python**

Run:
```bash
python -m pytest crates/murk-python/tests/test_vec_env.py -q
```

Expected output:
```
PASS with Python-facing preflight/introspection API.
```

**Step 3: Commit**

```bash
git add crates/murk-engine/src/realtime.rs crates/murk-engine/src/egress.rs crates/murk-ffi/src/world.rs crates/murk-python/src/world.rs crates/murk-python/python/murk/env.py crates/murk-python/python/murk/_murk.pyi crates/murk-engine/src/realtime.rs crates/murk-python/tests/test_vec_env.py
git commit -m "feat(realtime): add preflight queue/ring visibility APIs"
```

**Definition of Done:**
- [ ] Clients can detect overload risk before blocking observe calls.
- [ ] Health endpoints are available across Rust/FFI/Python.

## Task 10: Performance Harness and Regression Budgets

**Files:**
- Modify: `crates/murk-bench/benches/obs_ops.rs`
- Modify: `crates/murk-bench/benches/space_ops.rs`
- Modify: `crates/murk-bench/benches/arena_ops.rs`
- Create: `docs/design/performance-budget.md`
- Modify: `.github/workflows/ci.yml` (optional nightly perf gate)

**Step 1: Add representative benchmark scenarios**

Run:
```bash
cargo bench -p murk-bench --bench obs_ops --bench space_ops --bench arena_ops
```

Expected output:
```
Baseline benchmark results captured and committed in performance-budget doc.
```

**Step 2: Define regression thresholds**

Targets:
- `murk-obs` batch extraction throughput
- `murk-space` coordinate-to-rank lookup latency
- `murk-arena` snapshot publish + sparse reuse throughput

**Step 3: Commit**

```bash
git add crates/murk-bench/benches/obs_ops.rs crates/murk-bench/benches/space_ops.rs crates/murk-bench/benches/arena_ops.rs docs/design/performance-budget.md .github/workflows/ci.yml
git commit -m "perf: add benchmark harness and performance budgets"
```

**Definition of Done:**
- [ ] Benchmarks represent target workloads from roadmap.
- [ ] Budget thresholds are written and reviewable.

## Task 11: `murk-obs` Allocation and Batch Optimization

**Files:**
- Modify: `crates/murk-obs/src/pool.rs`
- Modify: `crates/murk-obs/src/plan.rs`
- Test: `crates/murk-obs/src/plan.rs`
- Bench: `crates/murk-bench/benches/obs_ops.rs`

**Step 1: Remove per-call pooling allocations**

Run:
```bash
cargo test -p murk-obs
```

Expected output:
```
PASS with unchanged semantics and reduced allocation churn under bench.
```

**Step 2: Optimize `execute_batch` hot path**

Run:
```bash
cargo bench -p murk-bench --bench obs_ops
```

Expected output:
```
Improved throughput versus baseline; deltas recorded in performance-budget doc.
```

**Step 3: Commit**

```bash
git add crates/murk-obs/src/pool.rs crates/murk-obs/src/plan.rs crates/murk-bench/benches/obs_ops.rs docs/design/performance-budget.md
git commit -m "perf(obs): reduce pooling allocations and optimize execute_batch"
```

**Definition of Done:**
- [ ] No behavior regressions in obs correctness tests.
- [ ] Benchmarks show measurable gain on multi-agent batches.

## Task 12: `murk-space` Lookup and Canonicalization Optimization

**Files:**
- Modify: `crates/murk-space/src/space.rs`
- Modify: `crates/murk-space/src/product.rs`
- Test: `crates/murk-space/src/space.rs`
- Test: `crates/murk-space/src/product.rs`
- Bench: `crates/murk-bench/benches/space_ops.rs`

**Step 1: Add caching/indexing for coordinate mapping**

Run:
```bash
cargo test -p murk-space
```

Expected output:
```
PASS with deterministic rank/index behavior preserved.
```

**Step 2: Validate perf gains**

Run:
```bash
cargo bench -p murk-bench --bench space_ops
```

Expected output:
```
Reduced lookup/canonicalization latency on high-dimensional spaces.
```

**Step 3: Commit**

```bash
git add crates/murk-space/src/space.rs crates/murk-space/src/product.rs crates/murk-bench/benches/space_ops.rs docs/design/performance-budget.md
git commit -m "perf(space): optimize coordinate lookup and canonical ordering"
```

**Definition of Done:**
- [ ] O(n) fallback paths reduced for common workloads.
- [ ] Benchmarks meet target budget.

## Task 13: `murk-arena` Snapshot and Sparse Reuse Optimization

**Files:**
- Modify: `crates/murk-arena/src/read.rs`
- Modify: `crates/murk-arena/src/sparse.rs`
- Modify: `crates/murk-arena/src/write.rs`
- Test: `crates/murk-arena/src/read.rs`
- Test: `crates/murk-arena/src/sparse.rs`
- Bench: `crates/murk-bench/benches/arena_ops.rs`

**Step 1: Reduce publish-time copying in owned snapshots**

Run:
```bash
cargo test -p murk-arena
```

Expected output:
```
PASS with snapshot integrity and determinism unchanged.
```

**Step 2: Improve sparse reuse bookkeeping**

Run:
```bash
cargo bench -p murk-bench --bench arena_ops
```

Expected output:
```
Improved sparse reuse hit behavior and reduced scan overhead.
```

**Step 3: Commit**

```bash
git add crates/murk-arena/src/read.rs crates/murk-arena/src/sparse.rs crates/murk-arena/src/write.rs crates/murk-bench/benches/arena_ops.rs docs/design/performance-budget.md
git commit -m "perf(arena): optimize snapshot copy path and sparse reuse bookkeeping"
```

**Definition of Done:**
- [ ] Arena snapshot performance improves without memory safety regressions.
- [ ] Sparse-heavy workloads show measurable throughput gain.

## Task 14: v0.2 Feature Work Package Kickoff (Post-Gates)

**Files:**
- Modify: `ROADMAP.md`
- Create: `docs/plans/2026-02-22-v0.2-los-render-heterogeneous.md`
- Modify: `crates/murk-space/src/lib.rs` (LOS API surface, when implementation begins)
- Modify: `crates/murk-obs/src/spec.rs` (heterogeneous composition, when implementation begins)

**Step 1: Lock v0.2 scope from roadmap**

Run:
```bash
rg -n "line-of-sight|render|heterogeneous" ROADMAP.md docs/arch-analysis-2026-02-22-1219/09-improvement-roadmap.md
```

Expected output:
```
Shared scope language across roadmap and implementation plan.
```

**Step 2: Write implementation plan for v0.2 features**

Run:
```bash
ls docs/plans | rg "v0.2-los-render-heterogeneous"
```

Expected output:
```
New v0.2 feature plan exists and references Phase 1-3 gate outcomes.
```

**Step 3: Commit**

```bash
git add ROADMAP.md docs/plans/2026-02-22-v0.2-los-render-heterogeneous.md
git commit -m "docs(v0.2): prepare LOS/render/heterogeneous feature package after hardening gates"
```

**Definition of Done:**
- [ ] v0.2 planning is explicitly dependent on completed hardening/perf gates.
- [ ] Feature scope is implementation-ready.

## Critical Path and Dependencies

1. Task 1 -> Task 2/3/4/5/6 (Phase 1 fan-out).
2. Task 2 + Task 3 + Task 4 + Task 5 + Task 6 must complete for Gate A.
3. Task 7 depends on Task 3 (policy semantics) and Task 5/6 (stable contract surfaces).
4. Task 8 depends on Task 7.
5. Task 9 depends on Task 7 and Task 8.
6. Task 10 should start during Task 7/8 and finish before Task 11/12/13.
7. Task 11/12/13 depend on Task 10 baseline budgets.
8. Task 14 depends on Gate C completion.

## Risks and Mitigations

- Risk: FFI policy disagreements delay Phase 1.
  Mitigation: Timebox Task 3 decision to one review cycle; default to `world-fatal` if unresolved.
- Risk: Metrics expansion breaks ABI consumers.
  Mitigation: Add compile-time layout asserts + C header sync tests before merge.
- Risk: Performance work causes hidden correctness regressions.
  Mitigation: Benchmark + determinism tests required in every perf PR.
- Risk: v0.2 scope creep starts before hardening completes.
  Mitigation: Enforce gate checks in epic definition and PR templates.

## Exit Criteria Checklist

- [ ] Phase 1: FFI hardened, replay/docs/packaging drift checks in CI, install docs aligned.
- [ ] Phase 2: realtime counters exposed in Rust/FFI/Python, preflight visibility shipped.
- [ ] Phase 3: perf budgets met for `murk-obs`, `murk-space`, `murk-arena`.
- [ ] Phase 4: v0.2 feature plan approved with explicit dependency on prior gates.
