# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [x] murk-propagators

## Engine Mode

- [x] Lockstep

## Summary

Several per-tick performance issues in library propagators:

1. **MorphologicalOp (morphological_op.rs:181-217):** Allocates a new `HashSet` and `VecDeque` per cell for BFS. On a 100x100 grid with radius=3, this is 10,000 BFS allocations per tick. Fix: pre-allocate and clear between iterations.

2. **AgentMovement (agent_movement.rs:174):** Linear O(n) scan per agent per tick to find agent position via `iter().position()`. With k agents and n cells, total is O(k*n). Fix: build a `HashMap<u16, usize>` mapping agent_id to position in one O(n) pass.

3. **NoiseInjection (noise_injection.rs:71-75):** Box-Muller generates two Gaussian samples but only uses the cosine term, wasting half the RNG. Fix: cache the spare sample.

4. **RewardPropagator (reward.rs:64-65):** Copies field data via `to_vec()` per tick without validating field component count.

## Expected Behavior

Amortized allocations and efficient lookups on the per-tick hot path.

## Actual Behavior

Per-cell/per-agent allocations and linear scans.

## Additional Context

**Source:** murk-propagators audit, M-3/M-4/M-7/M-6
