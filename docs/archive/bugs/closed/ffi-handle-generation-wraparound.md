# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
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
- [x] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`HandleTable::remove()` increments the slot generation counter using `wrapping_add(1)` (line 92 of `handle.rs`). After `u32::MAX` (4,294,967,295) remove/insert cycles on the same slot, the generation wraps from `u32::MAX` back to `0`, causing the slot to emit handles with generations that collide with previously-issued stale handles. `HandleTable::get()` and `get_mut()` only check generation equality (line 63), so a stale handle from a prior epoch would pass validation and access a completely different object now occupying that slot -- an ABA problem.

While 2^32 cycles per slot is infeasible in most interactive workloads, long-running RL training loops (the primary use case) that repeatedly create/destroy worlds or configs could approach this over extended runs, especially on hot slots at the front of the free list.

## Steps to Reproduce

```rust
let mut table = HandleTable::new();
// Insert and remove on the same slot 2^32 times
let h_stale = table.insert(1);
table.remove(h_stale);
for _ in 0..(u32::MAX as u64) {
    let h = table.insert(999);
    table.remove(h);
}
// Now insert a new value -- generation has wrapped back to match h_stale
let h_new = table.insert(2);
// h_stale now resolves to the object behind h_new (ABA)
assert_eq!(table.get(h_stale), Some(&2)); // should be None!
```

## Expected Behavior

Stale handles should always return `None` regardless of how many insert/remove cycles have occurred on the slot.

## Actual Behavior

After 2^32 remove cycles on a single slot, generation wraps and stale handles from prior epochs become silently valid again, returning references to unrelated objects.

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
// See Steps to Reproduce above.
// Requires 2^32 iterations so not practical as a unit test without
// narrowing the generation counter (e.g., to u8 for testing).
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-ffi/src/handle.rs.md
**Verified lines:** handle.rs:19, handle.rs:42-55, handle.rs:60-67, handle.rs:85-95
**Root cause:** `wrapping_add(1)` on a 32-bit generation counter with unconditional free-list reuse allows generation collision after 2^32 cycles.
**Fix applied:** Option 1 — `remove()` now checks if generation wrapped to 0 after incrementing. If so, the slot is permanently retired (not pushed to `free_list`). This sacrifices ~32 bytes per exhausted slot to guarantee no ABA. Test `generation_exhaustion_retires_slot` validates the fix by fast-forwarding a slot's generation to `u32::MAX` and verifying the slot is not recycled after the final remove.
**Status:** Fixed.
