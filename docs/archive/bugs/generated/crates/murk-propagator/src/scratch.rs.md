# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [x] Critical | [ ] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [x] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
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

`ScratchRegion::with_byte_capacity()` can panic on large `bytes` inputs (e.g. `usize::MAX`) because it immediately allocates a `Vec` with that computed slot count, causing capacity-overflow panic instead of graceful error handling.

## Steps to Reproduce

1. Call `ScratchRegion::with_byte_capacity(usize::MAX)`.
2. This computes `slots = usize::MAX / 4 + 1` in `scratch.rs`.
3. `ScratchRegion::new(slots)` executes `vec![0.0; slots]` and panics (`capacity overflow`).

## Expected Behavior

Oversized scratch requests should be rejected gracefully (e.g., error return at config/build time), not panic.

## Actual Behavior

Deterministic panic during scratch initialization path.

Evidence:
- `/home/john/murk/crates/murk-propagator/src/scratch.rs:33`
- `/home/john/murk/crates/murk-propagator/src/scratch.rs:36`
- `/home/john/murk/crates/murk-propagator/src/scratch.rs:22`
- `/home/john/murk/crates/murk-propagator/src/scratch.rs:24`

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

```text
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_propagator::ScratchRegion;

fn main() {
    // Panics with Vec capacity overflow
    let _ = ScratchRegion::with_byte_capacity(usize::MAX);
}
```

## Additional Context

Root cause:
- `with_byte_capacity` computes a mathematically correct ceil division, but does not validate whether the resulting slot count is representable/allocatable for `Vec<f32>`.
- It then calls `new()`, which performs immediate allocation via `vec![0.0; capacity]`, panicking on overflow-sized capacity.

Suggested fix:
- Make byte-capacity construction fallible (e.g., `try_with_byte_capacity(bytes) -> Result<Self, ...>`), and pre-check against a safe maximum slot bound before allocation.
- Propagate that error through the engine construction path instead of panicking.