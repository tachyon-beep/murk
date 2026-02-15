# Crystal Navigator

3D navigation on a face-centred cubic lattice with competing diffusion fields.

An agent navigates an 8x8x8 FCC12 lattice (256 cells, 12-connected) with two
diffusion fields pulling in opposite directions: a **beacon scent** that
attracts the agent toward a goal, and a **radiation hazard** that repels it
from a danger zone. The agent must follow the beacon gradient while steering
around the radiation field to reach the beacon cell.

**Demonstrates:**
- FCC12 topology (12-connected, 3D, isotropic connectivity)
- Graph Laplacian diffusion (topology-agnostic, sentinel trick for vectorized numpy)
- Dual competing reward signals (beacon attractive, radiation repulsive)
- 13-action discrete control (stay + 12 FCC offsets)
- Python propagator with Jacobi-style reads

## Prerequisites

```bash
# 1. Build the Murk Python extension
cd crates/murk-python
pip install maturin
maturin develop --release

# 2. Install RL dependencies
pip install stable-baselines3 gymnasium numpy torch
```

## Running

```bash
python examples/crystal_nav/crystal_nav.py
```

Training runs for 1,000,000 PPO timesteps. Expect 5--15 minutes depending on
hardware. The script prints a before/after comparison and a sample trajectory.

## What the output means

The script evaluates the agent before and after training:

```
Evaluating random policy (before training)...
  Mean reward:    -150.0
  Mean length:     300.0 steps
  Reach rate:        0%
```

The random policy never finds the beacon. With 13 actions, 256 cells, and a
300-step limit, random walks rarely stumble onto the goal. The negative reward
accumulates from the step penalty and occasional radiation exposure.

```
Evaluating trained policy (after training)...
  Mean reward:      95.0
  Mean length:       8.0 steps
  Reach rate:      100%
```

The trained agent reliably reaches the beacon in under 10 steps while avoiding
the radiation zone. The reward is dominated by the +100 terminal bonus.

The sample trajectory shows each step's action (as an FCC offset like
`(+1,+1,0)`), the agent's 3D position, and the per-step reward. A `***`
marker indicates the terminal step where the agent reaches the beacon.

## Key design decisions

### Why graph Laplacian instead of np.pad

The heat_seeker example uses `np.pad(field, 1, mode="edge")` to compute the
discrete Laplacian on a rectangular grid. That works because Square4 neighbors
are adjacent in row/column -- padding a 2D array by one element on each side
gives direct access to all four neighbors.

FCC12 connectivity does not map to array adjacency. Each cell has up to 12
neighbors at offsets like `(+1, +1, 0)`, and only cells with even coordinate
parity `(x + y + z) % 2 == 0` exist in the lattice. There is no way to
reshape the field into an array where adjacent elements correspond to FCC
neighbors.

Instead, we precompute an explicit neighbor-index array `NBR_IDX[cell, 12]`
and compute the combinatorial graph Laplacian directly:

```
L*u = sum_neighbors(u_nbr) - degree(v) * u_v
```

This formulation is topology-agnostic. The same diffusion code works on
Square4, Hex2D, FCC12, or an arbitrary graph -- only the `NBR_IDX` table
changes.

### The sentinel trick

`NBR_IDX` has shape `(256, 12)`, but boundary cells have fewer than 12
neighbors. Missing neighbor slots point to a **sentinel index** at position
256 (one past the last real cell). Before each diffusion step, we allocate a
padded array of length 257 with the sentinel element fixed at 0.0:

```python
padded = np.zeros(cell_count + 1, dtype=np.float32)
padded[:cell_count] = prev_field
nbr_sum = padded[NBR_IDX].sum(axis=1)
```

Fancy indexing gathers 0.0 for every missing neighbor, so `nbr_sum` is correct
without any Python-level branching or masking. The `DEGREE` array then corrects
the self-term (`-degree * u_v`) in the Laplacian. This is the combinatorial
graph Laplacian on the induced subgraph -- the sentinel is a vectorization
convenience for absent edges, not a "the outside world is zero" boundary
condition.

### Warmup ticks

On reset, all fields start at zero. The diffusion propagator needs time to
build a spatial gradient from each point source. We run **80 warmup ticks**
(no agent commands) before placing the agent.

80 ticks is more than the heat_seeker's 50 because the FCC12 mesh has a larger
graph diameter and 12-way connectivity spreads the signal more slowly per hop
(the diffusion coefficient per-edge is smaller to satisfy the non-negative
weights condition `12*D + decay < 1`). After 80 ticks, the beacon and
radiation fields are near steady-state and present a stable gradient for the
agent to follow.

### Reward shaping

The reward has three components:

```python
reward = GRADIENT_SCALE * (beacon[agent] - radiation[agent]) - STEP_PENALTY
if at_beacon:
    reward += TERMINAL_BONUS
```

| Component | Value | Purpose |
|-----------|-------|---------|
| Gradient term | `0.1 * (beacon - radiation)` | Directional shaping: move toward beacon, away from radiation |
| Step penalty | `-0.5` per step | Prevents camping at "good enough" positions |
| Terminal bonus | `+100` on reaching beacon | Makes goal-reaching far more valuable than gradient-riding |

The gradient term alone would let the agent sit at a high net-field position
without reaching the beacon. The step penalty makes standing still costly.
The terminal bonus makes the exact beacon cell worth more than any amount of
gradient-riding.

The two diffusion fields use different parameters to create distinct spatial
profiles:
- **Beacon** (D=0.06, decay=0.01): wide spread, detectable ~6 hops away
- **Radiation** (D=0.04, decay=0.03): sharp local hazard, drops off within ~2 hops

This asymmetry means the agent can sense the beacon from far away but only
"feels" the radiation when it gets close -- matching the intuition of a
long-range beacon and a localized danger zone.

### Entropy coefficient

FCC12 has 13 actions where each offset changes exactly 2 of 3 coordinate axes.
Without sufficient exploration, PPO collapses to a single action (e.g., always
`(+1,+1,0)`) and never discovers z-axis movement. An entropy coefficient of
0.15 prevents premature policy collapse and ensures the agent explores movement
in all three dimensions before converging.

## What's next

- **Action masking:** `VALID_MASK` is precomputed but unused during training.
  Passing it to a masked PPO variant would eliminate wasted boundary-bounce
  actions.
- **Rust propagator:** move `dual_diffusion_step` to Rust for ~100x tick
  throughput.
- **Local observations:** switch to `AgentDisk` for agent-centered perception
  that scales to larger lattices.
- **Dynamic sources:** randomize beacon and radiation positions each episode
  to test generalization.
- **Multiple agents:** add cooperative or competitive dynamics on the same
  lattice.
