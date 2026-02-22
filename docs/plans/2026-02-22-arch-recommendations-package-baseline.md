# Architecture Recommendations Baseline (2026-02-22)

## Scope Lock

- Baseline commit: `35f894b` (HEAD at baseline capture).
- Implementation scope for this package is locked to Gate A + Gate B closure (Tasks 1-9).
- Follow-on execution has completed Tasks 10-12 from Gate C; Tasks 13-14 remain deferred.

## Recommendation Classification

### Architecture Assessment (`05-architecture-assessment.md`)

1. `[Resolved]` Spec-surface trust (replay/install drift guards).
2. `[Resolved]` Packaging hygiene and artifact-first validation.
3. `[Resolved]` FFI hardening + poisoning policy/recovery semantics.
4. `[Resolved]` Realtime telemetry across Rust/FFI/Python plus preflight visibility.
5. `[PartiallyResolved]` Phase 3 performance work (`murk-obs` and `murk-space` resolved; `murk-arena` open).

### Technical Debt Catalog (`06-technical-debt-catalog.md`)

- Critical debt items are `[Resolved]`.
- High-priority hardening/telemetry items for Phase 1/2 are `[Resolved]`.
- Remaining scale/perf debt is `[PartiallyResolved]`.
- Python typing drift is `[PartiallyResolved]`.

### Improvement Roadmap (`09-improvement-roadmap.md`)

- Phase 1: `[Resolved]`
- Phase 2: telemetry + preflight `[Resolved]`, ring retention/skew signaling `[Resolved]`
- Phase 3: performance harness/budgets `[Resolved]`, `murk-obs` optimization `[Resolved]`, `murk-space` optimization `[Resolved]`, remaining optimization work `[Open]`
- Phase 4: `[Open]`

## Baseline Validation Commands

### 1) Surface Re-verification

Command:

```bash
rg -n "FORMAT_VERSION|Current version|requires-python|QueueFull|tick_disabled|MurkStepMetrics" \
  docs/replay-format.md crates/murk-replay/src/lib.rs README.md book/src/getting-started.md \
  crates/murk-python/pyproject.toml crates/murk-engine/src crates/murk-ffi/src
```

Result summary:

- Replay version alignment found (`docs/replay-format.md`, `crates/murk-replay/src/lib.rs`).
- Install Python version contract found (`crates/murk-python/pyproject.toml`).
- Realtime/drop/disabled telemetry surfaces found across engine + FFI metrics/status paths.

### 2) Classification-only analysis diff

Command:

```bash
git diff -- docs/arch-analysis-2026-02-22-1219
```

Result summary:

- Diff contains classification/tag updates (`[Open]`, `[PartiallyResolved]`, `[Resolved]`).
- No recommendation removals.

### 3) Workspace health baseline

Command:

```bash
cargo test --workspace
```

Result summary:

- PASS.
- No failing crates/tests at baseline capture time.

## Targeted Validation for Newly Completed Scope

Commands:

```bash
cargo test -p murk-engine -- realtime
cargo test -p murk-ffi
UV_CACHE_DIR=.uv-cache uv run pytest crates/murk-python/tests/test_vec_env.py -q
cargo test -p murk-obs --lib
cargo test -p murk-space
cargo bench -p murk-bench --bench obs_ops --bench space_ops --bench arena_ops -- --sample-size 20 --measurement-time 1
cargo bench -p murk-bench --bench obs_ops -- --sample-size 20 --measurement-time 1
cargo bench -p murk-bench --bench space_ops -- --sample-size 20 --measurement-time 1
```

Result summary:

- PASS for realtime preflight additions in engine.
- PASS for FFI hardening + added negative/integration-like tests.
- PASS for Python vec-env preflight surface test.
- PASS for `murk-obs` correctness suite after allocation/batch-path optimization.
- PASS for `murk-space` correctness suite after canonical-rank fast-path changes.
- PASS for representative Phase 3 benchmark scenarios; baselines captured in `docs/design/performance-budget.md`.
- PASS for `obs_ops` post-optimization run with Task 11 target met (`obs_execute_agents/agent_disk_r3/64`).
- PASS for `space_ops` post-optimization run with Task 12 target met (`space_rank_lookup/product_square4xline1d/4096`).

## Remaining Risks (Deferred to Gates C/D)

- `murk-arena` optimization task (`Task 13`) is not started.
- v0.2 feature-package kickoff plan (`Task 14`) is not started.
