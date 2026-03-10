# Three-Port Architecture Design

**Date**: 2026-02-24
**Status**: Reviewed — 5-specialist panel consensus (4 rounds)
**Scope**: v0.2 roadmap addition

---

## Context

Murk serves three distinct consumers:

| Port | Consumer | Purpose | Current State |
|------|----------|---------|---------------|
| 1 | Python / PyTorch | RL training | **Shipped** (murk-python, murk-ffi) |
| 2 | Graphics engine | Visualization | Planned (snapshot reads work, no scene abstraction) |
| 3 | Game / simulation control | Business logic | Partially served (direct LockstepWorld / RealtimeAsyncWorld API) |

Port 1 is complete. Ports 2 and 3 need dedicated affordances before Echelon
can demonstrate the full engine architecture.

### Key Gaps

1. **No scene description format** — a renderer must understand field layouts
   and spatial coordinates to extract visual data from raw snapshots.
2. **No unified World API** — game logic must choose between `LockstepWorld`
   and `RealtimeAsyncWorld` at compile time, with incompatible APIs and
   lifetime models.
3. **No render pipeline** — the observation pipeline (ObsSpec/ObsPlan)
   produces ML tensors, not human-readable scene descriptions.

---

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Port 3 runtime mode | Both lockstep and async | Different consumers need different modes; "engine stays an engine" |
| Port 3 API pattern | Typestate `WorldHandle<M: Mode>` | Mode-specific methods at compile time; no runtime dispatch overhead; eliminates "Shifting the Burden" archetype |
| Port 2 output format | FlatBuffers scene description | Zero-copy, cross-language; note: murk-obs uses custom MOBS format, not FlatBuffers |
| Port 2 pipeline pattern | RenderSpec → RenderPlan → Scene | Mirrors ObsSpec → ObsPlan → tensor |
| Port 2 static data | SpaceDescriptor (one-time) + Scene (per-tick) | Avoids ~192KB/tick of redundant coordinate data for Fcc12 |
| Crate layout | Three new crates | One job per crate, independent versioning |
| Render ownership | Externalized RenderPlan (caller-owned) | Avoids self-borrow conflict, enables multi-camera, clean Bevy integration |
| Thread safety | `Propagator: Send + Sync` | All existing propagators audited as Sync-safe; enables `Res<WorldHandle>` in Bevy |
| Shared types | Extract Transform/RegionSpec to murk-core | No murk-render → murk-obs dependency |
| FlatBuffer codegen | Checked-in generated code | No build.rs flatc dependency; `scripts/regenerate-flatbuffers.sh` + CI staleness check |

---

## Architecture

### Crate Structure

```
murk-scene    Leaf crate: FlatBuffer schema (SpaceDescriptor + Scene) + generated types
murk-render   RenderSpec -> RenderPlan compilation and execution (caller-owned plans)
murk-handle   WorldHandle<M: Mode> typestate adapter over LockstepWorld / RealtimeAsyncWorld
```

### Dependency Flow

```
murk-core ──┬── murk-arena ──┬── murk-engine ──┬── murk-ffi
            ├── murk-space ──┤                 ├── murk-python
            ├── murk-propagator ─┤             │
            ├── murk-obs ────────┘             │
            ├── murk-scene (new, leaf)         │
            └── murk-render (new) ─────────────┤
                murk-replay ───────────────────┤
                murk-handle (new) ─────────────┘
                    ↑
                murk (umbrella)
```

**Note:** murk-render depends on murk-scene, murk-space, and murk-core.
It does **not** depend on murk-obs. Shared value types (Transform, RegionSpec)
are extracted to murk-core.

### Consumer Topology

```
┌─────────────┐     ┌───────────────┐     ┌─────────────┐
│  PyTorch RL │     │  Bevy / wgpu  │     │  Game Logic  │
│  (training) │     │  (rendering)  │     │  (Echelon)   │
└──────┬──────┘     └───────┬───────┘     └──────┬───────┘
       │                    │                     │
  murk-python          murk-scene        murk-handle<M>
  + murk-ffi          (FlatBuffers)      (WorldHandle)
       │                    │                     │
       └────────────────────┴─────────────────────┘
                            │
                      murk-engine
                     (TickEngine)
```

**Port 2 integration pattern (Bevy):** An exclusive system owns
`ResMut<WorldHandle<M>>` for stepping. A parallel render system reads
`Res<SimSnapshot>` (`Arc<OwnedSnapshot>`) and calls
`plan.execute(&snapshot)` — never touching WorldHandle directly.

---

## Component Details

### murk-scene (Leaf Crate)

FlatBuffer schema defining the scene interchange format. No dependencies on
other murk crates. Renderer adapters depend only on this crate.

**Schema (`scene.fbs`):**

```flatbuffers
namespace murk.scene;

/// One-time spatial metadata (produced at compile time, not per-tick).
table SpaceDescriptor {
  space_type: SpaceType;
  cell_count: uint32;
  ndim: uint8;
  extent: [uint32];
  is_dense: bool = true;
  edge_behavior: EdgeBehavior;
  cell_coords: [int32];  // flattened ndim × cell_count; only for non-dense topologies
  schema_version: uint16;
}

/// Per-tick scene description.
table Scene {
  tick_id: uint64;
  dt: float64;
  scene_generation: uint64;
  entities: [Entity];
  field_layers: [FieldLayer];
  events: [Event];
  entity_manifest: EntityManifest;
}

table Entity {
  id: uint32;
  coords: [int32];
  properties: [float32];
}

table EntityManifest {
  property_names: [string];
  property_types: [PropertyType];
}

table FieldLayer {
  field_id: uint32;
  name: string;
  field_type: FieldType;
  data: [float32];
  valid_mask: [uint8];
  dirty: bool = true;           // hint flag, no behavioral contract in v0.2
  // reserved: dirty_range_start (uint32), dirty_range_count (uint32) — v1.0+
}

table Event {
  kind: EventKind;
  tick_id: uint64;
  entity_id: uint32;
  coords: [int32];
  payload: [float32];
}

enum SpaceType : byte { Line1D, Ring1D, Square4, Square8, Hex2D, Fcc12, Product }
enum EdgeBehavior : byte { Absorb, Clamp, Wrap }
enum FieldType : byte { Scalar, Vector, Categorical }
enum PropertyType : byte { Float, Int, Bool, Enum }
enum EventKind : byte { Spawn, Despawn, FieldWrite, CommandApplied, CommandRejected }
```

**Key design choices:**
- SpaceDescriptor is produced once at plan compilation time, not per tick.
  For Fcc12 (32³ = 32,768 cells × 3 coords × 4 bytes = ~384KB), this
  saves significant bandwidth at 60Hz.
- `dirty: bool = true` on FieldLayer is a hint flag with no behavioral
  contract in v0.2. FlatBuffer default of `true` means existing readers
  are unaffected. Delta semantics deferred to v1.0.
- `entity_manifest` provides named property slots so renderers can
  interpret `properties: [float32]` without out-of-band metadata.
- `scene_generation` binds to world generation for stale-plan detection.

**Rust API:** Generated FlatBuffer types (checked-in, not build.rs) plus a
`SceneBuilder` for ergonomic construction. Regeneration via
`scripts/regenerate-flatbuffers.sh` with CI staleness check.

### murk-render (Render Pipeline)

Mirrors the ObsSpec/ObsPlan pattern. Compiles a render specification into
a reusable execution plan. The plan is **caller-owned** (not stored in
WorldHandle).

**Core types:**

```rust
pub struct RenderSpec {
    pub field_layers: Vec<RenderFieldEntry>,
    pub include_entities: bool,
    pub include_events: bool,
    pub entity_manifest: Option<EntityManifest>,
    pub region: Option<RegionSpec>,
}

pub struct RenderFieldEntry {
    pub field_id: FieldId,
    pub transform: Option<RenderTransform>,  // murk-render's own transform type
    pub downsample: Option<DownsampleSpec>,
}

pub struct RenderPlan { /* precomputed gather indices, field offsets */ }

/// Compilation produces both the reusable plan and one-time spatial metadata.
pub struct RenderPlanResult {
    pub plan: RenderPlan,
    pub space_descriptor: Vec<u8>,  // FlatBuffer SpaceDescriptor bytes
}

impl RenderPlan {
    pub fn compile(
        spec: &RenderSpec,
        space: &dyn Space,
        fields: &[FieldDef],
    ) -> Result<RenderPlanResult, RenderError>;

    /// Per-tick execution. Returns Scene FlatBuffer bytes (no SpaceDescriptor).
    pub fn execute(
        &self,
        snapshot: &dyn SnapshotAccess,
    ) -> Result<Vec<u8>, RenderError>;
}
```

**Design notes:**
- `compile()` returns `RenderPlanResult` containing both the plan and a
  one-time SpaceDescriptor FlatBuffer. Consumers cache the descriptor.
- `execute()` returns per-tick Scene FlatBuffer bytes only. No spatial
  coordinates repeated per tick.
- Region support enables camera/viewport culling.
- Downsample support is v0.2 limitation: works for dense rectangular
  topologies (Square4, Square8) only. Fcc12/Hex2D LOD deferred to v1.0.
- `RenderTransform` is murk-render's own type, not shared with murk-obs.
  ML normalization and visual transforms have different requirements.
- Entity extraction reads from configurable "entity position" fields.
- Event capture is opt-in.

**Dependencies:** murk-scene, murk-space, murk-core.
Does **not** depend on murk-obs.

### murk-handle (WorldHandle Adapter)

Typestate adapter wrapping either `LockstepWorld` or `RealtimeAsyncWorld`.
The phantom type parameter `M: Mode` determines which methods are available
at compile time.

**Mode types:**

```rust
/// Sealed trait for runtime mode selection.
pub trait Mode: sealed::Sealed + Send + 'static {}

/// Synchronous, deterministic stepping. Borrow-checker enforces single-thread.
pub struct Lockstep;
impl Mode for Lockstep {}

/// Asynchronous background tick thread with epoch-based reclamation.
pub struct Async;
impl Mode for Async {}
```

**Core type:**

```rust
pub struct WorldHandle<M: Mode> {
    inner: WorldInner<M>,
    _mode: PhantomData<M>,
}

// Internal: concrete storage, not exposed
enum WorldInner<M: Mode> { /* ... */ }
```

**3-Tier API:**

```rust
// === Tier 1: Mode-specific methods (concrete return types) ===

impl WorldHandle<Lockstep> {
    pub fn lockstep(config: WorldConfig) -> Result<Self, ConfigError>;

    /// Synchronous step: blocks until tick completes, returns deterministic result.
    pub fn step_sync(&mut self, commands: Vec<Command>)
        -> Result<CompletedStep, HandleError>;
}

impl WorldHandle<Async> {
    pub fn realtime(config: WorldConfig, async_config: AsyncConfig)
        -> Result<Self, ConfigError>;

    /// Submit commands and peek latest snapshot. Does NOT block for next tick.
    pub fn submit_and_peek(&mut self, commands: Vec<Command>)
        -> Result<SubmittedStep, HandleError>;

    /// Block until at least one more tick has completed.
    pub fn await_next_tick(&self, timeout: Duration)
        -> Result<TickId, HandleError>;
}

// === Tier 2: Mode-agnostic methods (StepOutcome enum) ===

impl<M: Mode> WorldHandle<M> {
    /// Step with mode-appropriate semantics. Exhaustive match on StepOutcome
    /// forces consumers to handle the mode difference.
    pub fn step(&mut self, commands: Vec<Command>)
        -> Result<StepOutcome, HandleError>;
}

// === Common methods (no mode variance) ===

impl<M: Mode> WorldHandle<M> {
    pub fn snapshot(&self) -> Result<Arc<OwnedSnapshot>, HandleError>;
    pub fn reset(&mut self, seed: u64) -> Result<(), HandleError>;
    pub fn space(&self) -> &dyn Space;

    /// Compile a render plan from a spec. Caller owns the result.
    pub fn compile_render_plan(&self, spec: &RenderSpec)
        -> Result<RenderPlanResult, HandleError>;

    /// Convenience: execute a render plan against the latest snapshot.
    pub fn render(&self, plan: &RenderPlan) -> Result<Vec<u8>, HandleError>;

    // Escape hatches (advanced use only)
    pub fn as_lockstep(&self) -> Option<&LockstepWorld>;
    pub fn as_lockstep_mut(&mut self) -> Option<&mut LockstepWorld>;
    pub fn as_realtime(&self) -> Option<&RealtimeAsyncWorld>;
    pub fn as_realtime_mut(&mut self) -> Option<&mut RealtimeAsyncWorld>;
}
```

**Step outcome types:**

```rust
/// Lockstep step completed: snapshot is the result of this tick.
pub struct CompletedStep {
    pub snapshot: Arc<OwnedSnapshot>,
    pub tick_id: TickId,
    pub receipts: Vec<Receipt>,
    pub metrics: StepMetrics,
}

/// Async commands submitted: snapshot may be from a previous tick.
pub struct SubmittedStep {
    pub latest_snapshot: Arc<OwnedSnapshot>,
    pub submitted_tick: TickId,
    pub snapshot_tick: TickId,  // may differ from submitted_tick
    pub receipts: Vec<Receipt>,
}

/// Mode-agnostic step result for Tier 2 API.
pub enum StepOutcome {
    Completed(CompletedStep),
    Submitted(SubmittedStep),
}
```

The `submitted_tick` / `snapshot_tick` gap in `SubmittedStep` makes the
information delay between command submission and observation explicitly
visible — critical for correct async game logic.

**Error unification:**

```rust
pub enum HandleError {
    Config(ConfigError),
    Tick(TickError),
    Submit(SubmitError),
    Render(RenderError),
}
```

Note: `UnsupportedMode` is eliminated by typestate — invalid operations
are compile errors, not runtime errors.

**Dependencies:** murk-engine, murk-render, murk-scene, murk-core.

---

## Propagator Trait Change

The `Propagator` trait gains a `Sync` bound:

```rust
pub trait Propagator: Send + Sync + 'static {
    fn reads(&self) -> FieldSet;
    fn reads_previous(&self) -> FieldSet;
    fn writes(&self) -> FieldSet;
    fn max_dt(&self, space: &dyn Space) -> f64;
    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError>;
}
```

**Audit results:** All existing propagators (ScalarDiffusion, GradientCompute,
IdentityCopy, FlowField, AgentEmission, ResourceField, MorphologicalOp,
WavePropagation, NoiseInjection) use no `Cell`, `RefCell`, or `Rc`.
All are trivially `Sync`.

**Impact:** `WorldHandle<M>` becomes `Send + Sync`, enabling Bevy's
`Res<WorldHandle>` without `NonSend` workarounds. This is a v0.2
semver-breaking change.

---

## Trade-offs

### OwnedSnapshot Everywhere

The WorldHandle forces `OwnedSnapshot` (Arc + heap-allocated descriptor)
even in lockstep mode, where zero-copy `Snapshot<'w>` is available.

**Scaling curve** (replaces the original "~2μs" claim):

| Scale | Cell count | Fields | Clone cost | % of 16ms frame |
|-------|-----------|--------|------------|-----------------|
| Echelon baseline | ~2,048 | 5 | ~6μs | 0.04% |
| Echelon full | ~16,384 | 10 | ~60μs | 0.4% |
| Medium (256×256) | ~65,536 | 10 | ~625μs | 3.9% |
| Large (512×512) | ~262,144 | 10 | ~2.5ms | 15.6% |

**Conclusion:** Acceptable for Echelon scale. For arenas exceeding ~5MB,
share snapshots via `Arc` without re-cloning (v0.3 optimization target).

### Typestate vs Enum Dispatch

The original design used internal enum dispatch. The review identified this
as a "Shifting the Burden" archetype: hiding mode complexity rather than
resolving it. Typestate makes mode differences visible at compile time.

**Cost:** `WorldHandle<Lockstep>` and `WorldHandle<Async>` are distinct types.
Mode-agnostic code must be generic over `M: Mode` or use Tier 2 `step()`.

**Benefit:** Invalid operations (e.g., `await_next_tick` on lockstep) are
compile errors. No `UnsupportedMode` runtime error variant needed. The
3-tier API provides escape valves at every abstraction level.

### Externalized RenderPlan

The original design stored `RenderPlan` inside `WorldHandle`. The review
identified three independent problems:
1. **Self-borrow conflict:** `self.render_plan.execute(&self.snapshot())`
   borrows `&self` twice.
2. **Multi-camera:** Main viewport + minimap + debug overlay each need
   independent plans.
3. **Test isolation:** Externalized plans are independently testable without
   constructing a WorldHandle.

**Cost:** Caller manages plan lifecycle. `compile_render_plan()` is a
convenience method, not a storage mechanism.

**Benefit:** Clean Bevy integration pattern: exclusive system steps world,
parallel systems each hold their own `RenderPlan` + `Arc<OwnedSnapshot>`.

### FlatBuffers vs Typed Structs

Scene output is raw `Vec<u8>` (FlatBuffer bytes) rather than typed Rust
structs.

**Cost:** Rust consumers must use FlatBuffer accessors (`.entities()`,
`.field_layers()`) instead of direct struct field access.

**Benefit:** Zero-copy cross-language interop. A WASM renderer reads the
same bytes as a native Bevy renderer.

**Note:** Unlike the original design doc claimed, murk-obs uses a custom
MOBS binary format, not FlatBuffers. FlatBuffers for murk-scene is a
genuinely new dependency, chosen for its cross-language value.

---

## Test Strategy

### Pre-Implementation Gates

These must be completed before the main implementation:

1. **Fcc12 smoke tests in murk-obs** — verify existing observation pipeline
   handles Fcc12 topology correctly (baseline for murk-render comparison).
2. **test_harness module** as first murk-handle PR — generic test helpers
   for WorldHandle<Lockstep> and WorldHandle<Async>.
3. **Pinned binary golden file** ships with murk-scene day one — forward
   compatibility test (`v1 read by v2 reader`) from the start.

### High-Value Tests (5 tests, ordered by ROI)

1. **Mode-differential oracle** — run identical 1000-step sequences through
   `WorldHandle<Lockstep>` and `WorldHandle<Async>` (with `await_next_tick`),
   assert snapshot equality after each tick. Catches every mode parity bug.
2. **Render isolation invariant** — run simulation with and without render
   pipeline attached, assert identical tick hashes. Proves Port 2 is
   non-authoritative.
3. **SpaceDescriptor round-trip** — property test across all 7 space types:
   produce descriptor, consume it, verify coordinate reconstruction.
4. **Schema evolution golden files** — pinned binary FlatBuffer files from
   v0.2 day one. Every schema change must pass backward compatibility.
5. **Concurrent multi-plan stress** — 3+ RenderPlans executing against
   shared `Arc<OwnedSnapshot>` from parallel threads. Validates Send+Sync.

### Test Investment Strategy

Focus test effort on **boundary crossings** (WorldHandle → Engine,
RenderPlan → Snapshot, Scene → FlatBuffer) rather than internals.
Delegate internal correctness to murk-engine's existing 700+ tests.

---

## Roadmap Placement

These three crates replace the existing "Render Adapter Interface" bullet
in the v0.2 roadmap section.

**v0.2 additions:**

| Crate | Priority | Scope |
|-------|----------|-------|
| murk-scene | High | Leaf crate, small scope, blocks murk-render |
| murk-render | High | RenderSpec/RenderPlan pipeline (full-scene only, no LOD on non-rectangular) |
| murk-handle | Medium | WorldHandle\<M: Mode\> typestate adapter |
| Propagator: Send+Sync | High | One-line trait change, v0.2 breaking |
| murk-core type extraction | Medium | Transform, RegionSpec shared types |
| Umbrella prelude updates | Low | Re-export new types |

**v0.2 documented limitations:**
- LOD/downsampling only for dense rectangular topologies (Square4, Square8)
- No delta/streaming scene output (full scene per tick)
- OwnedSnapshot scaling acceptable up to ~65K cells; larger worlds need
  profiling before optimization

**v1.0 stability surfaces (added):**
- `SpaceDescriptor` FlatBuffer schema version
- `Scene` FlatBuffer schema version
- `RenderSpec` / `RenderPlan` format
- `WorldHandle<M>` common API surface
- `StepOutcome` / `CompletedStep` / `SubmittedStep` types

**Post-1.0 (unchanged):**
- murk-bevy adapter (depends on murk-scene)
- murk-terminal adapter
- murk-web adapter (WASM + WebGL)
- TorchRL / RLlib integrations
- Non-rectangular LOD (Fcc12 octree, hex shell grouping)
- Delta/streaming scene output
- Arc-caching for OwnedSnapshot (if profiling justifies)

---

## Implementation Notes

Implementation is deferred to a dedicated branch. When ready:

1. Start with murk-scene (leaf, no deps, fast to validate)
2. Extract shared types (Transform, RegionSpec) to murk-core
3. Then murk-render (mirrors murk-obs patterns; uses murk-core types)
4. Then murk-handle (typestate wrapper, mostly delegation)
5. Add `Sync` bound to `Propagator` trait (v0.2 breaking change)
6. Finally, umbrella prelude updates

Each crate should ship with:
- `#![deny(missing_docs)]` and `#![forbid(unsafe_code)]`
- Unit tests for compilation and execution
- Integration tests against LockstepWorld
- `#[cfg(test)]` mock types for error path testing (no internal trait)
- At least one example demonstrating the port

---

## Review History

This design was reviewed by a 5-specialist panel over 4 rounds:

| Reviewer | Expertise | Key Contribution |
|----------|-----------|-----------------|
| arch-reviewer | System architecture, API design | 3-tier API structure, SpaceDescriptor naming, delta field reservations |
| rust-specialist | Rust lifetimes, ownership, traits | Propagator Sync audit, typestate proposal, OwnedSnapshot scaling analysis |
| graphics-specialist | Bevy/wgpu, scene graphs, LOD | SpaceDescriptor one-time split, multi-camera pattern, bandwidth analysis |
| systems-thinker | System dynamics, archetypes | "Shifting the Burden" identification, epoch coupling analysis, leverage points |
| quality-specialist | Test strategy, regression prevention | Pre-implementation gates, 5 high-value tests, mode-differential oracle |

**Round 1:** Independent reviews. 25 findings (5 CRITICAL, 12 IMPORTANT, 8 MINOR).
**Round 2:** Cross-challenge. Compound findings emerged (self-borrow + multi-camera,
typestate + Sync + StepOutcome).
**Round 3:** Consensus ballot. 6/8 changes unanimous, 2 with minor challenges.
**Round 4:** Challenge resolution. All 8 changes accepted. Full consensus.
