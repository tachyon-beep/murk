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
- [x] murk-replay
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

`decode_frame`, `read_length_prefixed_str`, and `read_length_prefixed_bytes` read u32 lengths from wire input and immediately allocate `Vec` of that size without any upper bound. A crafted replay file can declare a string or blob length up to 4GB (u32::MAX), causing an immediate allocation attempt before `read_exact` validates the data exists. Similarly, `command_count` in `decode_frame` (line 257-258) is used for `Vec::with_capacity` with no cap.

This is a denial-of-service vector (OOM panic/abort), not a data-corruption or determinism issue. Replay files are typically trusted (self-generated), which reduces practical severity.

## Steps to Reproduce

1. Create a binary replay file with a valid header.
2. Write a frame with `command_count = 0xFFFFFFFF` (4 bytes LE).
3. Open with `ReplayReader` and call `next_frame()`.
4. Observe OOM panic/abort from `Vec::with_capacity(4294967295)`.

## Expected Behavior

Should return `ReplayError::MalformedFrame` when declared lengths exceed a reasonable limit.

## Actual Behavior

Attempts to allocate up to 4GB, causing OOM panic or process abort.

## Reproduction Rate

- Deterministic for crafted input.

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
use murk_replay::codec::decode_frame;

// Valid 8-byte tick_id + u32::MAX command count
let mut data = Vec::new();
data.extend_from_slice(&1u64.to_le_bytes()); // tick_id
data.extend_from_slice(&u32::MAX.to_le_bytes()); // command_count = 4 billion

let result = decode_frame(&mut data.as_slice());
// BUG: OOM panic before returning Err
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-replay/src/codec.rs.md
**Verified lines:** codec.rs:114-116 (read_length_prefixed_str), codec.rs:124-126 (read_length_prefixed_bytes), codec.rs:257-258 (decode_frame command_count)
**Root cause:** Codec treats wire-encoded lengths as trusted and uses infallible allocation APIs.
**Suggested fix:** Introduce decode limits (e.g., `MAX_BLOB_LEN = 64MB`, `MAX_STRING_LEN = 1MB`, `MAX_COMMANDS_PER_FRAME = 1_000_000`) and reject larger values with `ReplayError::MalformedFrame`. Alternatively, use `Vec::try_reserve` and map allocation failure to `ReplayError`.
