# Three-Port Architecture Design

**Date**: 2026-02-24
**Status**: Approved (design only; implementation deferred to dedicated branch)
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
| Port 3 API pattern | WorldHandle adapter (enum over modes) | Mode-agnostic game logic without trait vtable overhead |
| Port 2 output format | FlatBuffers scene description | Consistent with murk-obs; zero-copy, cross-language |
| Port 2 pipeline pattern | RenderSpec -> RenderPlan -> Scene | Mirrors ObsSpec -> ObsPlan -> tensor |
| Crate layout | Three new crates | One job per crate, independent versioning |

---

## Architecture

### Crate Structure

```
murk-scene    Leaf crate: FlatBuffer scene schema + generated types
murk-render   RenderSpec -> RenderPlan compilation and execution
murk-handle   WorldHandle adapter over LockstepWorld / RealtimeAsyncWorld
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

### Consumer Topology

```
┌─────────────┐     ┌───────────────┐     ┌─────────────┐
│  PyTorch RL │     │  Bevy / wgpu  │     │  Game Logic  │
│  (training) │     │  (rendering)  │     │  (Echelon)   │
└──────┬──────┘     └───────┬───────┘     └──────┬───────┘
       │                    │                     │
  murk-python          murk-scene            murk-handle
  + murk-ffi          (FlatBuffers)         (WorldHandle)
       │                    │                     │
       └────────────────────┴─────────────────────┘
                            │
                      murk-engine
                     (TickEngine)
```

---

## Component Details

### murk-scene (Leaf Crate)

FlatBuffer schema defining the scene interchange format. No dependencies on
other murk crates. Renderer adapters depend only on this crate.

**Schema (`scene.fbs`):**

```flatbuffers
namespace murk.scene;

table Scene {
  tick_id: uint64;
  dt: float64;
  space_type: SpaceType;
  cell_count: uint32;
  entities: [Entity];
  field_layers: [FieldLayer];
  events: [Event];
}

table Entity {
  id: uint32;
  coords: [int32];
  properties: [float32];
}

table FieldLayer {
  field_id: uint32;
  name: string;
  field_type: FieldType;
  data: [float32];
  valid_mask: [uint8];
}

table Event {
  kind: EventKind;
  tick_id: uint64;
  entity_id: uint32;
  coords: [int32];
  payload: [float32];
}

enum SpaceType : byte { Line1D, Ring1D, Square4, Square8, Hex2D, Fcc12, Product }
enum FieldType : byte { Scalar, Vector, Categorical }
enum EventKind : byte { Spawn, Despawn, FieldWrite, CommandApplied, CommandRejected }
```

**Rust API:** Generated FlatBuffer types plus a `SceneBuilder` for ergonomic
construction.

### murk-render (Render Pipeline)

Mirrors the ObsSpec/ObsPlan pattern. Compiles a render specification into
a reusable execution plan that produces FlatBuffer scene descriptions from
snapshots.

**Core types:**

```rust
pub struct RenderSpec {
    pub field_layers: Vec<RenderFieldEntry>,
    pub include_entities: bool,
    pub include_events: bool,
    pub region: Option<RegionSpec>,
}

pub struct RenderFieldEntry {
    pub field_id: FieldId,
    pub transform: Option<Transform>,
    pub downsample: Option<DownsampleSpec>,
}

pub struct RenderPlan { /* precomputed gather indices, field offsets */ }

impl RenderPlan {
    pub fn compile(
        spec: &RenderSpec,
        space: &dyn Space,
        fields: &[FieldDef],
    ) -> Result<Self, RenderError>;

    pub fn execute(
        &self,
        snapshot: &dyn SnapshotAccess,
    ) -> Result<Vec<u8>, RenderError>;
}
```

**Design notes:**
- `execute()` returns raw FlatBuffer bytes for zero-copy consumer access.
- Region support enables camera/viewport culling.
- Downsample support enables LOD for large worlds.
- Entity extraction reads from configurable "entity position" fields.
- Event capture is opt-in.

**Dependencies:** murk-scene, murk-obs (shared Transform, RegionSpec), murk-core.

### murk-handle (WorldHandle Adapter)

Mode-agnostic facade wrapping either `LockstepWorld` or `RealtimeAsyncWorld`.
Forces `OwnedSnapshot` at the boundary (~2us clone cost in lockstep mode).

**Core type:**

```rust
pub struct WorldHandle {
    inner: WorldInner,
    render_plan: Option<RenderPlan>,
}

enum WorldInner {
    Lockstep(LockstepWorld),
    Realtime(RealtimeAsyncWorld),
}
```

**Common API:**

```rust
impl WorldHandle {
    // Construction
    pub fn lockstep(config: WorldConfig) -> Result<Self, ConfigError>;
    pub fn realtime(config: WorldConfig, async_config: AsyncConfig)
        -> Result<Self, ConfigError>;

    // Render pipeline
    pub fn set_render_spec(&mut self, spec: RenderSpec) -> Result<(), RenderError>;

    // Simulation control
    pub fn submit_commands(&mut self, commands: Vec<Command>)
        -> Result<Vec<Receipt>, HandleError>;
    pub fn step(&mut self, commands: Vec<Command>)
        -> Result<HandleStepResult, HandleError>;
    pub fn snapshot(&self) -> Result<Arc<OwnedSnapshot>, HandleError>;
    pub fn render(&self) -> Result<Vec<u8>, HandleError>;
    pub fn reset(&mut self, seed: u64) -> Result<(), HandleError>;

    // Introspection
    pub fn mode(&self) -> WorldMode;
    pub fn space(&self) -> &dyn Space;

    // Escape hatches for mode-specific features
    pub fn as_lockstep(&self) -> Option<&LockstepWorld>;
    pub fn as_lockstep_mut(&mut self) -> Option<&mut LockstepWorld>;
    pub fn as_realtime(&self) -> Option<&RealtimeAsyncWorld>;
    pub fn as_realtime_mut(&mut self) -> Option<&mut RealtimeAsyncWorld>;
}
```

**Semantic note:** `step()` in async mode submits commands and returns the
latest snapshot. It does not block until the next tick completes. This is
a pragmatic compromise for API uniformity — consumers who need tick-precise
control should use the escape hatches.

**Error unification:**

```rust
pub enum HandleError {
    Config(ConfigError),
    Tick(TickError),
    Submit(SubmitError),
    Render(RenderError),
    UnsupportedMode { mode: WorldMode, operation: &'static str },
}
```

**Dependencies:** murk-engine, murk-render, murk-scene, murk-core.

---

## Trade-offs

### OwnedSnapshot Everywhere

The WorldHandle forces `OwnedSnapshot` (Arc + heap-allocated descriptor)
even in lockstep mode, where zero-copy `Snapshot<'w>` is available.

**Cost:** ~2us per tick for the descriptor clone. Negligible for game logic
and render consumers (sub-microsecond is only relevant for the ML hot path,
which uses Port 1 directly).

**Benefit:** Snapshots can be freely stored, shared across threads, and
passed to render pipelines without lifetime complications.

### Escape Hatches

`as_lockstep()` / `as_realtime()` break the mode-agnostic abstraction.

**Risk:** Consumer code becomes mode-dependent, defeating the purpose.

**Mitigation:** Document that escape hatches are for advanced use cases
(preflight telemetry, shutdown FSM, borrow-checker guarantees). The common
API should suffice for 90% of game logic.

### FlatBuffers vs Typed Structs

Scene output is raw `Vec<u8>` (FlatBuffer bytes) rather than typed Rust
structs.

**Cost:** Rust consumers must use FlatBuffer accessors (`.entities()`,
`.field_layers()`) instead of direct struct field access.

**Benefit:** Zero-copy cross-language interop. A WASM renderer reads the
same bytes as a native Bevy renderer. Consistent with the obs pipeline.

---

## Roadmap Placement

These three crates replace the existing "Render Adapter Interface" bullet
in the v0.2 roadmap section. They are concrete implementations of that
planned feature.

**v0.2 additions:**

| Crate | Priority | Scope |
|-------|----------|-------|
| murk-scene | High | Leaf crate, small scope, blocks murk-render |
| murk-render | High | RenderSpec/RenderPlan pipeline |
| murk-handle | Medium | WorldHandle adapter |
| Umbrella prelude updates | Low | Re-export new types |

**v1.0 stability surfaces (added):**
- `Scene` FlatBuffer schema version
- `RenderSpec` / `RenderPlan` format
- `WorldHandle` common API

**Post-1.0 (unchanged):**
- murk-bevy adapter (depends on murk-scene)
- murk-terminal adapter
- murk-web adapter (WASM + WebGL)
- TorchRL / RLlib integrations

---

## Implementation Notes

Implementation is deferred to a dedicated branch. When ready:

1. Start with murk-scene (leaf, no deps, fast to validate)
2. Then murk-render (mirrors murk-obs patterns closely)
3. Then murk-handle (wraps existing types, mostly delegation)
4. Finally, umbrella prelude updates

Each crate should ship with:
- `#![deny(missing_docs)]` and `#![forbid(unsafe_code)]`
- Unit tests for compilation and execution
- Integration tests against LockstepWorld
- At least one example demonstrating the port
