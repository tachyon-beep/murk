# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

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

No concrete bug found in /home/john/murk/crates/murk-arena/src/read.rs.

## Steps to Reproduce

1. Inspect `Snapshot::resolve_field` in `/home/john/murk/crates/murk-arena/src/read.rs:69`.
2. Inspect `OwnedSnapshot::resolve_field` in `/home/john/murk/crates/murk-arena/src/read.rs:166`.
3. Cross-check called bounds-checked readers in `/home/john/murk/crates/murk-arena/src/segment.rs:55` and `/home/john/murk/crates/murk-arena/src/static_arena.rs:68`.

## Expected Behavior

`read`/`read_field` return valid slices for existing fields and `None` for invalid/unresolvable handles without panic/UB.

## Actual Behavior

Observed code paths are consistent with expected behavior; no concrete panic/UB/incorrect-result bug was found in the target file.

## Reproduction Rate

N/A (no bug reproduced)

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

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// N/A - no concrete bug found in the audited file.
```

## Additional Context

Evidence reviewed in target file:
- `/home/john/murk/crates/murk-arena/src/read.rs:69` (descriptor lookup uses `?`, graceful `None` on missing field)
- `/home/john/murk/crates/murk-arena/src/read.rs:73` (location dispatch, no unsafe path)
- `/home/john/murk/crates/murk-arena/src/read.rs:76` and `/home/john/murk/crates/murk-arena/src/read.rs:80` (delegates to checked `SegmentList::slice`)
- `/home/john/murk/crates/murk-arena/src/read.rs:166` (owned snapshot mirror logic)
- `/home/john/murk/crates/murk-arena/src/read.rs:179` (static lookup through `StaticArena::read_field`, returns `Option`)