# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [x] Critical | [ ] High | [ ] Medium | [ ] Low

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

`ScratchRegion::alloc` can panic on large `len` (capacity overflow in `Vec::resize`) instead of returning `None`, violating its fallible API contract.

## Steps to Reproduce

1. Create a scratch region: `let mut s = ScratchRegion::new(0);`
2. Call `s.alloc(usize::MAX)`.
3. Observe panic (capacity overflow) instead of `None`.

## Expected Behavior

`alloc` should return `None` for unfulfillable allocation requests (including size/capacity overflow), without panicking.

## Actual Behavior

`alloc` computes `new_cursor` successfully, then calls `self.data.resize(new_cap, 0.0)` and panics for oversized element counts.  
Evidence: `/home/john/murk/crates/murk-arena/src/scratch.rs:55` (`resize`), with growth math at `/home/john/murk/crates/murk-arena/src/scratch.rs:48-54`.

## Reproduction Rate

Always (for sufficiently large `len`, e.g. `usize::MAX`).

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

```text
N/A - found via static analysis
```

## Minimal Reproducer

```rust
use murk_arena::scratch::ScratchRegion;

fn main() {
    let mut s = ScratchRegion::new(0);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = s.alloc(usize::MAX); // panics in Vec::resize
    }));
    assert!(r.is_err()); // got panic, not None
}
```

## Additional Context

Root cause: `alloc` guards `cursor + len` with `checked_add`, but does not guard `Vec` element-count/byte-size limits before `resize`. For extreme `len`, this triggers panic path in `Vec` internals.  
Suggested fix: add explicit maximum-element bound checks (based on `isize::MAX / size_of::<f32>()`) and use fallible growth (`try_reserve`) so all growth failures map to `None` consistently.