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

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`MurkVecEnv` docstring claims "Compatible with stable-baselines3 VecEnv interface" but the class does not implement the SB3 `VecEnv` contract:

1. `reset()` returns `(obs, infos)` tuple; SB3 expects `obs` only.
2. `step()` returns a 5-tuple `(obs, rewards, terminateds, truncateds, info_dict)`; SB3 expects a 4-tuple `(obs, rewards, dones, infos)`.
3. Missing required SB3 methods: `step_async`, `step_wait`, `env_is_wrapped`, `get_attr`, `set_attr`, `env_method`.
4. Does not inherit from `stable_baselines3.common.vec_env.VecEnv`.

The class actually follows Gymnasium conventions (5-tuple step, `(obs, info)` reset), which is a different API.

## Steps to Reproduce

1. Create a `MurkVecEnv` and pass it to any SB3 algorithm.
2. SB3 will fail when calling methods that do not exist or when unpacking return values.

## Expected Behavior

Either the class should actually implement the SB3 VecEnv interface, or the docstring should be corrected to state it follows Gymnasium conventions.

## Actual Behavior

Docstring is misleading. Users who trust the claim and pass `MurkVecEnv` to SB3 will get runtime errors.

## Reproduction Rate

- 100% when used with SB3.

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
from stable_baselines3 import PPO
from murk import MurkVecEnv

vec_env = MurkVecEnv([make_env_fn])
model = PPO("MlpPolicy", vec_env)  # Fails: VecEnv API mismatch
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-python/python/murk/vec_env.py.md
**Verified lines:** `crates/murk-python/python/murk/vec_env.py:20,42-54,56-87`
**Root cause:** The class follows Gymnasium conventions but claims SB3 compatibility. SB3's `VecEnv` has a different contract (4-tuple step, obs-only reset, async methods).
**Suggested fix:** Either (a) remove the SB3 compatibility claim and document the Gymnasium-style interface, or (b) implement a separate `MurkSB3VecEnv` that wraps `MurkVecEnv` and adapts to SB3's expected interface.
