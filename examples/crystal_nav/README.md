# Crystal Navigator: 3D navigation on an FCC lattice

This tutorial builds a reinforcement learning environment on Murk's 3D
FCC12 lattice — a 12-connected face-centered cubic topology used in
crystallography and materials science. It extends the Heat Seeker example
with 3D navigation, graph Laplacian diffusion, and dual competing reward
signals.

**What we're building:** an 8x8x8 FCC12 lattice with two diffusing fields —
a beacon scent (attractive) and a radiation hazard (repulsive). A PPO agent
starts at a random cell and learns to navigate toward the beacon while
avoiding the radiation source.

**What this demonstrates:**
- Murk's 3D FCC12 spatial topology (12-connected, isotropic)
- Graph Laplacian diffusion (topology-agnostic, works on any lattice)
- Exponential decay for spatial gradient formation
- Dual competing reward signals
- 13-action discrete control (stay + 12 FCC offsets)
- Gymnasium integration for 3D RL training

## Prerequisites

```bash
# 1. Build the Murk Python extension
cd crates/murk-python
pip install maturin
maturin develop --release

# 2. Install RL dependencies
pip install stable-baselines3 gymnasium numpy
```

## Step 1: Why FCC12

Murk supports seven spatial backends. The FCC12 lattice is unique among
them: it's the only 3D topology, and its 12 equidistant neighbors make
it **isotropic** — there's no preferred axis, so diffusion spreads
equally in all directions.

| Backend | Dims | Connectivity | Isotropy |
|---------|------|-------------|----------|
| `Line1D` | 1D | 2-connected | N/A |
| `Ring1D` | 1D | 2-connected (wrap) | N/A |
| `Square4` | 2D | 4-connected | No (axis-aligned bias) |
| `Square8` | 2D | 8-connected | No (diagonal is sqrt(2)) |
| `Hex2D` | 2D | 6-connected | Yes (2D) |
| `Fcc12` | 3D | 12-connected | Yes (3D) |
| `ProductSpace` | nD | Cartesian product | Depends on components |

**Our choice: `Fcc12`** — it provides isotropic 3D diffusion, a non-trivial
navigation challenge (13 actions, 3 axes), and showcases Murk's
topology-agnostic architecture. The same propagator code would work on
any lattice.

```python
config.set_space(SpaceType.Fcc12, [8.0, 8.0, 8.0, 0.0])
#                                  w    h    d    edge_behavior
#                                                 0=Absorb
```

An 8x8x8 FCC lattice contains exactly 256 valid cells (half the
8x8x8=512 grid, due to the parity constraint).

## Step 2: FCC coordinate math

### The parity constraint

FCC lattices only contain cells where `(x + y + z) % 2 == 0`. This is
the "checkerboard in 3D" pattern. Each valid cell has exactly 12
neighbors at the same unit distance — all permutations of `(+-1, +-1, 0)`:

```python
FCC_OFFSETS = [
    (1, 1, 0),  (-1, 1, 0),  (1, -1, 0),  (-1, -1, 0),
    (1, 0, 1),  (-1, 0, 1),  (1, 0, -1),  (-1, 0, -1),
    (0, 1, 1),  (0, -1, 1),  (0, 1, -1),  (0, -1, -1),
]
```

Each offset changes exactly 2 of the 3 axes. This means the agent can't
move along a single axis — it must combine offsets to navigate all three
dimensions. This makes 3D pathfinding genuinely harder than 2D.

### Canonical ordering

Cells are enumerated in z-then-y-then-x order (innermost loop on x),
skipping odd-parity positions. This matches the Rust engine's
`canonical_ordering()` in `crates/murk-space/src/fcc12.rs`, ensuring
Python rank indices align with the engine's field buffer layout.

```python
cells = []
for z in range(d):
    for y in range(h):
        x_start = (y + z) % 2
        for x in range(x_start, w, 2):
            cells.append((x, y, z))
```

## Step 3: Graph Laplacian diffusion

### Why np.pad doesn't work

The Heat Seeker example uses `np.pad` to compute the discrete Laplacian
on a rectangular grid — pad the 2D array by 1 on each side, then sum
the 4 neighbors. This only works because Square4 has a regular 2D
structure.

FCC12's connectivity graph is irregular: interior cells have 12 neighbors,
edge cells have fewer (down to 3 at corners). There's no natural way to
reshape the field into a 3D array where adjacent elements are FCC neighbors.

### The sentinel trick

Instead, we precompute an adjacency index array at module load time:

```python
# NBR_IDX: int32 array (256, 12) — each row lists neighbor ranks.
# Missing neighbors (at boundaries) use sentinel = 256.
# DEGREE: int32 array (256,) — actual neighbor count per cell.

padded = np.zeros(CELL_COUNT + 1, dtype=np.float32)
padded[:CELL_COUNT] = prev_field

# Fancy indexing: sentinel slots (index 256) read 0.0 from the padding.
nbr_sum = padded[NBR_IDX].sum(axis=1)     # shape (256,)
laplacian = nbr_sum - DEGREE * prev_field  # graph Laplacian
```

The sentinel trick avoids Python loops over cells: missing neighbors
point to the extra element at index `CELL_COUNT` (always 0.0), so
`padded[NBR_IDX]` returns the correct neighbor values even for boundary
cells. The `DEGREE` array corrects the center term of the Laplacian.

This pattern works on **any** Murk topology — Square4, Hex2D, ProductSpace —
as long as you precompute the adjacency array.

### CFL stability

The Courant-Friedrichs-Lewy condition for explicit diffusion on a graph
with maximum degree `d_max` is:

```
d_max * D * dt < 1
```

For FCC12 with dt=1.0: `12 * D < 1`, so `D < 0.083`. Our beacon
diffusion coefficient D=0.06 gives CFL=0.72, and radiation D=0.04
gives CFL=0.48 — both stable.

## Step 4: Exponential decay and spatial gradients

### The saturation problem

Plain diffusion with a constant source and bounded domain converges to
a **uniform** steady state — all cells approach the source value. After
enough warmup ticks, the gradient disappears entirely.

```
Without decay:  beacon=10.0 everywhere  →  gradient = 0  →  PPO learns nothing
```

### The fix: decay term

Adding exponential decay to the diffusion equation creates a spatial
gradient that persists at steady state:

```
du/dt = D * L(u) - lambda * u + S(x)
```

where `lambda` is the decay rate and `S(x)` is the point source. The
steady-state solution decays exponentially with distance from the source:

```
u(d) ~ S * exp(-d * sqrt(lambda / D))
```

We use different decay rates for the two fields:
- **Beacon** (D=0.06, lambda=0.01): spreads far, detectable ~6 hops away
- **Radiation** (D=0.04, lambda=0.03): concentrated, sharp local hazard

```python
new_beacon = prev_beacon + D * dt * laplacian - BEACON_DECAY * dt * prev_beacon
new_beacon[beacon_rank] = SOURCE_INTENSITY  # pin source at 10.0
```

### Resulting gradient

```
Cell            Beacon  Radiation  Gradient   Reward/step
(6,6,6) beacon  10.000    0.073    +9.927      +0.493
(5,6,7) nearby   4.302    0.077    +4.225      -0.077
(4,4,4) mid      1.123    0.356    +0.767      -0.423
(2,2,2) hazard   0.378   10.000    -9.622      -1.462
(0,0,0) corner   0.209    1.128    -0.919      -0.592
```

The beacon cell is the **only** position with positive per-step reward.
The radiation zone is strongly penalized. This creates a clear learning
signal for PPO.

## Step 5: 13-action space

The agent has 13 discrete actions: stay (0) plus the 12 FCC offsets (1-12).

```python
if action != 0:
    dx, dy, dz = FCC_OFFSETS[action - 1]
    nx, ny, nz = x + dx, y + dy, z + dz
    if (nx, ny, nz) in COORD_TO_RANK:
        x, y, z = nx, ny, nz  # valid move
    # else: absorb (stay put)
```

At the boundary, invalid moves are absorbed — the agent stays in place.
Interior cells have all 12 moves valid; corner cells may have as few as
3 valid moves.

## Step 6: Environment and reward

### Reward shaping

```python
reward = GRADIENT_SCALE * (beacon[agent] - radiation[agent]) - STEP_PENALTY
if at_beacon:
    reward += TERMINAL_BONUS
```

- `GRADIENT_SCALE = 0.1`: scales the gradient to a shaping signal
- `STEP_PENALTY = 0.5`: discourages camping at sub-optimal positions
- `TERMINAL_BONUS = 100.0`: large reward for reaching the beacon

The competing gradients make this harder than a single-field problem:
the agent must learn that high beacon value is good but high radiation
is bad, and the shortest path might not be the safest path.

### Reset and warmup

On reset, the world returns to tick 0 with all fields zeroed. We run 80
warmup ticks (more than Heat Seeker's 50) to let the 3D diffusion fields
reach near-steady-state through the 12-connected mesh. The agent starts
at a random valid FCC cell, excluding the beacon.

## Step 7: Training and results

```
============================================================
  Crystal Navigator: Murk + PPO on 3D FCC12 Lattice
============================================================

  Lattice:     8x8x8 FCC12 (256 cells)
  Beacon:      (6,6,6), D=0.06
  Radiation:   (2,2,2), D=0.04
  Actions:     13 (stay + 12 FCC offsets)
  Obs size:    768 (beacon + radiation + agent_pos)
  Warmup:      80 ticks
  Training:    1,000,000 timesteps

Evaluating random policy (before training)...
  Mean reward:     -63.1
  Mean length:     197.2 steps
  Reach rate:        10%

Training PPO for 1,000,000 timesteps...
  Done in 746.6s (1339 steps/sec)

Evaluating trained policy (after training)...
  Mean reward:      99.7
  Mean length:       3.8 steps
  Reach rate:       100%
```

The trained agent:
- Reliably reaches the beacon (**100% reach rate**)
- Does so in ~4 steps on average (near-optimal for the lattice)
- Achieves reward 99.7 (terminal bonus + per-step gradient)
- Improves from 10% reach rate to 100%

### PPO configuration

```python
PPO("MlpPolicy", env, n_steps=2048, ent_coef=0.15,
    policy_kwargs=dict(net_arch=[128, 128]))
```

Key hyperparameter choices:
- **`net_arch=[128, 128]`**: larger than Heat Seeker's default 64x64,
  to handle 768-dim input and 13 actions
- **`ent_coef=0.15`**: high entropy coefficient prevents premature
  policy collapse. FCC12's 13 actions require the agent to maintain
  action diversity — each offset changes 2 of 3 axes, so the agent
  must alternate between different offsets to navigate all dimensions
- **`n_steps=2048`**: longer rollouts give better value estimates for
  the sparse terminal reward

## What's next

- **3D visualization:** render the FCC lattice with beacon/radiation
  fields as a point cloud, animate the agent's trajectory
- **Local observations:** switch to `AgentDisk` with radius ~3 for
  agent-centered perception that scales to larger lattices
- **Rust propagator:** move the graph Laplacian to Rust for ~100x tick
  throughput
- **Multiple agents:** add competitive or cooperative multi-agent
  dynamics on the same lattice
- **Richer topology:** use `ProductSpace(Fcc12, Line1D)` for a 4D
  lattice or combine with `Hex2D` for mixed-dimension worlds
- **Obstacle fields:** add `Static` terrain fields that block certain
  cells, forcing the agent to learn to route around barriers
