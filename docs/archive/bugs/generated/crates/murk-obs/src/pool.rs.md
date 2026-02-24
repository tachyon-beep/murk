# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [x] Critical | [ ] High | [ ] Medium | [ ] Low

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

Unchecked `usize` multiplications in pooling shape/length validation overflow and allow malformed `input_shape` values to trigger deterministic panics.

## Steps to Reproduce

1. Call `pool_2d_into` with a shape whose product overflows `usize`, e.g. `h = usize::MAX / 2 + 1`, `w = 2`, `kernel_size = 1`, `stride = 1`.
2. Pass empty `input`, `input_mask`, `output`, and `output_mask` slices.
3. Execute in debug or release.

## Expected Behavior

The function should reject overflowing dimensions explicitly (e.g., checked arithmetic + error/assert message) before any indexing or loop execution.

## Actual Behavior

In debug builds, `h * w` overflows and panics during validation.  
In release builds, overflowed products can pass length checks, then later panic on out-of-bounds indexing inside the pooling loop.

## Reproduction Rate

Always

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

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
use murk_obs::pool::pool_2d_into;
use murk_obs::spec::{PoolConfig, PoolKernel};

fn main() {
    let cfg = PoolConfig {
        kernel: PoolKernel::Mean,
        kernel_size: 1,
        stride: 1,
    };

    let h = usize::MAX / 2 + 1; // checked_mul(h, 2) == None
    let w = 2usize;

    // Debug: panic on overflow in h * w validation.
    // Release: overflowed checks may pass, then panic on indexing.
    let _ = pool_2d_into(&[], &[], &[h, w], &cfg, &mut [], &mut []);
}
```

## Additional Context

Evidence:
- Overflow-prone validation multiply at `/home/john/murk/crates/murk-obs/src/pool.rs:68`
- Overflow-prone mask-length multiply at `/home/john/murk/crates/murk-obs/src/pool.rs:69`
- Overflow-prone output-length multiply at `/home/john/murk/crates/murk-obs/src/pool.rs:74`
- Subsequent indexing that panics after bad validation at `/home/john/murk/crates/murk-obs/src/pool.rs:106`

Root cause:
- Arithmetic uses unchecked `usize` multiplication (`h * w`, `out_h * out_w`) for safety-critical shape validation and buffer sizing.

Suggested fix:
- Replace multiplications with `checked_mul` (and `checked_add` where applicable), and fail fast with a clear error/assert when dimensions overflow.