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

`StaticArena::new` uses unchecked integer accumulation, so large `static_fields` lengths can overflow metadata and cause deterministic slice panics later in `read_field`/`write_field` (`/home/john/murk/crates/murk-arena/src/static_arena.rs:51`, `:58`, `:70`, `:80`).

## Steps to Reproduce

1. Build/run on a 32-bit target in release mode (`--release`) so `usize` arithmetic wraps instead of debug-overflow panicking.
2. Construct:
   `StaticArena::new(&[(FieldId(0), u32::MAX), (FieldId(1), 1)])`.
3. Call `arena.read_field(FieldId(0))` (or `write_field(FieldId(0))`).

## Expected Behavior

Constructor should reject impossible layouts (overflow) with an error/panic at construction time before storing inconsistent offsets, or use checked arithmetic so metadata and backing storage remain consistent.

## Actual Behavior

`total` and `cursor` are accumulated without checks (`/home/john/murk/crates/murk-arena/src/static_arena.rs:51`, `:58`).  
On 32-bit release, `total` wraps (e.g., to `0`), `data` is too small, but field metadata still records huge lengths; later `read_field`/`write_field` slice with out-of-bounds end index and panic (`/home/john/murk/crates/murk-arena/src/static_arena.rs:70`, `:80`).

## Reproduction Rate

Always (given overflow-triggering input; panic point differs by build mode).

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
use murk_arena::StaticArena;
use murk_core::FieldId;

// Reproduce on 32-bit target, release build.
fn main() {
    let arena = StaticArena::new(&[(FieldId(0), u32::MAX), (FieldId(1), 1)]);
    let _ = arena.read_field(FieldId(0)); // panics: range end out of bounds
}
```

## Additional Context

Root cause is unchecked arithmetic in layout construction and slice-end computation:
- `sum()` into `usize` without overflow guard: `/home/john/murk/crates/murk-arena/src/static_arena.rs:51`
- `cursor += len as usize` without `checked_add`: `/home/john/murk/crates/murk-arena/src/static_arena.rs:58`
- `offset + len` used directly for slicing: `/home/john/murk/crates/murk-arena/src/static_arena.rs:70`, `:80`

Suggested fix:
- Use `checked_add` for both total and cursor accumulation.
- Validate `offset.checked_add(len)` before slicing.
- Fail fast in `new` when layout cannot be represented safely.