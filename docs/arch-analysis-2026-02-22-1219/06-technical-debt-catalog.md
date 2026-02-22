# Technical Debt Catalog

**Coverage:** Focused on Critical + High items that materially affect correctness, security posture, reliability, and contributor UX.

## Critical Priority (Immediate Action Required)

### [Resolved] FFI: unchecked agent-centers length arithmetic in `murk_obsplan_execute_agents`

- **Evidence:** `crates/murk-ffi/src/obs.rs`
- **Impact:** Client-controlled dimensions can trigger overflow/over-allocation pressure or panics at the FFI boundary, undermining “panic-safe boundary” guarantees and raising security/reliability risk for any C/Python consumer.
- **Effort:** S
- **Category:** Security / Correctness

### [Resolved] Replay format spec drift vs implementation

- **Evidence:** `docs/replay-format.md`, `crates/murk-replay/src/lib.rs`
- **Impact:** External tooling can implement the wrong decoder/encoder, producing silent corruption or false determinism failures; breaks compatibility expectations.
- **Effort:** M
- **Category:** Correctness / Documentation

### [Resolved] Repo hygiene: tracked platform-specific binary and pytest cache in the Python tree

- **Evidence:** `crates/murk-python/python/murk/_murk.abi3.so`, `crates/murk-python/.pytest_cache/README.md`
- **Impact:** Weakens supply-chain trust (binary checked into source), creates platform coupling, and makes review/build behavior harder to reason about.
- **Effort:** S–M
- **Category:** Supply Chain / DX

## High Priority (Next Body of Work)

### [Resolved] Docs: Python version mismatch

- **Evidence:** `README.md`, `book/src/getting-started.md`, `crates/murk-python/pyproject.toml`
- **Impact:** Users follow docs and hit install/runtime failures; increases support burden.
- **Effort:** S
- **Category:** DX / Documentation

### [Resolved] Docs: “published vs install-from-source” mismatch

- **Evidence:** `README.md`, `book/src/getting-started.md`, `.github/workflows/release.yml`
- **Impact:** Confuses users and contributors; makes release posture ambiguous.
- **Effort:** S
- **Category:** DX / Documentation

### [Resolved] Engine: fail-stop semantics need first-class telemetry

- **Evidence:** `crates/murk-engine/src/tick.rs`, `crates/murk-engine/src/ingress.rs`
- **Impact:** In realtime workloads, `tick_disabled` and `QueueFull` behavior can look like “random drops / hangs” without counters and explicit signals; makes production diagnosis difficult.
- **Effort:** M
- **Category:** Reliability / Observability

### [Open] Engine: `BatchedEngine` blocks agent-relative observation specs

- **Evidence:** `crates/murk-engine/src/batched.rs`
- **Impact:** Forces high-throughput training users back to per-world stepping when they need `AgentDisk`/`AgentRect` style observations, negating the “single GIL release” advantage for richer tasks.
- **Effort:** M–L
- **Category:** Performance / Product Capability

### [Open] Observations: avoidable allocations and redundant batch work

- **Evidence:** `crates/murk-obs/src/plan.rs`, `crates/murk-obs/src/pool.rs`
- **Impact:** Increased heap churn and linear scaling penalties for large multi-agent or batched training workloads.
- **Effort:** M
- **Category:** Performance

### [Open] Space/regions: default O(n) lookups and expensive canonical materialization

- **Evidence:** `crates/murk-space/src/space.rs`, `crates/murk-space/src/product.rs`
- **Impact:** Large spaces and high-dimensional product spaces become expensive in observation planning and coordinate→index mapping; constrains “scale-up” roadmap items (LOS sensors, heterogeneous observation plans).
- **Effort:** L
- **Category:** Performance / Scalability

### [Open] Arena: snapshot ownership and sparse reuse hotspots

- **Evidence:** `crates/murk-arena/src/read.rs`, `crates/murk-arena/src/write.rs`, `crates/murk-arena/src/sparse.rs`
- **Impact:** Realtime mode and sparse-heavy simulations can pay unnecessary copying/scanning costs; increases memory bandwidth and reduces headroom.
- **Effort:** L
- **Category:** Performance / Scalability

## Medium Priority (Important, Not Blocking)

### [PartiallyResolved] Python typing surface can drift from PyO3 exports

- **Evidence:** `crates/murk-python/python/murk/_murk.pyi`, `crates/murk-python/src/lib.rs`
- **Impact:** Stale type hints and confusing editor/mypy behavior as the API evolves.
- **Effort:** M
- **Category:** DX

### [Resolved] FFI mutex poisoning recovery strategy is undefined

- **Evidence:** `crates/murk-ffi/src/lib.rs`
- **Impact:** One panic while holding a lock can effectively disable the API for the process lifetime; may be acceptable for “fail closed”, but should be an explicit design choice with a recovery path.
- **Effort:** M
- **Category:** Reliability / UX

## Limitations

- This catalog is based on targeted reading of key subsystems and CI/docs; it does not include profiling data or fuzzing results.
