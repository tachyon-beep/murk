# Hex Pursuit

Multi-agent predator-prey on a hexagonal grid.

A predator chases a prey on a 12x12 Hex2D grid (144 cells). Both agents
see only a local disk around their position via `AgentDisk(radius=3)`,
giving each agent a fixed 37-cell view regardless of grid size. The
predator is rewarded for closing distance; the prey is rewarded for
increasing it. The episode ends when the predator catches the prey or a
tick limit (100 steps) is reached.

**This example uses random policies** to demonstrate the Murk API for
multi-agent scenarios. For a complete RL training example with PPO, see
[heat_seeker](../heat_seeker/).

**Demonstrates:**
- **Hex2D** space (6-connected, pointy-top axial coordinates)
- **Multi-agent observation** via `ObsPlan.execute_agents()`
- **AgentDisk** foveation (each agent sees a local disk, not the full grid)
- **SetField commands** for agent movement
- **Competitive reward** design (predator minimizes distance, prey maximizes it)

## Prerequisites

```bash
# 1. Build the Murk Python extension
cd crates/murk-python
pip install maturin
maturin develop --release

# 2. Install dependencies
pip install numpy
```

No RL libraries are required since this example uses random policies.
To extend it with RL training, you will also need:

```bash
pip install stable-baselines3 gymnasium torch
```

## Running

```bash
python examples/hex_pursuit/hex_pursuit.py
```

## Expected output

```
============================================================
  Hex Pursuit: multi-agent predator-prey on Hex2D
============================================================

  Grid:        12x12 Hex2D (144 cells)
  Agents:      predator + prey
  Perception:  AgentDisk radius=3
  Actions:     6 (hex directions) + 1 (stay) = 7

  Obs per agent: 74 floats
  Mask per agent: 74 bytes
  (Compare: full grid would be 288 floats)

  Episode 1: CAUGHT at tick  42, pred_reward=  -192.9, final_dist=0
  Episode 2: ESCAPED at tick 100, pred_reward=  -587.0, final_dist=7
  Episode 3: CAUGHT at tick  71, pred_reward=  -298.5, final_dist=0
  Episode 4: ESCAPED at tick 100, pred_reward=  -498.0, final_dist=5
  Episode 5: ESCAPED at tick 100, pred_reward=  -612.0, final_dist=8

Done. With RL training, the predator learns to chase efficiently
while the prey learns evasion — both using only local observations.
```

With random policies, the predator occasionally catches the prey by
chance. Exact numbers will vary with the random seed, but expect most
episodes to end with ESCAPED and negative predator rewards.

## Key concept: foveation

Each agent observes through an `AgentDisk(radius=3)`, which gives a fixed
view centered on the agent. On this 12x12 grid (144 cells), that's
about 25% of the world. On a 100x100 grid (10,000 cells), the observation
would still be the same size -- this is how Murk scales to large worlds.

```python
# Observation plan with local perception
obs_entries = [
    ObsEntry(PREDATOR_FIELD, region_type=RegionType.AgentDisk, region_params=[3]),
    ObsEntry(PREY_FIELD, region_type=RegionType.AgentDisk, region_params=[3]),
]

# Execute for multiple agents at once
agent_centers = np.array([[pred_q, pred_r], [prey_q, prey_r]], dtype=np.int32)
plan.execute_agents(world, agent_centers, obs_buf, mask_buf)
```

## Code walkthrough

### World setup (lines 104-138)

The script creates a `Config` with a 12x12 `Hex2D` space and two scalar
fields (`predator` and `prey`), both `PerTick`. An identity propagator
copies previous field values forward each tick -- agent positions are
set entirely by `SetField` commands, not by physics.

```python
config = Config()
config.set_space_hex2d(COLS, ROWS)
config.add_field("predator", mutability=FieldMutability.PerTick)
config.add_field("prey", mutability=FieldMutability.PerTick)
```

### Observation plan (lines 140-172)

An `ObsPlan` is compiled with two `AgentDisk` entries (one per field).
The plan is executed for both agents simultaneously via
`execute_agents()`, which fills a single flat buffer with both agents'
observations concatenated. Pre-allocated numpy buffers avoid per-tick
allocation.

### Episode loop (lines 176-256)

Each episode:
1. **Reset** the world and place agents at random positions (at least 4
   hex steps apart).
2. **Observe** both agents via `execute_agents()` with their current
   positions as disk centers.
3. **Decide** actions using a random policy (0-5 = hex direction, 6 = stay).
4. **Move** agents using `hex_move()`, which clamps to grid bounds (absorb
   boundary behavior).
5. **Step** the world with `SetField` commands that clear old positions
   and stamp new ones.
6. **Reward**: predator reward is `-distance` (negative hex distance to
   prey). A +50 catch bonus is awarded if the predator reaches the prey.
7. **Terminate** if distance reaches 0 (caught) or tick limit is reached
   (escaped).

### Hex coordinate helpers (lines 84-99)

`hex_distance()` computes cube distance between two axial hex coordinates.
`hex_move()` moves one step in a hex direction with absorb boundaries
(clamping to valid grid range).

## Extending with RL

This example uses random policies. To train with RL:

1. Wrap the game loop in a Gymnasium `Env` subclass
2. Use `obs_buf[:obs_per_agent]` as the predator's observation
3. Use `obs_buf[obs_per_agent:]` as the prey's observation
4. Train with self-play (e.g., PPO for both agents alternating)

See [heat_seeker](../heat_seeker/) for a complete RL training example
that demonstrates the Gymnasium integration pattern, PPO configuration,
and evaluation.
