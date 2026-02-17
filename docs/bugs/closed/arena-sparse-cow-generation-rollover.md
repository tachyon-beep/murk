# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [x] murk-arena
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

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

Sparse CoW `write_sparse` uses `h.generation() < self.generation` to decide whether to copy previous data, which silently skips the copy after generation counter rollover, causing field data loss.

## Steps to Reproduce

1. Create a `PingPongArena` with a sparse field.
2. Write data to the sparse field at generation N.
3. Advance the arena past `u32::MAX` so generation wraps to 0.
4. Write to the sparse field again.
5. The `h.generation() < self.generation` check at write.rs:83 evaluates to `false` (old generation N > current generation 0), so the copy block is skipped.
6. The new sparse allocation contains only zeros; the previously written data is lost.

## Expected Behavior

When a sparse field is written in a new generation, the previous data should always be copied to the new allocation before the caller's write, regardless of generation counter arithmetic.

## Actual Behavior

At write.rs:83, the filter `.filter(|h| h.generation() < self.generation)` uses a non-wrapping less-than comparison. After generation rollover (u32::MAX -> 0), old handles have generation > current generation, so the filter rejects them and the copy-before-write block (write.rs:85-107) is skipped entirely. The new allocation retains its zero-initialization, silently discarding all previous sparse field data.

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
// The generation comparison at write.rs:83:
//   .filter(|h| h.generation() < self.generation)
//
// After u32 rollover:
//   h.generation() = 4_294_967_295 (u32::MAX, from last write)
//   self.generation = 0 (wrapped)
//   4_294_967_295 < 0 == false  --> copy skipped, data lost
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-arena/src/write.rs.md
**Verified lines:** write.rs:83 (`h.generation() < self.generation`, non-wrapping comparison), write.rs:85-107 (copy block that gets skipped), write.rs:111 (descriptor updated to new handle regardless), write.rs:170 (equality check in FieldWriter::write also vulnerable)
**Root cause:** The `<` comparison assumes generation is strictly monotonic in u32 space, but u32 wraps. This is a downstream consequence of the generation overflow bug (arena-generation-counter-overflow.md) but represents a separate fix location.
**Suggested fix:** Replace `h.generation() < self.generation` with `h.generation() != self.generation` at write.rs:83. The `!=` check correctly triggers the copy whenever the handle's generation differs from current, regardless of arithmetic ordering. This is a local fix; the upstream generation overflow should also be addressed.
