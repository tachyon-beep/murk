# Heat Seeker: Building an RL environment with Murk

This tutorial walks through building a complete reinforcement learning
environment on top of Murk, from spatial topology selection to a trained
PPO agent. Each section explains the decision being made, what alternatives
exist, and why we chose what we did.

**What we're building:** a 16x16 grid world where a heat source in one
corner diffuses warmth across the grid. A PPO agent starts at a random
position and learns to navigate toward the heat by reading the temperature
field.

**What this demonstrates:**
- Murk's spatial model (Square4 grid)
- Field definitions and mutability classes
- Python-side propagators (diffusion physics)
- Observation extraction via ObsPlan
- Command submission (agent movement)
- Gymnasium integration for RL training

## Prerequisites

```bash
# 1. Build the Murk Python extension
cd crates/murk-python
pip install maturin
maturin develop --release

# 2. Install RL dependencies
pip install stable-baselines3 gymnasium numpy
```

## Step 1: Choose a spatial topology

Murk supports six spatial backends. The choice determines how cells
are connected, how distances are measured, and what "neighbors" means.

| Backend | Connectivity | Use case |
|---------|-------------|----------|
| `Line1D` | 2-connected | 1D corridor, queue simulation |
| `Ring1D` | 2-connected (wrapping) | Circular track |
| `Square4` | 4-connected (N/S/E/W) | Grid worlds, cellular automata |
| `Square8` | 8-connected (+ diagonals) | Grid worlds where diagonal movement matters |
| `Hex2D` | 6-connected | Hex-based strategy games |
| `ProductSpace` | Cartesian product | Multi-layer worlds (e.g. Hex2D x Line1D) |

**Our choice: `Square4`** — it's the simplest 2D grid and maps naturally to
4-directional movement (up/down/left/right). Square8 would add diagonal
movement, which is fine but makes the action space larger without adding
pedagogical value. Hex2D would be interesting but harder to visualize.

```python
config.set_space(SpaceType.Square4, [16.0, 16.0, 0.0])
#                                    width  height edge_behavior
#                                                  0=Absorb (walls)
```

The edge behavior parameter controls what happens at the grid boundary:
- **Absorb (0):** edges are walls — no neighbor beyond the edge
- **Clamp (1):** beyond-edge references map back to the edge cell
- **Wrap (2):** periodic boundaries (left edge connects to right)

We use Absorb because the agent should learn to avoid corners, not wrap
around to the other side.

## Step 2: Define fields

Fields are per-cell data layers that propagators read and write each tick.
Every field has a **type** and a **mutability class**.

### Field types

| Type | Storage | Example |
|------|---------|---------|
| `Scalar` | 1 float per cell | Temperature, presence |
| `Vector{dims}` | N floats per cell | Velocity (2D), RGB color |
| `Categorical{n}` | N floats per cell (one-hot) | Terrain type |

### Mutability classes

This is the most important design decision for performance. It controls
how the arena allocates memory for the field each tick:

| Class | Behavior | When to use |
|-------|----------|-------------|
| `Static` | Allocated once, never changes | Terrain, walls, spawn points |
| `PerTick` | Fresh zero-filled allocation each tick | Fields recomputed every tick |
| `Sparse` | Copy-on-write (shared until mutated) | Fields that rarely change |

**Our fields:**

```python
config.add_field("heat", FieldType.Scalar, FieldMutability.PerTick)
config.add_field("agent_pos", FieldType.Scalar, FieldMutability.PerTick)
```

Both are `PerTick` because:
- **heat** is fully recomputed by the diffusion propagator every tick
- **agent_pos** is a binary mask (1.0 at agent's cell, 0.0 everywhere else)
  that we stamp fresh each tick via a command

If we had a terrain field that never changes, we'd use `Static`. If we
had a field that only changes in a few cells per tick (like damage markers),
we'd use `Sparse` to avoid copying the entire field.

**Note:** Field IDs are assigned in `add_field` order. The first field
added is ID 0, the second is ID 1, and so on.

## Step 3: Write a propagator

A propagator is a stateless function that runs once per tick. It declares
which fields it reads and writes, and the engine:
- validates there are no write-write conflicts between propagators
- provides the correct read views (current tick or previous tick)
- handles rollback if the propagator fails

### Euler vs Jacobi reads

This is a subtle but important choice:

- **`reads` (Euler):** sees writes from earlier propagators *in the same tick*.
  Good for sequential dependencies (movement then collision).
- **`reads_previous` (Jacobi):** sees the frozen state from the *previous tick*.
  Good for physics where all cells should update simultaneously (diffusion).

For diffusion, Jacobi is correct. If we used Euler reads, cells processed
first would see old heat values while cells processed later would see
already-updated values, introducing directional bias.

### Rust vs Python propagators

Murk supports both:
- **Rust propagators** run natively with zero overhead. Used for production.
- **Python propagators** use a trampoline: the engine copies field buffers
  to numpy arrays, calls your Python function, then copies results back.
  Slower, but lets you iterate without recompiling.

For this demo we use a Python propagator. In production, you'd move the
diffusion logic to a Rust `impl Propagator` for ~100x speedup.

### The diffusion propagator

```python
def diffusion_step(reads, reads_prev, writes, tick_id, dt, cell_count):
    prev = reads_prev[0].reshape(GRID_H, GRID_W)  # previous tick's heat
    out = writes[0]                                 # output buffer (zeros)

    # Discrete Laplacian with absorb boundaries (edge-padded).
    padded = np.pad(prev, 1, mode="edge")
    laplacian = (
        padded[:-2, 1:-1] + padded[2:, 1:-1]       # north + south
        + padded[1:-1, :-2] + padded[1:-1, 2:]      # west + east
        - 4.0 * prev                                # center
    )

    new_heat = prev + D * dt * laplacian - HEAT_DECAY * dt * prev
    new_heat[SOURCE_Y, SOURCE_X] = SOURCE_INTENSITY  # inject at source
    out[:] = new_heat.ravel()
```

**Why `np.pad(prev, 1, mode="edge")`?** This implements absorb boundaries.
Edge-padding copies the boundary cell's value into the ghost cell, so the
Laplacian contribution from the missing neighbor is zero (no flux across
the boundary). This matches the Square4 Absorb edge behavior.

**Why the decay term (`- HEAT_DECAY * dt * prev`)?** Without decay,
diffusion with a constant source on a bounded domain converges to a
uniform steady state — all cells approach `SOURCE_INTENSITY`, destroying
the gradient. The decay term creates an exponential spatial gradient
(`~exp(-d * sqrt(HEAT_DECAY / D))`) that persists at steady state,
giving the agent a directional signal to follow.

Register it with the engine:

```python
murk.add_propagator(
    config,
    name="diffusion",
    step_fn=diffusion_step,
    reads_previous=[HEAT_FIELD],  # Jacobi read
    writes=[(HEAT_FIELD, 0)],     # 0 = Full write mode
)
```

Write modes:
- **Full (0):** propagator writes every cell. Buffer starts at zero.
- **Incremental (1):** propagator adds to existing values. Used when
  multiple propagators contribute to the same field.

## Step 4: Wire observations

Murk's observation system separates *specification* from *execution*:

1. **ObsSpec** describes what to observe (which fields, which region, transforms)
2. **ObsPlan** is the compiled, executable version (precomputed indices)
3. **execute()** fills a flat `float32` buffer — ready for a neural network

### Region types

| Type | Output size | Use case |
|------|------------|----------|
| All (0) | cell_count per field | Full-grid observation |
| AgentDisk (5) | ~pi*r^2 per field | Local circular patch around agent |
| AgentRect (6) | (2h+1)^ndim per field | Local rectangular patch around agent |

**Our choice: All (full grid).** For a 16x16 grid, this gives 256 floats
per field. With two fields (heat + agent_pos), the observation is 512
floats — trivial for PPO's MLP policy.

For larger grids (100x100+), you'd switch to AgentDisk or AgentRect to
keep the observation size manageable and give the agent translation-invariant
local perception.

```python
obs_entries = [
    ObsEntry(HEAT_FIELD),   # 256 floats: the temperature gradient
    ObsEntry(AGENT_FIELD),  # 256 floats: 1.0 at agent position
]
```

The entries are concatenated in order: `obs[:256]` is heat, `obs[256:]`
is agent position.

### Transforms

ObsEntry supports transforms applied at extraction time:
- **Identity (0, default):** raw values
- **Normalize (1):** scale to [min, max] range

We use Identity since our heat values are already in a reasonable range
for neural network input.

## Step 5: Build the Gymnasium environment

`MurkEnv` is a base class that handles the world lifecycle and Gymnasium
protocol. You subclass it and override four hooks:

```python
class HeatSeekerEnv(murk.MurkEnv):

    def _action_to_commands(self, action):
        """Convert discrete action → list of Murk commands."""
        # Move agent, stamp new position into the field.
        ...
        return [Command.set_field(AGENT_FIELD, [x, y], 1.0)]

    def _compute_reward(self, obs, info):
        """Reward = heat value at agent's position."""
        return float(obs[agent_flat_index])

    def _check_terminated(self, obs, info):
        """Episode ends when agent reaches the heat source."""
        return self._agent_x == SOURCE_X and self._agent_y == SOURCE_Y

    def _check_truncated(self, obs, info):
        """Episode truncated after MAX_STEPS ticks."""
        return info.get("tick_id", 0) >= MAX_STEPS
```

### Commands: how the agent acts

Murk's ingress system accepts **commands** — declarative mutation intents
applied before propagators run each tick. For this demo, the agent's
action becomes a `SetField` command that stamps a 1.0 at its new position.

Since `agent_pos` is PerTick, it starts at zero every tick. We only need
to set the one cell where the agent is — no need to clear the old position.

### Reset and warmup

On reset, the world returns to tick 0 with all fields zeroed. The heat
field needs time to build up a gradient, so we run 50 "warmup" ticks
with no commands before placing the agent. This lets the diffusion
propagator establish a near-steady-state temperature distribution.

```python
def reset(self, *, seed=None, options=None):
    self._world.reset(self._seed)

    # Let heat diffuse to steady state.
    for _ in range(WARMUP_TICKS):
        self._world.step(None)

    # Place agent, stamp position, extract initial observation.
    ...
```

**Alternative:** use a `Static` field for the equilibrium heat distribution,
pre-computed once. This avoids warmup ticks but requires computing the
steady-state solution offline.

## Step 6: Train with PPO

stable-baselines3's PPO works out of the box with any Gymnasium env:

```python
from stable_baselines3 import PPO

env = DummyVecEnv([lambda: HeatSeekerEnv(seed=42)])
model = PPO("MlpPolicy", env, n_steps=2048, ent_coef=0.15, verbose=0,
            policy_kwargs=dict(net_arch=[128, 128]))
model.learn(total_timesteps=300_000, progress_bar=True)
```

**Why these hyperparameters?**
- `MlpPolicy`: two-layer MLP (128x128). Handles the 512-dim observation.
- `n_steps=2048`: longer rollouts (~13 episodes) give better value estimates
  for the sparse terminal reward.
- `ent_coef=0.15`: high entropy coefficient prevents premature policy
  collapse. Without sufficient entropy, PPO converges to a single action
  before discovering the terminal bonus.
- `300_000 timesteps`: enough to converge on this problem (~2000 episodes).

## Step 7: Evaluate

We compare a random policy (untrained model) against the trained policy:

```
Evaluating random policy (before training)...
  Mean reward:    -146.1
  Mean length:     149.0 steps
  Reach rate:         0%

Training PPO for 300,000 timesteps...
  Done in 165.6s (1811 steps/sec)

Evaluating trained policy (after training)...
  Mean reward:      91.5
  Mean length:      11.9 steps
  Reach rate:       100%
```

The trained agent:
- Reliably reaches the heat source (100% reach rate)
- Does so in ~12 steps on average (near-optimal for the grid)
- Achieves reward ~91 vs -146 for random (terminal bonus dominates)

## What's next

This demo exercises the full Murk stack but leaves plenty of room to grow:

- **Vectorized training:** use `MurkVecEnv` with 8-16 parallel worlds
  for faster training
- **Rust propagators:** move diffusion to Rust for ~100x tick throughput
- **Local observations:** switch to `AgentDisk` for agent-centered
  perception that scales to larger grids
- **Multiple agents:** add more agents with shared or competitive rewards
- **Richer physics:** add velocity fields, obstacles, or resource
  depletion using additional propagators
- **RealtimeAsync mode:** run the simulation in a background thread
  for real-time visualization
