# Determinism Catalogue (R-DET-6)

Living document cataloging known sources of non-determinism and the
mitigations applied in the Murk simulation framework.

## Sources of Non-Determinism

### 1. HashMap / HashSet Iteration Order

**Risk**: `HashMap` and `HashSet` use randomized hashing by default.
Iterating over them produces different orderings across runs.

**Mitigation**: Banned project-wide via `clippy.toml`:
```toml
disallowed-types = [
    { path = "std::collections::HashMap", reason = "Use IndexMap for deterministic iteration" },
    { path = "std::collections::HashSet", reason = "Use IndexSet for deterministic iteration" },
]
```
All code uses `IndexMap` / `BTreeMap` instead.

**Verification**: `cargo clippy` enforces this at CI time.

---

### 2. Floating-Point Reassociation

**Risk**: Compilers may reorder floating-point operations for performance
(e.g., `-ffast-math`), producing different results across builds.

**Mitigation**:
- Rust does not enable fast-math by default.
- All arithmetic uses explicit operation ordering (no auto-vectorization
  that could reassociate).
- Build metadata is recorded in the replay header, enabling detection
  of toolchain differences.

**Verification**: Replay header stores `BuildMetadata.compile_flags` and
`BuildMetadata.toolchain`.

---

### 3. Sort Stability

**Risk**: Unstable sorts may produce different orderings for equal elements
across implementations or runs.

**Mitigation**:
- Command ordering uses `priority_class` (primary), `source_id` (secondary),
  `arrival_seq` (final tiebreaker) — all fields are distinct.
- Agent actions are sorted by `agent_id` before processing.
- All sorts use stable sort (`sort_by_key` / `sort_by`).

**Verification**: Scenario 2 (multi-source command ordering) exercises
3 sources with 1000 ticks.

---

### 4. Thread Scheduling

**Risk**: In multi-threaded modes, OS thread scheduling is non-deterministic.

**Mitigation**:
- Lockstep mode is single-threaded by design. All propagators execute
  sequentially in pipeline order.
- RealtimeAsync mode (future) will use epoch-synchronized snapshots
  and deterministic command ordering at tick boundaries.

**Status**: N/A for Lockstep (current scope). Future concern for RealtimeAsync.

---

### 5. Arena Recycling

**Risk**: Memory recycling patterns could theoretically affect state if
buffer reuse is order-dependent.

**Mitigation**:
- PingPong buffer swap is deterministic: generation N always writes to
  buffer `N % 2`, reads from `(N-1) % 2`.
- Arena allocations are generation-indexed, not address-indexed.
- Ring buffer recycling is deterministic (circular index modulo ring size).

**Verification**: Scenario 4 (arena double-buffer recycling) runs 1100 ticks
to exercise multiple full ring buffer cycles.

---

### 6. RNG Seed

**Risk**: Different seeds produce different simulation trajectories.

**Mitigation**:
- Seed is stored in the replay header (`InitDescriptor.seed`).
- Replay reconstruction uses the same seed.
- `config_hash()` includes the seed.

**Verification**: All scenarios use explicit seeds and verify hash equality.

---

### 7. Build Metadata Differences

**Risk**: Different compilers, optimization levels, or target architectures
may produce different floating-point results for the same source code.

**Mitigation**:
- `BuildMetadata` is recorded in every replay file: `toolchain`,
  `target_triple`, `murk_version`, `compile_flags`.
- Replay consumers can warn or reject when metadata doesn't match.

**Status**: Detection only. Cross-build determinism is not guaranteed
and is explicitly documented as a known limitation.

---

### 8. Command Serialization Fidelity

**Risk**: Fields like `expires_after_tick` and `arrival_seq` are
runtime-only and should not affect determinism if excluded.

**Mitigation**:
- `expires_after_tick` is NOT serialized in replays. On deserialization,
  it is set to `TickId(u64::MAX)` (never expires).
- `arrival_seq` is NOT serialized. Set to `0` on deserialization.
  The ingress pipeline assigns fresh arrival sequences.
- Only `payload`, `priority_class`, `source_id`, `source_seq` are recorded.

**Verification**: Proptest round-trip tests verify command serialization
preserves all payload data. Integration tests verify replay produces
identical snapshots despite sentinel values.

---

## Verified Scenarios

| # | Scenario | Ticks | Status |
|---|----------|-------|--------|
| 1 | Sequential-commit vs Jacobi | 1000 | PASS |
| 2 | Multi-source command ordering | 1000 | PASS |
| 3 | WriteMode::Incremental | 1000 | PASS |
| 4 | Arena double-buffer recycling | 1100 | PASS |
| 5 | Sparse field modification | 1000 | PASS |
| 6 | Tick rollback recovery | 100 | PASS |
| 7 | GlobalParameter mid-episode | 1000 | PASS |
| 8 | 10+ propagator pipeline | 1000 | PASS |
