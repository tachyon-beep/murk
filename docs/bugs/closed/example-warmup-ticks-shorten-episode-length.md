# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [ ] murk-ffi
- [x] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [x] Lockstep
- [ ] RealtimeAsync
- [ ] Both / Unknown

## Summary

Three example environments suffer from shortened episode lengths because warmup ticks in `reset()` advance the absolute world `tick_id`, but truncation in the base `MurkEnv._check_truncated` checks `tick_id >= self._tick_limit` using the absolute tick count. None of the three examples override `_check_truncated` to use episode-relative tick counting.

Affected examples and effective episode shortening:
- **crystal_nav.py**: `WARMUP_TICKS=80` + 1 stamp tick = 81 warmup. `MAX_STEPS=300`. Effective agent actions: ~219.
- **heat_seeker.py**: `WARMUP_TICKS=50` + 1 stamp tick = 51 warmup. `MAX_STEPS=200`. Effective agent actions: ~149.
- **layered_hex.py**: `WARMUP_TICKS=60` + 1 stamp tick = 61 warmup. `MAX_STEPS=200`. Effective agent actions: ~139.

## Steps to Reproduce

1. Run any of the three examples.
2. Count the number of `step()` calls before truncation.
3. Observe that the agent gets significantly fewer actions than `MAX_STEPS`.

## Expected Behavior

The agent should get `MAX_STEPS` actions per episode, regardless of how many warmup ticks are consumed during reset.

## Actual Behavior

The agent gets `MAX_STEPS - WARMUP_TICKS - 1` actions per episode because warmup ticks consume the global tick budget.

## Reproduction Rate

- 100%, every episode.

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)
- **Python version (if murk-python):** 3.10+

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```python
from examples.heat_seeker.heat_seeker import HeatSeekerEnv

env = HeatSeekerEnv(seed=42)
obs, info = env.reset()
start_tick = info["tick_id"]
print(f"First tick after reset: {start_tick}")  # ~51, not 0

steps = 0
done = False
while not done:
    obs, reward, terminated, truncated, info = env.step(0)
    steps += 1
    done = terminated or truncated

print(f"Agent actions: {steps}")  # ~149, not 200
```

## Additional Context

**Source reports:**
- /home/john/murk/docs/bugs/generated/examples/crystal_nav/crystal_nav.py.md
- /home/john/murk/docs/bugs/generated/examples/heat_seeker/heat_seeker.py.md
- /home/john/murk/docs/bugs/generated/examples/layered_hex/layered_hex.py.md

**Verified lines:**
- `crates/murk-python/python/murk/env.py:151-152` (base class truncation logic)
- `examples/crystal_nav/crystal_nav.py:340,356,373` (warmup + tick limit)
- `examples/heat_seeker/heat_seeker.py:201,215-216,227` (warmup + tick limit)
- `examples/layered_hex/layered_hex.py:337,352-353,374` (warmup + tick limit)

**Root cause:** The base class `_check_truncated` uses absolute `tick_id` but examples consume ticks during warmup in `reset()`. Neither the base class nor the examples account for this offset.

**Suggested fix:** In each example, override `_check_truncated` to track episode-relative ticks:
```python
def reset(self, ...):
    ...
    self._episode_start_tick = info["tick_id"]
    return obs, info

def _check_truncated(self, obs, info):
    return (info["tick_id"] - self._episode_start_tick) >= MAX_STEPS
```
Alternatively, adjust `self._tick_limit = MAX_STEPS + WARMUP_TICKS + 1` (less clean but simpler).
