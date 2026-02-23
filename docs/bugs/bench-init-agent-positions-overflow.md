# Bug Report

**Date:** 2026-02-24
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

`init_agent_positions` uses plain `*` instead of `wrapping_mul` in its hash computation, causing integer overflow panic in debug/overflow-checked builds when `i >= 14`.

## Steps to Reproduce

1. Call `init_agent_positions(100, 14, 42)` in a debug or overflow-checked build.
2. The loop reaches `i = 13` (0-indexed), computing `13u64 * 1442695040888963407`.
3. Program panics with `attempt to multiply with overflow`.

## Expected Behavior

`init_agent_positions` should handle all valid `n` values (up to `u16::MAX`) without panicking, consistent with its documentation which promises "no panic, no infinite loop" (`crates/murk-bench/src/lib.rs:129`). The hash computation should use wrapping arithmetic throughout.

## Actual Behavior

The expression `i as u64 * 1442695040888963407` at `crates/murk-bench/src/lib.rs:145` uses plain `*` on `u64`, which overflows for `i >= 14` (since `14 * 1442695040888963407 > u64::MAX`). This panics in debug builds and any build with `overflow-checks = true`.

The surrounding operations (`wrapping_mul` on line 144, `wrapping_add` on line 145) correctly use wrapping arithmetic, but the inner `i as u64 * 1442695040888963407` does not.

## Reproduction Rate

Always (when overflow checks are enabled and `n >= 14`).

## Environment

- **OS:** Any
- **Rust toolchain:** stable (debug or overflow-checked builds)
- **Murk version/commit:** HEAD (feat/release-0.1.9)

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
use murk_bench::init_agent_positions;

fn main() {
    // Panics in debug/overflow-checked builds:
    // 13u64 * 1442695040888963407 overflows u64
    let _ = init_agent_positions(100, 14, 42);
}
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-bench/src/lib.rs.md`

**Affected lines:**
- Doc claim of no panic: `crates/murk-bench/src/lib.rs:129`
- Overflowing expression: `crates/murk-bench/src/lib.rs:145`

**Root cause:** The hash computation mixes wrapping and non-wrapping arithmetic. The seed is hashed via `wrapping_mul` (line 144) and the agent index offset is combined via `wrapping_add` (line 145), but the agent index hash itself (`i as u64 * 1442695040888963407`) uses plain multiplication which panics on overflow.

**Suggested fix:** Replace the plain `*` with `wrapping_mul`:

```rust
// Before (line 145):
.wrapping_add(i as u64 * 1442695040888963407))

// After:
.wrapping_add((i as u64).wrapping_mul(1442695040888963407)))
```

This makes the hash computation consistent (all wrapping) and honours the "no panic" documentation contract.
