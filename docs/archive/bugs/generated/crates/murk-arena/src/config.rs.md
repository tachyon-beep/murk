# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
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

`ArenaConfig::segment_bytes()` can overflow on 32-bit targets and return a wrapped (too-small) size, causing incorrect allocation/bounds sizing (`/home/john/murk/crates/murk-arena/src/config.rs:68`).

## Steps to Reproduce

1. Build/run on a 32-bit target (for example `i686-unknown-linux-gnu`).
2. Create `ArenaConfig` with `segment_size >= 1_073_741_824` (for example `u32::MAX`).
3. Call `segment_bytes()` and compare with a widened (`u64`) multiplication.

## Expected Behavior

`segment_bytes()` should either return the mathematically correct byte count or fail safely (for example via checked arithmetic / validation error).

## Actual Behavior

`segment_bytes()` performs unchecked multiplication into `usize`; on 32-bit targets the result wraps and returns a smaller value than required.

## Reproduction Rate

Always (on 32-bit targets with large `segment_size`).

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
#[cfg(target_pointer_width = "32")]
#[test]
fn segment_bytes_overflow_repro() {
    use murk_arena::config::ArenaConfig;

    let cfg = ArenaConfig {
        segment_size: u32::MAX,
        max_segments: 1,
        max_generation_age: 1,
        cell_count: 1,
    };

    let got = cfg.segment_bytes();
    let expected_u64 = (cfg.segment_size as u64) * (std::mem::size_of::<f32>() as u64);

    // Wrapped value differs from true product on 32-bit.
    assert_ne!(got as u64, expected_u64);
}
```

## Additional Context

Evidence:
- Unchecked multiplication and `usize` return in `/home/john/murk/crates/murk-arena/src/config.rs:68`.
- On 32-bit, `usize` cannot represent large products from `u32 * 4` once `segment_size >= 2^30`.

Root cause hypothesis:
- Missing checked/widened arithmetic in size computation.

Suggested fix:
- Compute in `u64` with `checked_mul`, then `usize::try_from(...)`, and fail/validate when out of range.
