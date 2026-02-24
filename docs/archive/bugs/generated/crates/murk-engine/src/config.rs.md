# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [ ] murk-core
- [x] murk-engine
- [ ] murk-arena
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

`WorldConfig::validate()` reports field-count overflow using `ConfigError::CellCountOverflow`, whose `Display` text always says "cell count", producing incorrect diagnostics for field-count failures.

## Steps to Reproduce

1. See field-count overflow branch in `/home/john/murk/crates/murk-engine/src/config.rs:245` and `/home/john/murk/crates/murk-engine/src/config.rs:246`–`/home/john/murk/crates/murk-engine/src/config.rs:249`.
2. Note it returns `ConfigError::CellCountOverflow` for a field-count check.
3. See `Display` for that variant in `/home/john/murk/crates/murk-engine/src/config.rs:157`–`/home/john/murk/crates/murk-engine/src/config.rs:159`, which always renders `"cell count {value} exceeds u32::MAX"`.

## Expected Behavior

Field-count overflow should produce a field-specific error (or at least a message that does not mislabel it as cell-count overflow).

## Actual Behavior

Field-count overflow is surfaced as `CellCountOverflow` and formatted as a cell-count error, which is misleading.

## Reproduction Rate

Always (when the field-count overflow path is reached).

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
use murk_engine::config::ConfigError;

fn main() {
    // Variant used by both cell-count and field-count overflow paths.
    let err = ConfigError::CellCountOverflow { value: 4_294_967_296usize };
    assert_eq!(err.to_string(), "cell count 4294967296 exceeds u32::MAX");
    // Message is incorrect for field-count overflow.
}
```

## Additional Context

Evidence:
- Field-count check and return path: `/home/john/murk/crates/murk-engine/src/config.rs:245`–`/home/john/murk/crates/murk-engine/src/config.rs:249`
- Variant docs indicate dual use ("Cell count or field count"): `/home/john/murk/crates/murk-engine/src/config.rs:126`
- `Display` hardcodes "cell count": `/home/john/murk/crates/murk-engine/src/config.rs:157`–`/home/john/murk/crates/murk-engine/src/config.rs:159`

Suggested fix:
- Add a distinct `FieldCountOverflow` variant, or
- Add a discriminator (`kind: Cell|Field`) to `CellCountOverflow`, and update `Display` accordingly.