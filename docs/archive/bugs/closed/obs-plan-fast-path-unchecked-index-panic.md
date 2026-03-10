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

The fast-path code in `execute_agent_entry_direct` (line 970) and `execute_agent_entry_pooled` (line 1018) uses unchecked indexing `field_data[field_idx]` where `field_idx = (base_rank + op.stride_offset) as usize`. If `field_data` is shorter than the Space's canonical cell count (e.g., a malformed or partially-loaded snapshot), this causes an out-of-bounds panic. The corresponding slow paths (lines 984, 1028) guard with `idx < field_data.len()` and skip gracefully.

The fast path is gated on `is_interior` returning `true`, which guarantees the coordinates are within grid bounds. Under normal operation this implies `field_idx < field_data.len()` because field buffers should match the space's cell count. However, if a snapshot has a short field buffer (e.g., due to arena corruption or partial snapshot), the fast path panics while the slow path would gracefully skip.

## Steps to Reproduce

1. Create a 2D grid Space (e.g., `Square4::new(10, 10, Absorb)` with 100 cells).
2. Compile an `ObsPlan` with an agent-relative entry (e.g., `AgentDisk { radius: 2 }`).
3. Provide a snapshot where a field buffer has fewer than 100 elements (e.g., 50).
4. Call `execute_agents` with an agent at an interior position (e.g., `[5, 5]`).
5. Observe: fast path is selected (interior check passes), then `field_data[field_idx]` panics for idx >= 50.

## Expected Behavior

Should return `ObsError::ExecutionFailed` (or equivalent) instead of panicking.

## Actual Behavior

Panics with `index out of bounds: the len is 50 but the index is XX`.

## Reproduction Rate

- Deterministic for any short field buffer with an interior agent.

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
// Requires constructing a snapshot with a short field buffer
// and executing an ObsPlan with an interior agent.
// The fast path at plan.rs:970 panics on:
//   field_data[field_idx]
// where field_idx is valid for the grid but exceeds field_data.len().
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-obs/src/plan.rs.md
**Verified lines:** plan.rs:970 (direct fast-path unchecked index), plan.rs:1018 (pooled fast-path unchecked index), plan.rs:984 (direct slow-path bounds check), plan.rs:1028 (pooled slow-path bounds check)
**Root cause:** Fast path assumes `field_data.len() == space.cell_count()` invariant but does not verify it. Slow path defensively checks bounds.
**Suggested fix:** Either (a) add a pre-check `assert_eq!(field_data.len(), space.cell_count())` at entry to `execute_agents` with a clear error, or (b) use `field_data.get(field_idx)` in the fast path with graceful fallback. Option (a) is preferred since it fails fast with a clear message and avoids per-cell overhead.
