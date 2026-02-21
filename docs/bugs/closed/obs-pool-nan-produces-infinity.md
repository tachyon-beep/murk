# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [x] murk-obs
- [ ] murk-replay
- [ ] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`pool_2d` returns `-inf` (Max) or `+inf` (Min) with `output_mask = 1` (valid) when all mask-valid cells in a pooling window contain `NaN` values. This happens because:

1. `valid_count` is incremented for every mask-valid cell regardless of NaN (line 62).
2. The `>` / `<` comparisons (lines 66, 71) always evaluate to `false` for NaN, so the accumulator retains its sentinel value (`NEG_INFINITY` for Max, `INFINITY` for Min).
3. Since `valid_count > 0`, the output is marked valid (line 80) and the sentinel is emitted as a real pooled value (line 84).

This silently injects infinity values into observation tensors, which can corrupt downstream neural network training.

## Steps to Reproduce

1. Create a 1x1 input with `input = [NaN]`, `input_mask = [1]`, shape `[1, 1]`.
2. Call `pool_2d` with `PoolKernel::Max`, kernel_size=1, stride=1.
3. Observe output is `[-inf]` with mask `[1]`.

## Expected Behavior

Output should either:
- Mark the window as invalid (`output_mask = 0`) since no numeric value was aggregated, OR
- Propagate `NaN` explicitly as the pooled value (if NaN propagation is the chosen policy).

## Actual Behavior

Output is `f32::NEG_INFINITY` for Max (or `f32::INFINITY` for Min) with `output_mask = 1`, falsely indicating a valid numeric result.

## Reproduction Rate

- Deterministic whenever all mask-valid cells in a window are NaN.

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
use murk_obs::pool::pool_2d;
use murk_obs::spec::{PoolConfig, PoolKernel};

let input = vec![f32::NAN];
let mask = vec![1u8];
let cfg = PoolConfig { kernel: PoolKernel::Max, kernel_size: 1, stride: 1 };

let (output, out_mask, _) = pool_2d(&input, &mask, &[1, 1], &cfg);

// BUG: output is [-inf] with mask [1] (valid)
assert_eq!(out_mask[0], 1);           // passes -- incorrectly marked valid
assert!(output[0].is_nan());          // FAILS -- output is NEG_INFINITY, not NaN
assert!(output[0] == f32::NEG_INFINITY); // passes -- sentinel leaked
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-obs/src/pool.rs.md
**Verified lines:** pool.rs:50-51 (sentinel init), pool.rs:62 (valid_count unconditional), pool.rs:66,71 (NaN-blind comparisons), pool.rs:80,84 (emission)
**Root cause:** `valid_count` counts mask-valid cells without checking for NaN. The comparator-based Max/Min update never fires for NaN operands.
**Suggested fix:** Skip NaN values when incrementing `valid_count` for Max/Min kernels. Check `val.is_nan()` before the comparison, and only count non-NaN values. If no non-NaN values were seen, leave `output_mask = 0`. Mean/Sum already handle this correctly because `accum += NaN` propagates NaN through the accumulator.
