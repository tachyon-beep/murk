# Hex Pursuit

Multi-agent predator-prey on a hexagonal grid.

Demonstrates:
- **Hex2D** space (6-connected, pointy-top axial coordinates)
- **Multi-agent observation** via `ObsPlan.execute_agents()`
- **AgentDisk** foveation (each agent sees a local disk, not the full grid)
- **SetField commands** for agent movement
- **Competitive reward** design (predator minimizes distance, prey maximizes it)

## Key concept: foveation

Each agent observes through an `AgentDisk(radius=3)`, which gives a fixed
49-cell view centered on the agent. On this 12x12 grid (144 cells), that's
about 34% of the world. On a 100x100 grid (10,000 cells), the observation
would still be 49 cells — this is how Murk scales to large worlds.

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

## Running

```bash
cd crates/murk-python && maturin develop --release
python examples/hex_pursuit/hex_pursuit.py
```

## Extending with RL

This example uses random policies. To train with RL:

1. Wrap the game loop in a Gymnasium `Env` subclass
2. Use `obs_buf[:obs_per_agent]` as the predator's observation
3. Use `obs_buf[obs_per_agent:]` as the prey's observation
4. Train with self-play (e.g., PPO for both agents alternating)
