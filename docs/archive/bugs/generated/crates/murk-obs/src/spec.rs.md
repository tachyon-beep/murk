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

No concrete bug found in /home/john/murk/crates/murk-obs/src/spec.rs.

## Steps to Reproduce

1. Inspect `/home/john/murk/crates/murk-obs/src/spec.rs:1`.
2. Verify the file contents are declarations only (structs/enums/fields), through `/home/john/murk/crates/murk-obs/src/spec.rs:194`.
3. Confirm there is no executable logic in this file that can trigger overflow, truncation-at-runtime, panic paths, unsafe misuse, or geometry/pooling behavior.

## Expected Behavior

A concrete, reproducible bug should be identifiable in the target file if one exists.

## Actual Behavior

No concrete runtime bug is present in this file alone; it is a spec/type-definition module without behavior to execute.

## Reproduction Rate

Not reproducible (no bug found in this file).

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
// N/A: no concrete bug found in /home/john/murk/crates/murk-obs/src/spec.rs.
```

## Additional Context

Evidence inspected: `/home/john/murk/crates/murk-obs/src/spec.rs:1` through `/home/john/murk/crates/murk-obs/src/spec.rs:194`.  
The file defines observation-spec data types and enums only; bug-prone behaviors called out in scope (e.g., `u16` truncation on counts, `is_interior` dimensional checks, `pool_2d` NaN handling) would need to be validated in implementation files that consume these types.