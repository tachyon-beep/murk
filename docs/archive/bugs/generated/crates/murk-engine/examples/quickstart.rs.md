# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [ ] murk-core
- [x] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
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
- [ ] Both / Unknown

## Summary

No concrete bug found in /home/john/murk/crates/murk-engine/examples/quickstart.rs.

## Steps to Reproduce

1. Inspect `/home/john/murk/crates/murk-engine/examples/quickstart.rs` for panic/UB/overflow/indexing/FFI/atomic issues.
2. Verify index math and field access safety at `/home/john/murk/crates/murk-engine/examples/quickstart.rs:111`, `/home/john/murk/crates/murk-engine/examples/quickstart.rs:113`, `/home/john/murk/crates/murk-engine/examples/quickstart.rs:278`.
3. Verify field definitions and write/read consistency at `/home/john/murk/crates/murk-engine/examples/quickstart.rs:181`, `/home/john/murk/crates/murk-engine/examples/quickstart.rs:205`, `/home/john/murk/crates/murk-engine/examples/quickstart.rs:228`.

## Expected Behavior

The quickstart example should execute the Lockstep diffusion simulation and command injection flow without panic or out-of-bounds access.

## Actual Behavior

Static analysis found no concrete defect in the target file.  
Note: runtime execution could not be performed in this environment due sandbox write restrictions.

## Reproduction Rate

N/A (no bug found)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

## Determinism Impact

- [ ] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// N/A - no concrete bug found in target file.
```

## Additional Context

Reviewed likely-risk points and found them internally consistent for this example:
- Indexing loops are bounded by fixed dimensions and consistent with configured space size (`/home/john/murk/crates/murk-engine/examples/quickstart.rs:35`, `/home/john/murk/crates/murk-engine/examples/quickstart.rs:36`, `/home/john/murk/crates/murk-engine/examples/quickstart.rs:167`).
- Heat/source reads and writes match declared fields (`/home/john/murk/crates/murk-engine/examples/quickstart.rs:69`, `/home/john/murk/crates/murk-engine/examples/quickstart.rs:73`, `/home/john/murk/crates/murk-engine/examples/quickstart.rs:181`).
- `unwrap()` calls are on fields defined in config and used immediately in same world setup (`/home/john/murk/crates/murk-engine/examples/quickstart.rs:228`, `/home/john/murk/crates/murk-engine/examples/quickstart.rs:277`, `/home/john/murk/crates/murk-engine/examples/quickstart.rs:288`).