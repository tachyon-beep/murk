# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-python

## Engine Mode

- [x] Lockstep

## Summary

`BatchedVecEnv` (batched_vec_env.py) does not expose `observation_space` or `action_space` attributes. Unlike `MurkVecEnv` which provides these, `BatchedVecEnv` breaks compatibility with RL frameworks like stable-baselines3 and Gymnasium which expect these attributes on vectorized environments.

## Expected Behavior

`BatchedVecEnv` should expose `observation_space` and `action_space` (either as constructor params or derived from the obs spec).

## Actual Behavior

Attributes absent; SB3 integration fails at runtime.

## Additional Context

**Source:** murk-python audit, F-34
**File:** `crates/murk-python/python/murk/batched_vec_env.py`
