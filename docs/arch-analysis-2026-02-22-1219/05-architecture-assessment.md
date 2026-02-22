# Architecture Quality Assessment

## Assessment Summary

- **Quality level:** Good (for v0.1.x), with several high-risk edge cases.
- **Primary pattern:** A layered, library-first simulation kernel with explicit determinism + memory-lifetime contracts, extended via FFI/Python adapters.
- **Severity:** High (not because the core design is broken, but because a small number of edge-path issues can undermine trust: FFI hardening gaps, doc/code drift, and realtime observability/backpressure).
- **Timeline:** Address Phase 1 items before expanding the feature surface for v0.2 (LOS, rendering, heterogeneous sensors), otherwise new features will amplify existing risks.

## Evidence (What’s Working)

- Clear layering and boundaries (core → arena/space → propagator/obs → engine → bindings), with the facade crate as the intended user entrypoint (`Cargo.toml`, `crates/murk/src/lib.rs`).
- Strong safety stance: `#![forbid(unsafe_code)]` across most crates, with `unsafe` constrained to `murk-ffi` and (per docs) arena internals (`crates/*/src/lib.rs`).
- Determinism is treated as a first-class product requirement (determinism contract + replay hashing) (`docs/ARCHITECTURE.md`, `docs/determinism-catalogue.md`, `crates/murk-replay/src/*`).
- CI is comprehensive and aligned with the design goals: MSRV gate, multi-OS tests, clippy `-D warnings`, rustfmt, Miri on arena, cargo-deny, Python tests + example smoke runs (`.github/workflows/*.yml`).

## Architectural Problems (What Limits Fitness)

### 1) Trust Erosion: “Docs/Code Drift” in load-bearing specs

- Replay format documentation does not match code (`docs/replay-format.md` vs `crates/murk-replay/src/lib.rs`), and install guidance diverges across README/book/release automation.
- Fitness impact: external consumers can implement the wrong replay decoder or assume incorrect compatibility guarantees.

### 2) Supply-Chain / Repo Hygiene: tracked build artifacts in the Python package tree

- The repo tracks a platform-specific binary (`crates/murk-python/python/murk/_murk.abi3.so`) and pytest cache files (`crates/murk-python/.pytest_cache/README.md`).
- Fitness impact: undermines the “build from source / reproducible artifacts” story; increases risk of accidental platform coupling and review friction.

### 3) FFI Hardening is Close but Not Finished

- The panic boundary is strong (`ffi_guard!`, `murk_last_panic_message`), but there are remaining input-validation sharp edges (e.g., dimension multiplication and allocation pressure in `murk_obsplan_execute_agents`) (`crates/murk-ffi/src/lib.rs`, `crates/murk-ffi/src/obs.rs`).
- Mutex poisoning causes persistent `InternalError` behavior after a panic while holding locks (`crates/murk-ffi/src/lib.rs`).

### 4) Realtime Mode Needs More Backpressure + Observability

- `IngressQueue` drops on saturation and `TickEngine` can fail-stop after consecutive rollbacks; both are reasonable design choices, but they need “first-class signals” (counters/telemetry) so callers can react (`crates/murk-engine/src/ingress.rs`, `crates/murk-engine/src/tick.rs`).
- Snapshot retention/egress can become hard to diagnose under load (ring behavior + worker-stall mitigation) (`crates/murk-engine/src/ring.rs`, `crates/murk-engine/src/tick_thread.rs`).

### 5) Scale/Perf Hotspots Are Predictable and Fixable

- Observation execution has avoidable per-call allocations (pooling) and batch execution repeats work linearly (`crates/murk-obs/src/plan.rs`, `crates/murk-obs/src/pool.rs`).
- Space region lookup defaults are O(region size) unless backends override, and canonical ordering materialization can become expensive for large/high-D spaces (`crates/murk-space/src/space.rs`, `crates/murk-space/src/product.rs`).
- Arena snapshots may copy more than necessary (owned snapshot strategy) and sparse reuse can degrade with linear scans (`crates/murk-arena/src/read.rs`, `crates/murk-arena/src/sparse.rs`).

## Recommendations (What Must Change Next)

1. **Make the spec surfaces trustworthy again**: reconcile replay format docs with code, and align install/version constraints across README/book/pyproject/release.
2. **Remove tracked binary/caches from the repo** and rely on the release pipeline to produce wheels/sdists (and add CI that tests the built artifacts, not only editable builds).
3. **Finish the FFI hardening pass** (checked arithmetic, bounded allocations, clearer error reporting) and decide on a strategy for poisoning recovery.
4. **Add realtime-first telemetry**: queue saturation, rollbacks/tick-disabled transitions, worker-stall events, ring “not available” rates.
5. **Pay down the obvious perf debt** in `murk-obs`, `murk-space`, and snapshot handling before building v0.2 features that multiply observation complexity (LOS sensors, heterogeneous specs).
