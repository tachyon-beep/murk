# Murk Concepts Guide

This guide explains the mental model behind Murk. It's written for someone
who has run the [heat_seeker](https://github.com/tachyon-beep/murk/tree/main/examples/heat_seeker) example and wants
to build something of their own.

Every Murk simulation has five components:

1. A **Space** — the topology cells live on
2. **Fields** — per-cell data stored in arenas
3. **Propagators** — stateless operators that update fields each tick
4. **Commands** — how actions from outside enter the simulation
5. **Observations** — how state gets extracted for agents or renderers

These components are configured once, compiled into a world, and then
ticked forward repeatedly. The rest of this guide explains each one.

---

## Spaces & Topologies

A space defines **how many cells** exist and **which cells are neighbors**.
Murk ships with seven built-in space backends:

| Space | Dims | Neighbors | Parameters | Distance metric |
|-------|------|-----------|------------|-----------------|
| `Line1D` | 1D | 2 | `length`, `edge` | Manhattan |
| `Ring1D` | 1D | 2 (periodic) | `length` | min(fwd, bwd) |
| `Square4` | 2D | 4 (N/S/E/W) | `width`, `height`, `edge` | Manhattan |
| `Square8` | 2D | 8 (+ diagonals) | `width`, `height`, `edge` | Chebyshev |
| `Hex2D` | 2D | 6 (pointy-top) | `cols`, `rows` | Cube distance |
| `Fcc12` | 3D | 12 (face-centred cubic) | `w`, `h`, `d`, `edge` | FCC metric |
| `ProductSpace` | N-D | varies | list of component spaces | L1 sum |

### Choosing a space

- **Line1D / Ring1D** — 1D cellular automata, queues, pipelines.
- **Square4** — grid worlds, pathfinding, Conway's Game of Life.
- **Square8** — grid worlds where diagonal movement matters.
- **Hex2D** — isotropic 2D movement without diagonal bias.
- **Fcc12** — 3D isotropic lattice (12 equidistant neighbors). Good for
  volumetric simulations like crystal growth or 3D diffusion.
- **ProductSpace** — compose any spaces together (e.g., `Hex2D x Line1D`
  for a hex map with a vertical elevation axis).

### Edge behaviors

Spaces that have boundaries support three edge behaviors:

| Behavior | At boundary | Example use |
|----------|------------|-------------|
| `Absorb` | Edge cells have fewer neighbors | Bounded arena, finite grid |
| `Clamp` | Beyond-edge maps to edge cell | Image processing, extrapolation |
| `Wrap` | Wraps to opposite side (torus) | Pac-Man map, periodic simulation |

`Ring1D` is always periodic (wrap). `Hex2D` only supports `Absorb`.

### Coordinates

Every cell has a coordinate — a small vector of `i32` values:

- `Line1D` / `Ring1D`: `[x]`
- `Square4` / `Square8`: `[row, col]`
- `Hex2D`: `[q, r]` (axial, pointy-top)
- `Fcc12`: `[x, y, z]` where `(x + y + z) % 2 == 0`
- `ProductSpace`: concatenation of component coordinates

Cells are stored in **canonical order** (a deterministic traversal of
all coordinates). When you read a field as a flat `f32` array, element
`i` corresponds to canonical coordinate `i`. For 2D grids this is
row-major order.

### Cell count

The number of cells is determined by the space parameters:

- `Line1D(5)` → 5 cells
- `Square4(10, 10)` → 100 cells
- `Hex2D(8, 8)` → 64 cells
- `Fcc12(4, 4, 4)` → approximately `w*h*d / 2` cells (parity constraint)

This matters because every field allocates `cell_count * components`
floats per generation.

---

## Fields & Mutability

Fields are per-cell data arrays. A 100-cell `Square4` world with one
`Scalar` field allocates 100 `f32` values for that field.

### Field types

| Type | Storage per cell | Use case |
|------|-----------------|----------|
| `Scalar` | 1 × f32 | Temperature, density, boolean flags |
| `Vector { dims }` | `dims` × f32 | Velocity, color |
| `Categorical { n_values }` | 1 × f32 (stored as index) | Terrain type, cell state |

### Field mutability

Mutability controls **how** and **when** memory is allocated for a field.
This is the most important performance decision you'll make.

| Mutability | Allocation pattern | Read baseline | Use when |
|------------|-------------------|---------------|----------|
| `Static` | Once, never again | Always generation 0 | Constants (terrain type, wall mask) |
| `PerTick` | Fresh buffer every tick | Previous tick's values | Frequently-updated state (heat, positions) |
| `Sparse` | New buffer only on write | Shared until mutated | Infrequently-changed state (terrain HP) |

**Static** fields are allocated once in a shared arena. They're
read-only after initialization — propagators can read them but never
write them. Use these for data that never changes (terrain layout,
obstacle masks).

**PerTick** fields get a fresh buffer every tick. If a propagator
writes to the field, it fills the new buffer. If nothing writes to
the field, the previous tick's values are copied forward. This is
the most common mutability class — use it for anything that changes
regularly.

**Sparse** fields share memory across ticks until something writes to
them, at which point a new buffer is allocated (copy-on-write). Use
these for data that changes rarely — the arena skips allocation on
ticks where the field isn't modified.

**Quick decision guide:**
1. Does this field ever change after initialization? No --> `Static`
2. Does it change every tick? Yes --> `PerTick`
3. Does it change rarely (< 10% of ticks)? Yes --> `Sparse`
4. Unsure? Default to `PerTick`

### Bounds and boundary behavior

Fields can optionally have value bounds `(min, max)`. When a value
is written outside those bounds, the `BoundaryBehavior` determines
what happens:

- `Clamp` — value is clamped to the nearest bound
- `Reflect` — value bounces off the bound
- `Absorb` — value is set to the bound
- `Wrap` — value wraps to the opposite bound

If you don't need bounds, just use the defaults.

---

## Propagators

A propagator is a **stateless function** that runs once per tick. It reads
some fields, writes some fields, and that's it. All simulation logic lives
in propagators.

### The step signature (Python)

```python
def my_propagator(reads, reads_prev, writes, tick_id, dt, cell_count):
    """
    reads:       list of numpy arrays (fields from current-tick overlay)
    reads_prev:  list of numpy arrays (fields from previous tick, frozen)
    writes:      list of numpy arrays (output buffers to fill)
    tick_id:     int, monotonically increasing tick counter
    dt:          float, simulation timestep in seconds
    cell_count:  int, number of cells in the space
    """
    ...
```

### The step signature (Rust)

```rust
fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
    let prev_heat = ctx.reads_previous().read_field(HEAT_ID)?;
    let space = ctx.space();
    let writer = ctx.writes();
    // ... compute new values, write to output ...
}
```

### Read modes: Euler vs Jacobi

Every propagator declares which fields it reads. There are two read
modes:

- **`reads`** (Euler mode) — sees the **in-tick overlay**. If a prior
  propagator in the same tick already wrote to this field, you see those
  new values. This creates a dependency chain between propagators.

- **`reads_previous`** (Jacobi mode) — sees the **frozen tick-start
  snapshot**. Always reads the base generation, regardless of what other
  propagators have written this tick.

The choice matters for correctness:

- **Diffusion** should use `reads_previous` (Jacobi). Otherwise the
  result depends on cell visit order, which is wrong.
- **A reward propagator** that reads an agent-position field written by
  a movement propagator should use `reads` (Euler) to see the
  already-updated position.

### Write modes

Each written field has a write mode:

- **`WriteMode.Full`** — the propagator fills every cell. The engine
  gives you a fresh, zeroed buffer. In debug builds, a coverage guard
  checks that every cell was written.

- **`WriteMode.Incremental`** — the propagator modifies only some cells.
  The engine pre-seeds the buffer with the previous tick's values
  via `memcpy`. You only update the cells you need.

### Pipeline validation

Murk validates the propagator pipeline at startup:

- **Write conflicts** — two propagators writing the same field is an
  error (detected and reported with both propagator names).
- **CFL stability** — if a propagator declares a `max_dt`, Murk checks
  that the configured `dt` doesn't exceed it.
- **Undefined fields** — reading a field that doesn't exist is an error.

### Ordering

Propagators run in the order they're registered. This ordering, combined
with the Euler/Jacobi read declarations, defines the dataflow. The
engine precomputes a `ReadResolutionPlan` that maps each
(propagator, field) pair to either the base generation or a prior
propagator's staged output — with zero per-tick routing overhead.

---

## Commands & Ingress

Commands are how actions from outside the simulation (agent actions,
user input, network messages) enter the tick loop.

### Command types

| Command | Purpose |
|---------|---------|
| `SetField(coord, field_id, value)` | Write a single cell value |
| `Move(entity_id, target_coord)` | Move an entity |
| `Spawn(coord, field_values)` | Create a new entity |
| `Despawn(entity_id)` | Remove an entity |
| `SetParameter(key, value)` | Change a global simulation parameter |
| `Custom(type_id, data)` | User-defined command type |

In the Python API, the most common command is `SetField`:

```python
cmd = Command.set_field(field_id=1, coord=[5, 3], value=1.0)
receipts, metrics = world.step([cmd])
```

### Receipts

Every command submitted to `step()` gets a receipt:

```python
receipts, metrics = world.step([cmd])
for r in receipts:
    print(r.accepted, r.applied_tick_id)
```

A command can be rejected if the ingress queue is full, the command
is stale (refers to an old tick), or the world is shutting down.

### Command ordering

Commands are applied in this order: `priority_class` (lower = higher
priority), then `source_id`, then `arrival_seq` (monotonic counter).
System commands (priority 0) run before user commands (priority 1).

---

## Observations

The observation system extracts field data into flat `f32` tensors
suitable for neural networks.

### The pipeline: ObsSpec → ObsPlan → execute

1. **ObsSpec** — a list of `ObsEntry` objects declaring what to observe.
2. **ObsPlan** — a compiled plan (precomputed gather indices). Created
   once, reused every tick.
3. **execute** — runs the plan against the current world snapshot,
   producing a flat `f32` array.

```python
# 1. Specify what to observe
obs_entries = [
    ObsEntry(field_id=0, region_type=RegionType.All),
    ObsEntry(field_id=1, region_type=RegionType.AgentDisk, radius=3),
]

# 2. MurkEnv compiles the plan internally
# 3. Each step(), the plan executes and returns obs as a numpy array
obs, reward, terminated, truncated, info = env.step(action)
```

### Region types

| Region | Description | When to use |
|--------|-------------|-------------|
| `All` | Every cell in the space | Full observability, small grids |
| `AgentDisk(radius)` | Cells within `radius` graph-distance of the agent | Partial observability, foveation |
| `AgentRect(half_extent)` | Axis-aligned bounding box around agent | Rectangular partial observability |

`All` is the simplest — you get `cell_count` floats per entry. Agent-centered
regions give partial observability and scale better on large grids.

### Transforms

Transforms are applied to field values during extraction:

- **`Identity`** — raw values, no change
- **`Normalize(min, max)`** — linearly maps `[min, max]` to `[0, 1]`,
  clamping values outside the range

### Pooling

For large observations, pooling reduces dimensionality:

- `PoolKernel.Mean` — average of each window
- `PoolKernel.Max` — maximum of each window
- `PoolKernel.Min` — minimum of each window
- `PoolKernel.Sum` — sum of each window

Pooling is configured per-entry with `kernel_size` and `stride`.

### Observation layout

Entries are concatenated in order. If you observe two fields on a
100-cell grid with `region_type=All`, you get a 200-element `f32`
array: the first 100 elements are field 0, the next 100 are field 1.

---

## Runtime Modes

Murk has two runtime modes that share the same tick engine but differ
in how you interact with it.

### LockstepWorld (synchronous)

The standard mode for RL training:

```python
# Python (via MurkEnv)
obs, reward, terminated, truncated, info = env.step(action)

# Rust
let result = world.step_sync(commands)?;
let snapshot = result.snapshot;  // borrows world
```

**Properties:**
- Blocking `step()` call — you wait for the tick to complete
- In Rust, `&mut self` enforces single-threaded access at compile time
- The snapshot borrows the world, preventing a new step until you're done reading
- Deterministic: same seed + same commands = same result, always

This is what `MurkEnv` and `MurkVecEnv` use internally.

### RealtimeAsyncWorld (asynchronous)

For real-time applications (game servers, live visualizations):

```rust
// Commands are submitted without blocking
world.submit_commands(commands)?;

// Observations can be taken concurrently
let result = world.observe(&mut plan)?;
```

**Properties:**
- Background tick thread runs at a configurable rate
- Multiple observation requests can be served concurrently via a worker pool
- Epoch-based reclamation ensures snapshots aren't freed while being read
- Command channel provides back-pressure when the queue is full

The Python bindings expose `LockstepWorld` via `MurkEnv`/`MurkVecEnv`, and `BatchedEngine` via `BatchedVecEnv` for high-throughput training.

### BatchedEngine (high-throughput training)

For RL training at scale, stepping worlds one-by-one through Python has
a bottleneck: each `step()` call acquires and releases the GIL. With
thousands of environments, this overhead dominates.

`BatchedEngine` solves this by owning N `LockstepWorld` instances and
stepping them all in a single Rust call. The GIL is released once,
covering the entire step + observe operation for all worlds:

```python
from murk import BatchedVecEnv, Config, ObsEntry, RegionType

def make_config(i: int) -> Config:
    cfg = Config()
    cfg.set_space_square4(rows=16, cols=16)
    cfg.add_field("temperature", initial_value=0.0)
    return cfg

obs_entries = [ObsEntry(field_id=0, region_type=RegionType.All)]
env = BatchedVecEnv(make_config, obs_entries, num_envs=64)

obs, info = env.reset(seed=42)
obs, rewards, terminateds, truncateds, info = env.step(actions)
```

**Architecture (three layers):**

| Layer | Class | Role |
|-------|-------|------|
| Rust engine | `BatchedEngine` | Owns N `LockstepWorld`s, `step_and_observe()` |
| PyO3 wrapper | `BatchedWorld` | Handles GIL release, buffer validation |
| Pure Python | `BatchedVecEnv` | SB3-compatible API, auto-reset, override hooks |

**Override hooks** let you customise the RL interface without touching Rust:

- `_actions_to_commands(actions)` — convert action array to per-world command lists
- `_compute_rewards(obs, tick_ids)` — compute per-world rewards
- `_check_terminated(obs, tick_ids)` — per-world termination conditions
- `_check_truncated(obs, tick_ids)` — per-world truncation conditions

Here is a minimal example of overriding `_compute_rewards` in a
subclass (from the
[batched_heat_seeker](https://github.com/tachyon-beep/murk/tree/main/examples/batched_heat_seeker)
example):

```python
class BatchedHeatSeekerEnv(BatchedVecEnv):
    def step(self, actions):
        # ... move agents, build commands, call step_and_observe ...
        obs = self._obs_flat.reshape(self.num_envs, self._obs_per_world)

        # Vectorized reward: index into (N, cell_count) heat matrix
        heat = obs[:, :CELL_COUNT]
        agent_indices = self._agent_y * GRID_W + self._agent_x
        heat_at_agent = heat[np.arange(self.num_envs), agent_indices]

        terminated = (self._agent_x == SOURCE_X) & (self._agent_y == SOURCE_Y)
        rewards = REWARD_SCALE * heat_at_agent - STEP_PENALTY
        rewards[terminated] += TERMINAL_BONUS

        # ... auto-reset, return obs/rewards/terminated/truncated/info ...
```

**vs MurkVecEnv:** `MurkVecEnv` wraps N independent `World` objects and
calls `step()` N times (N GIL releases). `BatchedVecEnv` calls
`step_and_observe()` once (1 GIL release). For 1024 environments, this
eliminates ~1023 unnecessary GIL cycles per training step.

---

## Arena & Memory

Murk uses **arena-based generational allocation** instead of per-object
heap allocation. This is what makes it fast and GC-free.

### The ping-pong buffer

The engine maintains two segment pools (A and B). On each tick:

1. One pool is **staging** (being written by propagators)
2. The other is **published** (readable as a snapshot)
3. After the tick, they swap roles

This means the previous tick's data is always available for reading
while the current tick is being computed.

### How mutability maps to memory

- **Static** fields live in a separate shared arena. They're allocated
  once and never touched again. No per-tick cost.

- **PerTick** fields get a fresh allocation in the staging pool every
  tick. After publish, the old staging pool (now published) still holds
  the previous tick's values — so snapshots and `reads_previous` work
  without copying.

- **Sparse** fields use a dedicated copy-on-write slab. They share
  memory across ticks until a propagator writes to them, at which point
  a new allocation is made. On ticks where nothing writes to a sparse
  field, there's zero allocation cost.

### Why this matters

- **No garbage collection pauses** — arena memory is bulk-freed, not
  per-object
- **Deterministic memory lifetime** — you know exactly when memory is
  allocated and freed
- **Zero-copy snapshots** — reading the previous tick's data is just
  a pointer into the published pool

For most users, you don't need to think about arenas directly. The
practical takeaway is: choose the right `FieldMutability` for your
data, and the arena system handles the rest efficiently.

---

## Putting It Together

Here's how these concepts compose in a typical simulation:

```python
import murk
from murk import (
    Config, FieldMutability, EdgeBehavior,
    WriteMode, ObsEntry, RegionType,
)

# 1. Space: defines topology
config = Config()
config.set_space_square4(32, 32, EdgeBehavior.Wrap)

# 2. Fields: define per-cell data
config.add_field("temperature", mutability=FieldMutability.PerTick)
config.add_field("terrain", mutability=FieldMutability.Static)
config.add_field("agent_pos", mutability=FieldMutability.PerTick)

# 3. Propagator: defines simulation logic
def diffuse(reads, reads_prev, writes, tick_id, dt, cell_count):
    # reads_prev[0] = previous tick's temperature
    # writes[0] = this tick's temperature output
    ...

murk.add_propagator(
    config,
    name="diffusion",
    step_fn=diffuse,
    reads_previous=[0],              # Jacobi read of field 0
    writes=[(0, WriteMode.Full)],    # Full write to field 0
)

config.set_dt(0.1)
config.set_seed(42)

# 4. Observations: define what the agent sees
obs_entries = [
    ObsEntry(0, region_type=RegionType.All),       # Full temperature grid
    ObsEntry(2, region_type=RegionType.AgentDisk, radius=5),  # Agent's local view
]

# 5. Environment: wraps everything in the Gymnasium interface
env = murk.MurkEnv(config, obs_entries, n_actions=5, seed=42)
obs, info = env.reset()
obs, reward, terminated, truncated, info = env.step(action)
```

For a complete working example, see [heat_seeker](https://github.com/tachyon-beep/murk/tree/main/examples/heat_seeker).

---

## Glossary

| Term | Definition |
|------|-----------|
| **Cell** | A single location in the space. Has a coordinate and one value per field. |
| **Tick** | One simulation timestep. All propagators run, then the arena publishes. |
| **Generation** | Arena version counter. Incremented on each publish. |
| **Canonical order** | The deterministic traversal of all coordinates (row-major for 2D grids). |
| **Snapshot** | Read-only view of the world state at a particular generation. |
| **ObsPlan** | Compiled observation plan. Precomputes gather indices for fast extraction. |
| **Ingress** | The command queue that feeds actions into the tick loop. |
| **Egress** | The observation pathway that extracts state out of the simulation. |
| **CFL condition** | Courant-Friedrichs-Lewy stability constraint: `N * D * dt < 1`, where **N** is the neighbor count of the space (e.g., 4 for `Square4`), **D** is the diffusion coefficient, and **dt** is the simulation timestep. When this condition is violated, explicit diffusion becomes numerically unstable -- values oscillate or diverge instead of converging. Propagators can declare `max_dt()` so the engine rejects configurations that violate their CFL bound at startup. |
