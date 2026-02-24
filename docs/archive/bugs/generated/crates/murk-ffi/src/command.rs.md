# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

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

`convert_receipt` silently truncates `Receipt.command_index` from `usize` to `u32`, which can return an incorrect command index to C callers.

## Steps to Reproduce

1. Construct a `murk_core::command::Receipt` with `command_index = (u32::MAX as usize) + 1`.
2. Call `convert_receipt(&receipt)` in `murk-ffi`.
3. Observe `MurkReceipt.command_index` becomes `0` (wrapped/truncated), not the original index.

## Expected Behavior

`command_index` should be preserved exactly across the FFI boundary, or overflow should be detected and surfaced as an error.

## Actual Behavior

`command_index` is narrowed with `as u32`, causing silent truncation for values above `u32::MAX`.

## Reproduction Rate

Always

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** Any

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
use murk_core::command::Receipt;
use murk_core::id::TickId;
use murk_ffi::command::convert_receipt; // crate-private in current code; place in crate test

#[test]
fn command_index_truncates() {
    let r = Receipt {
        accepted: true,
        applied_tick_id: Some(TickId(1)),
        reason_code: None,
        command_index: (u32::MAX as usize) + 1,
    };
    let c = convert_receipt(&r);
    assert_ne!(c.command_index as usize, r.command_index); // truncates to 0
}
```

## Additional Context

Evidence:
- `crates/murk-ffi/src/command.rs:126` performs `command_index: r.command_index as u32`.
- `crates/murk-core/src/command.rs:162` defines `Receipt.command_index` as `usize`.

Root cause:
- Lossy cast from wider type (`usize`) to narrower FFI field (`u32`) without bounds checking.

Suggested fix:
- Use checked conversion (`u32::try_from(r.command_index)`) and propagate an explicit overflow error path, or widen `MurkReceipt.command_index` to `u64` in the FFI ABI.