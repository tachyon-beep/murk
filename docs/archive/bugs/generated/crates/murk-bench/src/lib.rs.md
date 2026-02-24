# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

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

`init_agent_positions` performs unchecked `u64` multiplication that overflows for `i >= 13`, causing a runtime panic in overflow-checked builds despite the function documentation promising “no panic.”

## Steps to Reproduce

1. Call `init_agent_positions(100, 14, 42)` in a debug/overflow-checked build.
2. Execution reaches the hash expression in `init_agent_positions`.
3. Program panics with integer overflow (`attempt to multiply with overflow`).

## Expected Behavior

`init_agent_positions` should handle all valid `n` values without panicking (as documented), using wrapping arithmetic consistently.

## Actual Behavior

For `n >= 14`, the expression at `crates/murk-bench/src/lib.rs:145` uses plain `*` on `u64`, which overflows and panics in debug/overflow-checked builds.

## Reproduction Rate

Always (when overflow checks are enabled and `n >= 14`).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):**
- **C compiler (if murk-ffi C header/source):**

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
use murk_bench::init_agent_positions;

fn main() {
    // Panics in debug/overflow-checked builds
    let _ = init_agent_positions(100, 14, 42);
}
```

## Additional Context

Evidence:
- Doc claim of no panic: `crates/murk-bench/src/lib.rs:128`
- Overflowing expression: `crates/murk-bench/src/lib.rs:145`

Root cause:
- The code mixes wrapping ops (`wrapping_mul`, `wrapping_add`) with one plain multiplication: `i as u64 * 1442695040888963407`.

Suggested fix:
- Replace with wrapping multiplication, e.g. `(i as u64).wrapping_mul(1442695040888963407)`, so behavior is consistent and panic-free.