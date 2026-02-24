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

No concrete bug found in /home/john/murk/crates/murk-bench/benches/codec_ops.rs.

## Steps to Reproduce

1. Inspect `/home/john/murk/crates/murk-bench/benches/codec_ops.rs`.
2. Evaluate reachable panic/overflow/FFI/unsafe/truncation/indexing/resource-leak paths.
3. No concrete incorrect behavior is reproducible from this file as written.

## Expected Behavior

Benchmark helper/build code should construct valid inputs and measure codec/hash operations without introducing its own correctness defects.

## Actual Behavior

No concrete defect identified in this file.

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

```text
N/A - found via static analysis
```

## Minimal Reproducer

```text
N/A - no concrete bug found.
```

## Additional Context

Evidence inspected (line-specific):
- `/home/john/murk/crates/murk-bench/benches/codec_ops.rs:60` (`encode_frame(...).unwrap()`) is in benchmark-only code with locally constructed valid frame input.
- `/home/john/murk/crates/murk-bench/benches/codec_ops.rs:72` pre-encoding path mirrors line 60 under same valid input assumptions.
- `/home/john/murk/crates/murk-bench/benches/codec_ops.rs:77` decode path consumes a buffer produced by the matching encoder in line 72.
- `/home/john/murk/crates/murk-bench/benches/codec_ops.rs:89` hash call uses fixed field count matching the snapshot construction loop at line 44.