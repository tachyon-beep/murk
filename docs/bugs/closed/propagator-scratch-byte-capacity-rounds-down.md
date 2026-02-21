# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [ ] murk-core
- [x] murk-engine
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

- [x] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`ScratchRegion::with_byte_capacity()` converts bytes to f32 slots using floor division (`bytes / size_of::<f32>()`), which silently truncates non-aligned byte counts. A propagator requesting `scratch_bytes() = 5` gets only 4 bytes of backing storage (1 f32 slot) instead of 8 bytes (2 f32 slots, which would accommodate 5 bytes). While the doc comment says "rounded down," the `Propagator::scratch_bytes()` API contract says "Scratch memory required in bytes" with no alignment caveat.

## Steps to Reproduce

1. Implement a propagator with `scratch_bytes() -> usize { 5 }`.
2. Build a `TickEngine` with this propagator.
3. Inside `step()`, attempt to `ctx.scratch().alloc(2)` (2 f32 slots = 8 bytes).
4. Observe that `alloc()` returns `None` because only 1 slot (4 bytes) was allocated.

## Expected Behavior

`with_byte_capacity(5)` should allocate at least 2 f32 slots (8 bytes) to fully contain the 5 requested bytes.

## Actual Behavior

`with_byte_capacity(5)` allocates `5 / 4 = 1` f32 slot (4 bytes), under-allocating by 1 byte relative to the request.

## Reproduction Rate

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

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
use murk_propagator::scratch::ScratchRegion;

let s = ScratchRegion::with_byte_capacity(5);
assert_eq!(s.capacity(), 1); // Only 1 f32 slot = 4 bytes, not enough for 5 bytes

// Expected: capacity() == 2 (8 bytes, sufficient for 5 requested)
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-propagator/src/scratch.rs.md
**Verified lines:** scratch.rs:25-27 (floor division), propagator.rs:104 (scratch_bytes in bytes), tick.rs:142 (with_byte_capacity(max_scratch))
**Root cause:** The byte-to-slot conversion uses floor division instead of ceiling division. The `with_byte_capacity` doc says "rounded down" but this contradicts the `scratch_bytes()` contract which implies byte-exact allocation.
**Suggested fix:** Change line 27 to ceiling division: `Self::new((bytes + std::mem::size_of::<f32>() - 1) / std::mem::size_of::<f32>())`. Optionally add a debug assertion in `scratch_bytes()` default or validation that the value is a multiple of 4.
**Mitigating factors:** All current propagators return `scratch_bytes() = 0` (the default). This is a latent bug that would only surface when a custom propagator requests a non-f32-aligned scratch size.
