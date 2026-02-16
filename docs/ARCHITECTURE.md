# Murk Architecture

This document explains Murk's architecture for developers who want to
understand how the engine works internally. For a practical introduction
to building simulations, see [CONCEPTS.md](CONCEPTS.md).

---

## Table of Contents

- [Design Goals](#design-goals)
- [Crate Structure](#crate-structure)
- [Three-Interface Model](#three-interface-model)
- [Arena-Based Generational Allocation](#arena-based-generational-allocation)
- [Runtime Modes](#runtime-modes)
- [Threading Model](#threading-model)
- [Spatial Model](#spatial-model)
- [Field Model](#field-model)
- [Propagator Pipeline](#propagator-pipeline)
- [Observation Pipeline](#observation-pipeline)
- [Command Model](#command-model)
- [Error Handling and Recovery](#error-handling-and-recovery)
- [Determinism](#determinism)
- [Language Bindings](#language-bindings)

---

## Design Goals

Murk is a world simulation engine for reinforcement learning and
real-time applications. The architecture optimises for:

- **Deterministic replay** — identical inputs produce identical outputs
  across runs on the same platform.
- **Zero-GC memory management** — arena allocation with predictable
  lifetimes, no garbage collection pauses.
- **ML-native observation extraction** — pre-compiled observation plans
  that produce fixed-shape tensors directly, not intermediate
  representations.
- **Two runtime modes** from one codebase — synchronous lockstep for
  training, asynchronous real-time for live interaction.

Three principles guide every subsystem:

1. **Egress Always Returns** — observation extraction never blocks
   indefinitely, even during tick failures or shutdown. Responses may
   indicate staleness or degraded coverage via metadata, but always
   return data.
2. **Tick-Expressible Time** — all engine-internal time references that
   affect state transitions are expressed in tick counts, never wall
   clocks. This prevents replay divergence.
3. **Asymmetric Mode Dampening** — staleness and overload are handled
   differently in each runtime mode, because Lockstep and RealtimeAsync
   have fundamentally different dynamics.

---

## Crate Structure

```text
murk/
├── murk              Top-level facade (add this one dependency)
├── murk-core          Leaf crate: IDs, field defs, commands, core traits
├── murk-arena         Arena-based generational allocation
├── murk-space         Spatial backends and region planning
├── murk-propagator    Propagator trait, pipeline validation, StepContext
├── murk-propagators   Reference propagators (diffusion, movement, reward)
├── murk-obs           Observation spec, compilation, tensor extraction
├── murk-engine        Simulation engine: LockstepWorld, RealtimeAsyncWorld
├── murk-replay        Deterministic replay recording and verification
├── murk-ffi           C ABI bindings with handle tables
├── murk-python        Python/PyO3 bindings with Gymnasium adapters
├── murk-bench         Benchmark profiles and utilities
└── murk-test-utils    Shared test fixtures
```

Dependency flow (arrows point from dependee to dependent):

```text
murk-core ──┬── murk-arena ──┬── murk-engine ──┬── murk-ffi
            ├── murk-space ──┤                 └── murk-python
            ├── murk-propagator ─┤
            └── murk-obs ────────┘
                murk-replay ─────┘
```

**Safety boundary:** only `murk-arena` and `murk-ffi` are permitted
`unsafe` code. Every other crate uses `#![forbid(unsafe_code)]`.

---

## Three-Interface Model

All interaction with a Murk world flows through three interfaces:

```text
[Producers]                           [Consumers]
    |                                      ^
    v                                      |
 Ingress ──(bounded queue)──> TickEngine ──(publish)──> Egress
                                  \                      |
                                   └──(ring buffer)──────┘
```

- **Ingress** accepts commands (intents to change world state). It
  implements backpressure via a bounded queue, TTL-based expiry, and
  deterministic drop policies.
- **TickEngine** is the sole authoritative mutator. It drains the
  ingress queue, executes the propagator pipeline, and publishes an
  immutable snapshot at each tick boundary.
- **Egress** reads published snapshots to produce observations. It
  never mutates world state. In RealtimeAsync mode, egress workers run
  on a thread pool for concurrent observation extraction.

This separation enforces the key invariant: only TickEngine holds
`&mut WorldState`. Everything else operates on immutable snapshots.

---

## Arena-Based Generational Allocation

This is Murk's most load-bearing design decision. It replaces
traditional copy-on-write with a generational arena scheme:

1. Each field is stored as a contiguous `[f32]` allocation in a
   generational arena.
2. At tick start, propagators write to **fresh allocations** in the new
   generation — no copies required.
3. Unmodified fields share their allocation across generations
   (zero-cost structural sharing).
4. Snapshot publication swaps a ~1KB descriptor of field handles.
   Cost: <2us.
5. Old generations remain readable until all snapshot references are
   released.

| Property | Traditional CoW | Arena-Generational |
|----------|----------------|-------------------|
| Copy cost | Fault-driven, unpredictable | Zero (allocate fresh) |
| Snapshot publish | Clone or fork | Descriptor swap, <2us |
| Rollback | Undo log or checkpoint | Free (abandon generation) |
| Memory predictability | Fault-driven | Bump allocation |

### Rust type-level enforcement

- `ReadArena` (published snapshots): `Send + Sync`, safe for concurrent
  reads.
- `WriteArena` (staging, exclusive to TickEngine): `&mut` access, no
  aliasing possible.
- Snapshot descriptors contain `FieldHandle` values (generation-scoped
  integers), not raw pointers. `ReadArena::resolve(handle)` provides
  `&[f32]` access.
- Field access requires `&FieldArena` — the borrow checker enforces
  arena liveness.

### Lockstep arena recycling

In Lockstep mode, two arena buffers alternate roles each tick
(ping-pong). The caller's `&mut self` borrow on `step_sync()` guarantees
no outstanding snapshot borrows. Memory usage is bounded at 2x the
per-generation field footprint regardless of episode length.

### RealtimeAsync reclamation

In RealtimeAsync mode, epoch-based reclamation manages arena lifetimes.
Each egress worker pins an epoch while reading a snapshot. The
TickEngine reclaims old generations only when no worker holds a
reference. Stalled workers are detected and torn down to prevent
unbounded memory growth.

---

## Runtime Modes

Murk provides two runtime modes from the same codebase. There is no
runtime mode-switching — you choose at construction time.

### LockstepWorld

A callable struct with `&mut self` methods. The caller's thread executes
the full pipeline: command processing, propagators, snapshot publication,
and observation extraction.

```rust
let mut world = LockstepWorld::new(config)?;
let result = world.step_sync(commands)?;
let heat = result.snapshot.read(FieldId(0)).unwrap();
```

- Synchronous, deterministic, throughput-maximised.
- The borrow checker enforces that snapshots are released before the
  next step.
- No background threads, no synchronisation overhead.
- Primary use case: RL training loops, deterministic replay.

### RealtimeAsyncWorld

An autonomous tick thread running at a configurable rate (e.g., 60 Hz).

```rust
let world = RealtimeAsyncWorld::start(config)?;
world.submit_commands(commands)?;
let snapshot = world.latest_snapshot();
let report = world.shutdown(Duration::from_secs(5))?;
```

- Non-blocking command submission and observation extraction.
- Egress thread pool for concurrent ObsPlan execution.
- Epoch-based memory reclamation.
- Primary use case: live games, interactive tools, dashboards.

### BatchedEngine

`BatchedEngine` owns a `Vec<LockstepWorld>` and an optional `ObsPlan`.
Its hot path, `step_and_observe()`, steps all worlds sequentially then
calls `ObsPlan::execute_batch()` to fill a contiguous output buffer
across all worlds.

**Error model:** `BatchError` annotates failures with the world index:
- `Step { world_index, error }` — a world's `step_sync()` failed
- `Observe(ObsError)` — observation extraction failed
- `Config(ConfigError)` — world creation or reset failed
- `InvalidIndex { world_index, num_worlds }` — index out of bounds
- `NoObsPlan` — observation requested without `ObsSpec`
- `InvalidArgument { reason }` — argument validation failed

**FFI layer:** `BATCHED: Mutex<HandleTable<BatchedEngine>>` stores
engine instances. Nine `extern "C"` functions expose create, step,
observe, reset, destroy, and dimension queries.

**PyO3 layer:** `BatchedWorld` caches dimensions at construction time,
validates buffer shapes eagerly, and releases the GIL via `py.detach()`
on all hot paths. The `Ungil` boundary requires casting raw pointers to
`usize` before entering the detached closure.

---

## Threading Model

### Lockstep

No dedicated threads. The caller's thread runs the full tick pipeline.
Thread count equals the number of vectorised environments (typically
16-128 for RL training).

### RealtimeAsync

| Thread(s) | Role | Owns |
|-----------|------|------|
| TickEngine (1) | Tick loop: drain ingress, run propagators, publish | `&mut WorldState`, `WriteArena` |
| Egress pool (N) | Execute ObsPlans against snapshots | `&ReadArena` (shared) |
| Ingress acceptor (0-M) | Accept commands, assign `arrival_seq` | Write end of bounded queue |

Snapshot lifetime is managed by epoch-based reclamation, not reference
counting. This avoids cache-line ping-pong from atomic refcount
updates under high observation throughput.

---

## Spatial Model

Spaces define **how many cells** exist and **which cells are neighbours**.
All spaces implement the `Space` trait, which provides:

- `cell_count()` — total cells
- `neighbours(cell)` — ordered neighbour list
- `distance(a, b)` — scalar distance metric
- Region planning for observation extraction

### Built-in backends

| Space | Dims | Neighbours | Edge handling |
|-------|------|------------|---------------|
| `Line1D` | 1D | 2 | Absorb, Wrap |
| `Ring1D` | 1D | 2 (periodic) | Always wraps |
| `Square4` | 2D | 4 (N/S/E/W) | Absorb, Wrap |
| `Square8` | 2D | 8 (+ diagonals) | Absorb, Wrap |
| `Hex2D` | 2D | 6 | Absorb, Wrap |
| `FCC12` | 3D | 12 (face-centred cubic) | Absorb, Wrap |

### ProductSpace

Spaces can be composed via `ProductSpace` to create higher-dimensional
topologies. For example, `Hex2D x Line1D` creates a layered hex map
where each layer is a hex grid and vertical neighbours are connected
via the Line1D component.

```rust
let space = ProductSpace::new(vec![
    Box::new(Hex2D::new(8, EdgeBehavior::Wrap)?),
    Box::new(Line1D::new(3, EdgeBehavior::Absorb)?),
]);
```

Coordinates are concatenated across components. Neighbours vary one
component at a time (no diagonal cross-component adjacency).

---

## Field Model

Fields are per-cell data stored in arenas. Each field has:

- **Type**: `Scalar` (1 float), `Vector(n)` (n floats), or
  `Categorical(n)` (n classes).
- **Mutability class**: controls arena allocation strategy.
- **Boundary behaviour**: `Clamp`, `Reflect`, `Absorb`, or `Wrap`.
- Optional units and bounds metadata.

### Mutability classes

| Class | Arena behaviour | Use case |
|-------|----------------|----------|
| `Static` | Allocated once in generation 0, shared across all snapshots | Terrain, obstacles |
| `PerTick` | Fresh allocation each tick | Temperature, velocity |
| `Sparse` | New allocation only when modified | Rare events, flags |

For vectorised RL (128 envs x 2MB mutable + 8MB shared static):
**264MB** total vs 1.28GB without Static field sharing.

---

## Propagator Pipeline

Propagators are stateless operators that update fields each tick.
They implement the `Propagator` trait:

```rust
pub trait Propagator: Send + Sync {
    fn name(&self) -> &str;
    fn reads(&self) -> FieldSet;          // current-tick values (Euler)
    fn reads_previous(&self) -> FieldSet; // frozen tick-start values (Jacobi)
    fn writes(&self) -> Vec<(FieldId, WriteMode)>;
    fn max_dt(&self) -> Option<f64>;      // CFL constraint
    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError>;
}
```

Key properties:

- **`&self` signature** — propagators are stateless. All mutable state
  flows through `StepContext`.
- **Split-borrow reads** — `reads()` sees current in-tick values
  (Euler style), `reads_previous()` sees frozen tick-start values
  (Jacobi style). This supports both integration approaches.
- **Write-conflict detection** — the pipeline validates at startup that
  no two propagators write the same field in conflicting modes.
- **CFL validation** — if a propagator declares `max_dt()`, the engine
  checks `dt <= max_dt` at configuration time.
- **Deterministic execution order** — propagators run in the order they
  are registered. The pipeline is a strict ordered list.

---

## Observation Pipeline

The observation pipeline transforms world state into fixed-shape
tensors for RL frameworks:

```text
ObsSpec ──(compile)──> ObsPlan ──(execute against snapshot)──> f32 tensor
```

1. **ObsSpec** declares what to observe: which fields, which spatial
   region, what transforms (normalisation, pooling, foveation).
2. **ObsPlan** is a compiled, bound, executable plan. It pre-resolves
   field offsets, region iterators, index mappings, and pooling kernels.
   Compilation is done once; execution is the hot path.
3. **Execution** fills a caller-allocated buffer with `f32` values and
   a validity mask for non-rectangular domains (e.g., hex grids).

ObsPlans are bound to a world configuration generation. If the world
configuration changes (fields added, space resized), plans are
invalidated and must be recompiled.

---

## Command Model

Commands are the way external actions enter the simulation. Each
command carries:

- **Payload**: `SetField`, `SpawnEntity`, `RemoveEntity`, or custom.
- **TTL**: `expires_after_tick` — tick-based expiry (never wall clock).
- **Priority class**: determines application order within a tick.
- **Ordering provenance**: `source_id`, `source_seq`, and
  engine-assigned `arrival_seq` for deterministic ordering.

The TickEngine drains and applies commands in deterministic order:
1. Resolve `apply_tick_id` for each command.
2. Group by tick.
3. Sort within tick by priority class, then source ordering.

Every command produces a `Receipt` reporting whether it was accepted,
which tick it was applied at, and a reason code if rejected.

---

## Error Handling and Recovery

### Tick atomicity

Tick execution is all-or-nothing. If any propagator fails, all staging
writes are abandoned (free with the arena model — just drop the staging
generation). The world state remains exactly as it was before the tick.

### Recovery behaviour

- **Lockstep**: `step_sync()` returns `Err(StepError)`. The caller
  decides how to recover (typically `reset()`).
- **RealtimeAsync**: after 3 consecutive rollbacks, the TickEngine
  disables ticking and rejects further commands. Egress continues
  serving the last good snapshot (Egress Always Returns). Recovery
  via `reset()`.

See [error-reference.md](error-reference.md) for the complete error
type catalogue.

---

## Determinism

Murk targets **Tier B determinism**: identical results within the same
build, ISA, and toolchain, given the same initial state, seed, and
command log.

Key mechanisms:

- **No `HashMap`/`HashSet`** — banned project-wide via clippy. All code
  uses `IndexMap`/`BTreeMap` for deterministic iteration.
- **No fast-math** — floating-point reassociation is prohibited in
  authoritative code paths.
- **Tick-based time** — all state-affecting time references use tick
  counts, not wall clocks.
- **Deterministic command ordering** — commands are sorted by priority
  class and source ordering, not arrival time.
- **Replay support** — binary replay format records initial state, seed,
  and command log with per-tick snapshot hashes for divergence detection.

See [determinism-catalogue.md](determinism-catalogue.md) for the full
catalogue of non-determinism sources and mitigations.

---

## Language Bindings

### C FFI (`murk-ffi`)

Stable, handle-based C ABI:

- Opaque handles (`MurkWorld`, `MurkSnapshot`, `MurkObsPlan`) with
  slot+generation for safe double-destroy.
- Caller-allocated buffers for tensor output (no allocation on the
  hot path).
- Versioned API with explicit error codes.

### Python (`murk-python`)

PyO3/maturin native extension:

- `MurkEnv` — single-environment Gymnasium `Env` adapter.
- `MurkVecEnv` — vectorised environment adapter for parallel RL
  training.
- `BatchedWorld` — batched PyO3 wrapper: steps N worlds and extracts
  observations in a single `py.detach()` call. Pointer addresses are
  cast to `usize` for the `Ungil` closure boundary.
- `BatchedVecEnv` — pure-Python SB3-compatible vectorized environment
  with pre-allocated NumPy buffers, auto-reset, and override hooks for
  reward/termination logic.
- Direct NumPy array filling via the C FFI path.
- Python-defined propagators for prototyping.
