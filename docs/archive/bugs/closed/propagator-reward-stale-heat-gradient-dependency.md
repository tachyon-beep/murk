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
- [x] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [ ] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [x] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`RewardPropagator::reads()` declares a dependency on `HEAT_GRADIENT`, but `step()` never reads that field. This creates an unnecessary hard dependency that forces pipeline validation to require `HEAT_GRADIENT` to be a defined field, even though the reward computation only uses `HEAT` and `AGENT_PRESENCE`. Users who want a reward propagator without a diffusion propagator (which writes `HEAT_GRADIENT`) will hit `PipelineError::UndefinedField` at startup.

## Steps to Reproduce

1. Create a `RewardPropagator`.
2. Register it in a pipeline where `HEAT` and `AGENT_PRESENCE` are defined but `HEAT_GRADIENT` is not.
3. Call `validate_pipeline()`.
4. Observe `PipelineError::UndefinedField` for `HEAT_GRADIENT`.

## Expected Behavior

`RewardPropagator::reads()` should only declare `[HEAT, AGENT_PRESENCE]`, matching the fields actually read in `step()`. Pipeline validation should pass without requiring `HEAT_GRADIENT` to be defined.

## Actual Behavior

`reads()` at reward.rs:40 returns `[HEAT, AGENT_PRESENCE, HEAT_GRADIENT]`. Pipeline validation at pipeline.rs:208-214 checks all declared `reads()` fields against `defined_fields` and rejects missing ones. The unit tests at reward.rs:108 and reward.rs:144 work around this by injecting dummy `HEAT_GRADIENT` data into the mock reader.

## Reproduction Rate

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_propagators::reward::RewardPropagator;
use murk_propagator::propagator::Propagator;
use murk_propagators::fields::{HEAT, AGENT_PRESENCE, HEAT_GRADIENT};

let prop = RewardPropagator::new(1.0, -0.1);
let reads = prop.reads();

// BUG: HEAT_GRADIENT is declared but never used
assert!(reads.contains(HEAT_GRADIENT)); // This passes
// step() only calls ctx.reads().read(HEAT) and ctx.reads().read(AGENT_PRESENCE)
// There is no ctx.reads().read(HEAT_GRADIENT) anywhere in step()
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-propagators/src/reward.rs.md
**Verified lines:** reward.rs:40 (reads() includes HEAT_GRADIENT), reward.rs:47-79 (step() only reads HEAT and AGENT_PRESENCE), reward.rs:108,144 (tests inject dummy HEAT_GRADIENT)
**Root cause:** A stale dependency remained in `reads()` after reward logic was simplified to depend only on heat and agent presence.
**Suggested fix:**
1. Remove `HEAT_GRADIENT` from `RewardPropagator::reads()`, keeping only `[HEAT, AGENT_PRESENCE]`.
2. Remove the `HEAT_GRADIENT` import from the use statement (line 6) if no longer needed.
3. Update reward tests to stop injecting dummy `HEAT_GRADIENT` data.
