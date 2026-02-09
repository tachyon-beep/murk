# Murk World Engine — Design Decisions v3.1

**Status:** Approved (4-expert panel, unanimous consensus on all 5 decisions)
**Date:** 2026-02-09
**Panel:** Systems Architect, Simulation Engineer, Systems Thinker, DRL Integration Specialist
**Source:** Review of HLD v3.0.1 and Implementation Plan v1.0

---

## Context

Five open design decisions were identified during review of the HLD and Implementation Plan. Each was resolved through a structured cross-challenge process: independent expert recommendations → targeted cross-challenge between disagreeing experts → convergence. All 5 reached consensus (4:0 or 3:1 with concession).

---

## Decision B: Arena Unsafe Strategy

**Resolution:** `#![deny(unsafe_code)]` in `murk-arena` with per-function audited `#[allow]` in a bounded `raw.rs` module (≤5 functions). Phased rollout. `FullWriteGuard` for debug-mode validation.

### Mechanism

**Phase 1 (WP-2 through WP-5):** `Vec<f32>` zero-init for all arena allocations. Safe during early development when arena and propagator pipeline are both under active development. `FullWriteGuard` does not exist yet; zero-init provides a safety net.

**Phase 2 (after WP-4 delivers pipeline + guard):** Migrate to `MaybeUninit<f32>` + `FullWriteGuard`.

- `crates/murk-arena/src/raw.rs` — the ONLY file with `#[allow(unsafe_code)]`
- ≤5 unsafe functions: `alloc_uninit`, `assume_init`, segment pointer math, bump-pointer reset
- Each function has a `// SAFETY:` comment and Miri coverage
- Public API of `murk-arena` is 100% safe (`ReadArena::resolve() -> &[f32]`, etc.)

**FullWriteGuard:**
- Debug builds: `BitVec` coverage tracking per cell. Panics on drop if any `Full` write buffer was incompletely written, with diagnostic: propagator name, field ID, write coverage %.
- Release builds: compiles to bare `&mut [MaybeUninit<f32>]`. Zero overhead.

**Safety net:** `cfg(feature = "zero-init-arena")` forces `Vec<f32>` zero-init in production. Permanent opt-in for users who prefer safety over ~5μs/tick.

### Rationale

Cross-challenge killed the zero-init-everywhere position: **zeros mask incomplete-write bugs (silent wrong answer for RL) while FullWriteGuard catches them (immediate panic with diagnostics).** Zero-init is a symptomatic fix (Shifting the Burden archetype); the guard is the fundamental fix.

### Changes Required

- **HLD §5.3:** Update "None (borrow checker verifies)" to acknowledge bounded unsafe in `raw.rs`
- **Implementation Plan §1.4:** Change `forbid(unsafe_code)` to `deny(unsafe_code)` for `murk-arena`
- **WP-2:** Add Phase 1/Phase 2 migration and FullWriteGuard as deliverables
- **WP-4:** Add FullWriteGuard as acceptance criterion

---

## Decision E: Graceful Shutdown Protocol

**Resolution:** Mode-specific drain-then-join with 4 internal states for RealtimeAsync, trivial `Drop` for Lockstep, bounded timeouts ≤300ms total, reuse §8.3 stalled-worker machinery.

### Lockstep Shutdown

```
Running → Dropped
```

`LockstepWorld` implements `Drop`. `&mut self` guarantees no outstanding borrows. Arena reset reclaims all memory. No threads to join, no queues to drain. In-flight commands are impossible (step_sync is synchronous).

### RealtimeAsync Shutdown

```
Running → Draining → Quiescing → Dropped
```

**Phase 1 — Draining (Running → Draining):**
- Close ingress queue (reject new commands with `MURK_ERROR_SHUTTING_DOWN`).
- TickEngine completes current tick (tick atomicity preserved), publishes final snapshot, then stops.
- Bounded: 2× tick budget (~33ms at 60Hz). Force-stop on timeout (abandon staging = rollback).

**Phase 2 — Quiescing (Draining → Quiescing):**
- Signal cancellation to all egress workers (cooperative flag — reuse §8.3 mechanism).
- Wait for all workers to reach quiescent point (release epoch references).
- Stalled workers get §8.3 teardown (same mechanism as normal operation).
- Bounded: 2× `max_epoch_hold` (~200ms).

**Phase 3 — Dropped (Quiescing → Dropped):**
- All epoch references released. Ring buffer safe to drop.
- Join TickEngine thread + egress workers.
- Drop arenas. Return `ShutdownReport`.

**Critical ordering invariant:** Arena deallocation MUST happen AFTER all epoch references are released. The §8.3 `all_epochs_quiesced()` check enforces this.

**ShutdownResult:**
```rust
pub enum ShutdownResult {
    Clean { report: ShutdownReport },
    TimedOut { phase: ShutdownPhase, report: ShutdownReport },
}
pub enum ShutdownPhase { Draining, Quiescing }
pub struct ShutdownReport {
    pub final_tick_id: TickId,
    pub commands_dropped: u32,
    pub workers_stalled: u32,
    pub shutdown_duration_ms: u32,
}
```

**External API:**
- Internal states are `pub(crate)` with `#[cfg(test)]` accessors
- C ABI: `murk_destroy()` = `shutdown().wait(DEFAULT_TIMEOUT)`, blocking
- Python: context manager `__exit__` calls `murk_destroy()`. GIL released during blocking wait.
- Worst-case shutdown time: ~243ms (33ms drain + 200ms quiesce + 10ms join)

### New Error Code

`MURK_ERROR_SHUTTING_DOWN` — returned for commands arriving during Draining or Quiescing phases.

### Changes Required

- **HLD §9.7:** Add `MURK_ERROR_SHUTTING_DOWN` to minimum error code set
- **HLD §24:** Update I-6 status to "Resolved"
- **Implementation Plan:** Add shutdown protocol to WP-6 (Lockstep) and WP-12 (RealtimeAsync)

---

## Decision J: Re-Enqueue Loop Prevention

**Resolution:** No re-enqueue in RealtimeAsync. Drop commands with `TICK_ROLLBACK` reason code. `tick_disabled` flag after 3 consecutive rollbacks. Observability counter via C ABI. Lockstep: error returns to caller.

### RealtimeAsync Tick Rollback Behavior

```rust
match pipeline.execute(&mut state, &mut staging, commands, dt) {
    Ok(()) => {
        arena.publish(staging);
        generation += 1;
        consecutive_rollback_count = 0;
    }
    Err(TickError::PropagatorFailed { name, reason }) => {
        drop(staging);  // abandon — state unchanged, zero-cost
        // DO NOT re-enqueue. Drop all commands with TICK_ROLLBACK.
        for cmd in commands {
            receipts.push(Receipt::rejected(cmd, ReasonCode::TICK_ROLLBACK));
        }
        consecutive_rollback_count += 1;
        if consecutive_rollback_count >= MAX_CONSECUTIVE_ROLLBACKS {
            tick_disabled.store(true, Ordering::Release);
            log!(CRITICAL, "Tick disabled after {} consecutive rollbacks (last: {} in {})",
                 consecutive_rollback_count, name, reason);
        }
    }
}
```

**tick_disabled mechanism:**
1. Track `consecutive_rollback_count` (always, for telemetry).
2. After 3 consecutive rollbacks: set `tick_disabled: AtomicBool`. Log CRITICAL.
3. TickEngine stops executing ticks (thread stays alive for shutdown).
4. Ingress rejects commands with `MURK_ERROR_TICK_DISABLED`.
5. Egress continues serving last good snapshot (P-1 satisfied).
6. Recovery: caller calls `reset()` (clears flag) or destroys world.

**Why not re-enqueue:**
- Logic errors: same commands trigger same failure → infinite loop.
- Stale basis: re-enqueued commands have stale `basis_tick_id`.
- TTL violations: re-enqueued commands may have expired `expires_after_tick`.
- Ordering ambiguity: re-enqueue position is a non-determinism source for replay.
- Self-healing: agents observe unchanged state (P-1) and naturally resubmit.

**Lockstep:** `step_sync()` returns `Err(StepError::PropagatorFailed)`. Caller decides. No `tick_disabled` mechanism needed.

### New Error Code and C ABI Function

- `MURK_ERROR_TICK_DISABLED` — returned when tick_disabled flag is set
- `murk_consecutive_rollbacks(world: *const MurkWorld) -> u32` — query rollback count

### Changes Required

- **HLD §9.1:** Replace re-enqueue with drop-on-rollback for RealtimeAsync
- **HLD §9.7:** Add `MURK_ERROR_TICK_DISABLED` to minimum error code set
- **Implementation Plan WP-5:** Add tick_disabled mechanism and rollback counter
- **Implementation Plan WP-12:** Add tick_disabled to RealtimeAsync deliverables

---

## Decision M: Space Dispatch Strategy

**Resolution:** `&dyn Space` in `StepContext` for uniform propagator access. `Space: Any + Send + 'static` enables `downcast_ref::<T>()` for opt-in specialization. ObsPlan inner loops use precomputed index tables (no Space calls in hot path).

### Mechanism

```rust
pub trait Space: Any + Send + 'static {
    fn neighbours(&self, coord: &Coord) -> SmallVec<[Coord; 8]>;
    fn distance(&self, a: &Coord, b: &Coord) -> f64;
    fn compile_region(&self, spec: &RegionSpec) -> RegionPlan;
    fn iter_region(&self, plan: &RegionPlan) -> Box<dyn Iterator<Item = Coord> + '_>;
    fn map_coord_to_tensor_index(&self, coord: &Coord, plan: &MappingPlan) -> Option<usize>;
    fn canonical_ordering_spec(&self) -> OrderingSpec;
    // ...
}

impl dyn Space {
    pub fn downcast_ref<T: Space>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref::<T>()
    }
}
```

**Default path:** `&dyn Space` — works for all topologies (built-in and custom). ~2-4ns vtable overhead per call, amortized by ~50ns propagator computation per cell. <8% of per-cell cost.

**Opt-in fast path:** Performance-critical propagators specialize:
```rust
fn step(&self, ctx: &StepContext<'_, impl FieldReader, impl FieldWriter>, dt: f64) -> Result<(), PropagatorError> {
    if let Some(sq4) = ctx.space.downcast_ref::<Square4Space>() {
        self.step_square4(sq4, ctx, dt)  // monomorphized, all inlined
    } else {
        self.step_generic(ctx, dt)  // dyn dispatch fallback
    }
}
```

**ObsPlan:** Precomputed index tables at compilation time. Gather loop uses `index_table[i]` — no Space calls in the hot path. No dispatch overhead of any kind.

**ProductSpace:** Stores components as `Vec<Box<dyn Space>>`. Vtable dispatch handles nested components naturally (unlike enum dispatch, which requires recursive matching).

### Why Not Enum Dispatch

Simulation engineer initially proposed enum dispatch (`SpaceKind`). Cross-challenge identified two problems:
1. ProductSpace nesting creates recursive match statements that scale poorly.
2. `Custom(Box<dyn Space>)` fallback creates a two-tier performance model, discouraging custom topology research (violates R-SPACE-2 "pluggable topology" goal).

### Changes Required

- **Implementation Plan §6.2:** Update `StepContext.space` from `Box<dyn Space>` to `&'a dyn Space`
- **Implementation Plan WP-3:** Add `Space: Any + Send + 'static` requirement
- **Implementation Plan WP-4:** Add `downcast_ref` documentation to Propagator trait
- **Implementation Plan WP-10b:** ProductSpace stores components as `Vec<Box<dyn Space>>`

---

## Decision N: ObsPlan Coupling to Arena

**Resolution:** `SnapshotAccess` trait in `murk-core` with `&dyn SnapshotAccess` dispatch. ObsPlan reads through the trait, not directly from `ReadArena`. `murk-obs` depends on `(core, space)`, NOT `(core, arena, space)`.

### Trait Definition

```rust
// In murk-core
pub trait SnapshotAccess {
    fn read_field(&self, field: FieldId) -> Option<&[f32]>;
    fn tick_id(&self) -> TickId;
    fn world_generation_id(&self) -> WorldGenerationId;
    fn parameter_version(&self) -> ParameterVersion;
}
```

- 4 methods — minimal surface
- `&dyn SnapshotAccess` dispatch (consistent with `&dyn Space` from Decision M)
- Trait lives in `murk-core` (no circular dependencies)
- `ReadArena`-backed `Snapshot` implements the trait in `murk-arena`
- `MockSnapshot` implements the trait in `murk-test-utils` with `Vec<f32>` backing
- Engine-internal only — not exposed through C ABI or Python

### Rationale

The cross-challenge established three arguments for the trait:

1. **Test isolation:** ObsPlan is the most user-visible hot path. Silent tensor corruption = silent RL training failure. `MockSnapshot` isolates ObsPlan testing from arena bugs, enabling rapid iteration on hex tensor export (WP-10a, risk-flagged as "deceptively hard").

2. **Dependency graph simplification:** `murk-obs` depends on `(core, space)` instead of `(core, arena, space)`. This removes a dependency on the highest-risk crate (`murk-arena`) and enables ObsPlan development before the arena is stable.

3. **Consistency:** The `FieldReader`/`FieldWriter` pattern for propagator decoupling (plan consensus #2) establishes the precedent. `SnapshotAccess` follows the same pattern: trait in murk-core, implementation in downstream crate, consumer uses `&dyn`.

### Vtable Cost

~80 `read_field` calls per tick × ~2ns vtable overhead = **160ns/tick** (0.5% of gather work at reference profile). Negligible.

### Changes Required

- **Implementation Plan §1.2:** Update murk-obs dependencies to `(core, space)` — remove arena
- **Implementation Plan §1.3:** Add `SnapshotAccess` trait to murk-core contents
- **Implementation Plan WP-1:** Add `SnapshotAccess` trait to deliverables
- **Implementation Plan WP-0:** Add `MockSnapshot` to murk-test-utils deliverables
- **Implementation Plan §6.1:** Add `SnapshotAccess` to murk-core interface contract
- **Implementation Plan §6.2:** Update ObsPlan to use `&dyn SnapshotAccess`

---

## Cross-Decision Consistency

The 5 decisions form a coherent architectural pattern:

| Pattern | Decisions |
|---------|-----------|
| **`&dyn` dispatch for engine-internal trait boundaries** | M (Space), N (SnapshotAccess) |
| **Traits in murk-core, implementations elsewhere** | M, N, existing (FieldReader/FieldWriter) |
| **Mode duality drives distinct mechanisms** | B (feature flag), E (Lockstep Drop vs RealtimeAsync state machine), J (caller decides vs tick_disabled) |
| **Debug-mode validation, release-mode trust** | B (FullWriteGuard) |
| **Precomputed over runtime dispatch** | M (ObsPlan index tables), B (ReadResolutionPlan) |
| **Reuse existing machinery** | E (§8.3 for shutdown), J (natural RL retry loop for self-healing) |

---

## Emergent Improvements

Four mechanisms emerged from cross-challenge that no individual expert proposed:

1. **FullWriteGuard** (Decision B) — debug-mode BitVec coverage tracking that catches incomplete Full writes. From debate between architect and simulation engineer.
2. **ShutdownResult with phase reporting** (Decision E) — distinguishes "tick hung" from "egress stalled" for operators. From debate between architect and systems thinker.
3. **Phased unsafe migration** (Decision B) — Vec zero-init during early development, MaybeUninit after pipeline proven. Compromise proposed by systems thinker.
4. **Dependency graph simplification via SnapshotAccess** (Decision N) — murk-obs no longer depends on murk-arena. Emerged from full panel convergence.
