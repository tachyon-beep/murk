# Layered Hex World

ProductSpace composition: navigating a 3-floor hexagonal building.

An agent navigates a building with three hexagonal floors. Each floor is a
6x6 Hex2D grid (36 cells, 6-connected), and the floors are stacked vertically
via a Line1D with 3 cells (Absorb edges). The agent starts at a random
position on floor 0 and must reach a goal in the far corner of floor 2.

**Demonstrates:**
- ProductSpace composition (Hex2D x Line1D = 108 cells)
- Low-level `set_space()` for ProductSpace parameter encoding
- Cross-component navigation (hex movement + floor transitions)
- Graph Laplacian diffusion across a composed space
- 9-action discrete control (stay + 6 hex directions + down + up)

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
python examples/layered_hex/layered_hex.py
```

Training runs for 200,000 PPO timesteps. Expect 1--5 minutes depending on
hardware. The script prints a before/after comparison and a sample trajectory.

## What the output means

The script evaluates the agent before and after training:

```
Evaluating random policy (before training)...
  Mean reward:    -100.0
  Mean length:     200.0 steps
  Reach rate:        0%
```

The random policy almost never finds the goal. With 9 actions and a 200-step
limit, random walks on 108 cells rarely reach the specific target cell on
a different floor. The negative reward accumulates from the step penalty.

```
Evaluating trained policy (after training)...
  Mean reward:      94.0
  Mean length:      12.0 steps
  Reach rate:      100%
```

The trained agent reliably reaches the goal by following the beacon gradient
up through the floors. The terminal bonus (+100) dominates the reward.

The sample trajectory shows each step's action (hex direction or floor
change), the agent's 3D position as `(q,r,fZ)`, and the per-step reward.
A `***` marker indicates the terminal step where the agent reaches the goal.

## How ProductSpace works

### Composition

ProductSpace takes N component spaces and creates their Cartesian product.
Each cell in the product space is an N-tuple of per-component coordinates,
concatenated into a single flat coordinate:

```
Hex2D(q, r) x Line1D(z)  -->  coord = [q, r, z]
```

For a 6x6 hex grid with 3 floors, the product space has `36 * 3 = 108` cells.

### Python API

There is no typed `set_space_product()` method. ProductSpace uses the
low-level `set_space()` with a flat parameter array. Flat encoding is
necessary because ProductSpace supports variable-length nesting -- each
component can have a different number of parameters, so a fixed-signature
method cannot represent all possible compositions. Here is what that
looks like in code:

```python
config.set_space(
    SpaceType.ProductSpace,
    [
        2.0,   # n_components
        4.0,   # type_0 = Hex2D (SpaceType enum value)
        2.0,   # n_params_0 (Hex2D takes 2 params)
        6.0,   # cols
        6.0,   # rows
        0.0,   # type_1 = Line1D (SpaceType enum value)
        2.0,   # n_params_1 (Line1D takes 2 params)
        3.0,   # length
        0.0,   # edge_behavior = Absorb
    ],
)
```

The encoding format is:
```
[n_components, type_0, n_params_0, p0_0, p0_1, ..., type_1, n_params_1, p1_0, ...]
```

SpaceType enum values: Line1D=0, Ring1D=1, Square4=2, Square8=3, Hex2D=4,
ProductSpace=5, Fcc12=6. EdgeBehavior values: Absorb=0, Clamp=1, Wrap=2.

### Neighbours

ProductSpace neighbours vary one component at a time while holding the
others constant (R-SPACE-8):

- 6 hex neighbours: change `(q, r)`, hold `z` fixed
- Up to 2 line neighbours: change `z`, hold `(q, r)` fixed
- Total: up to 8 neighbours per cell (interior cells)

This means the agent can move within a hex floor OR change floors, but not
both in a single step. This is the defining property of ProductSpace
connectivity -- it creates a graph where components are orthogonal axes
of movement.

### Canonical ordering

ProductSpace uses lexicographic ordering with the leftmost component
slowest and the rightmost fastest (R-SPACE-10). For Hex2D x Line1D:

```
hex_rank(q, r) = r * cols + q     (hex uses r-then-q)
product_rank(q, r, z) = hex_rank(q, r) * n_floors + z
```

So the first cells in order are: `(0,0,0), (0,0,1), (0,0,2), (1,0,0),
(1,0,1), (1,0,2), ...` -- all floors for hex cell (0,0), then all floors
for hex cell (1,0), and so on.

### Diffusion across the product graph

The beacon diffusion propagator uses the graph Laplacian on the full
product graph. This means the scent spreads both within hex floors
(through hex edges) and between floors (through line edges). A beacon
placed on floor 2 creates a gradient that the agent can follow both
horizontally and vertically.

The sentinel trick (same as crystal_nav) handles boundary cells
efficiently: missing neighbour slots in the adjacency array point to an
extra zero-valued element, so `padded[NBR_IDX].sum(axis=1)` computes
the correct neighbour sum without branching.

### Commands with ProductSpace coordinates

`Command.set_field` takes a coordinate as a list of integers. For
ProductSpace, this is the concatenated per-component coordinate:

```python
Command.set_field(AGENT_FIELD, [q, r, z], 1.0)
```

The engine maps `[q, r, z]` to the canonical rank and writes the value
to that cell in the field buffer.

## Key design decisions

### Why floor transitions are separate actions

The 9-action space separates hex movement (6 directions) from floor
changes (up/down). This reflects the ProductSpace structure: each action
varies exactly one component. An alternative would be to allow diagonal
movement in the product graph (change hex AND floor simultaneously),
but that would not match the ProductSpace neighbour definition and would
require a different space topology.

### Why Absorb edges on the line component

Absorb means the agent cannot move beyond floor 0 or floor 2 -- the
floor-change action becomes a no-op at the boundaries. This models a
building with a ground floor and a top floor. Wrap would connect floor 0
to floor 2, creating a loop (useful for a cylindrical topology). Clamp
would self-loop at the boundary, which is functionally similar to Absorb
for movement but differs for diffusion boundary conditions.

### Why 200K timesteps is sufficient

The product space has only 108 cells and the optimal path is short
(~10 steps from floor 0 to floor 2 goal). The beacon gradient provides
strong shaping signal from the start. PPO converges quickly because:
- Small observation (216 floats)
- Clear gradient signal (beacon diffuses across floors)
- Short optimal episode length (~10 steps)

Compare with crystal_nav (1M timesteps for 256 cells in 3D FCC12) --
the layered hex world is simpler because the product space structure
provides a natural "highway" between floors.

## What's next

- **More floors:** increase `N_FLOORS` to 5 or 10 to create a taller
  building where floor transitions become a larger fraction of the
  optimal path
- **Obstacles:** add a static field marking blocked hex cells (walls)
  and modify the propagator to zero diffusion through them
- **Multiple staircases:** place floor-transition portals at specific
  hex cells rather than allowing floor changes everywhere
- **Three-component product:** Hex2D x Line1D x Ring1D for a cylindrical
  building with wrapping corridors
- **Rust propagator:** move `beacon_diffusion_step` to Rust for ~100x
  tick throughput
