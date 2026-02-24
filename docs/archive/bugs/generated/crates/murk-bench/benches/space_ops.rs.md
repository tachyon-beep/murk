# Bug Report

**Date:** 2026-02-23
**Reporter:** static-analysis-agent
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

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
- [ ] murk-python
- [x] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

No concrete bug found in /home/john/murk/crates/murk-bench/benches/space_ops.rs.

## Steps to Reproduce

1. Run `cargo bench -p murk-bench --bench space_ops`.
2. Execute all benchmark functions in `crates/murk-bench/benches/space_ops.rs`.
3. Review benchmark code paths for overflow, panic/UB, truncation, and bounds misuse.

## Expected Behavior

Benchmarks execute without panic/UB and measure the intended space operations.

## Actual Behavior

No concrete failure found from static analysis of the target file; code paths are bounded and inputs remain within constructed space dimensions.

## Reproduction Rate

Always (no bug observed).

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

```text
N/A - no concrete bug identified in the target file.
```

## Additional Context

Evidence reviewed (with exact locations): `/home/john/murk/crates/murk-bench/benches/space_ops.rs:13`, `/home/john/murk/crates/murk-bench/benches/space_ops.rs:30`, `/home/john/murk/crates/murk-bench/benches/space_ops.rs:49`, `/home/john/murk/crates/murk-bench/benches/space_ops.rs:55`, `/home/john/murk/crates/murk-bench/benches/space_ops.rs:60`, `/home/john/murk/crates/murk-bench/benches/space_ops.rs:79`, `/home/john/murk/crates/murk-bench/benches/space_ops.rs:101`, `/home/john/murk/crates/murk-bench/benches/space_ops.rs:106`, `/home/john/murk/crates/murk-bench/benches/space_ops.rs:107`, `/home/john/murk/crates/murk-bench/benches/space_ops.rs:108`, `/home/john/murk/crates/murk-bench/benches/space_ops.rs:135`.  
Cross-check of coordinate contracts used by this benchmark: `/home/john/murk/crates/murk-space/src/hex2d.rs:21`, `/home/john/murk/crates/murk-space/src/hex2d.rs:334`, `/home/john/murk/crates/murk-space/src/product.rs:377`, `/home/john/murk/crates/murk-space/src/product.rs:532`.