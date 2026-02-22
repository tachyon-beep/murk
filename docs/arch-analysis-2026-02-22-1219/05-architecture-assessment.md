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

- Observation execution allocation/batch hotspots in `murk-obs` have been reduced; remaining scale debt is now concentrated in `murk-space` and `murk-arena`.
- Space region lookup defaults are O(region size) unless backends override, and canonical ordering materialization can become expensive for large/high-D spaces (`crates/murk-space/src/space.rs`, `crates/murk-space/src/product.rs`).
- Arena snapshots may copy more than necessary (owned snapshot strategy) and sparse reuse can degrade with linear scans (`crates/murk-arena/src/read.rs`, `crates/murk-arena/src/sparse.rs`).

## Recommendations (What Must Change Next)

1. **[Resolved] Make the spec surfaces trustworthy again**: replay format/docs drift and install/version contract checks are now CI-gated.
2. **[Resolved] Remove tracked binary/caches from the repo** and validate artifact-first packaging via release smoke tests.
3. **[Resolved] Finish the FFI hardening pass** (checked arithmetic, bounded allocations, stable error reporting) and implement a documented poisoning recovery policy.
4. **[Resolved] Add realtime-first telemetry**: queue saturation, rollbacks/tick-disabled transitions, worker-stall events, ring “not available” rates, and ring retention/skew signals are exposed in Rust/FFI/Python metrics plus realtime preflight.
5. **[PartiallyResolved] Pay down the obvious perf debt**: `murk-obs` is improved; `murk-space` and snapshot handling remain before building v0.2 features that multiply observation complexity (LOS sensors, heterogeneous specs).
