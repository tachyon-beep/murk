# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
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

`murk_last_panic_message` cannot distinguish “no panic recorded” from “panic recorded with empty message,” returning `0` for both cases.

## Steps to Reproduce

1. Trigger `ffi_guard!` with an empty panic payload (`panic!("")`) so `LAST_PANIC` becomes `""`.
2. Call `murk_last_panic_message(NULL, 0)`.
3. Observe it returns `0`, identical to the “no panic recorded” path.

## Expected Behavior

After a caught panic, callers should be able to detect that a panic happened, even if the panic message is empty.

## Actual Behavior

`murk_last_panic_message` returns `0` when `LAST_PANIC` is empty (`msg.is_empty()`), which conflates:
- no panic recorded, and
- a recorded panic with empty message.

Evidence:
- Empty check and early return: `/home/john/murk/crates/murk-ffi/src/lib.rs:110`
- `return 0` path: `/home/john/murk/crates/murk-ffi/src/lib.rs:111`
- `ffi_guard!` stores panic message (can be empty string): `/home/john/murk/crates/murk-ffi/src/lib.rs:67`, `/home/john/murk/crates/murk-ffi/src/lib.rs:69`

## Reproduction Rate

Always

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

```text
N/A - found via static analysis
```

## Minimal Reproducer

```rust
#[test]
fn empty_panic_message_is_indistinguishable_from_no_panic() {
    // Simulate a caught panic with empty message
    let _ = ffi_guard!({
        panic!(""); // empty payload
    });

    // API reports 0, same as "no panic recorded"
    let len = murk_last_panic_message(std::ptr::null_mut(), 0);
    assert_eq!(len, 0);
}
```

## Additional Context

Root cause is API contract + implementation mismatch: `0` is used as “no panic recorded,” but an empty panic message is also represented as length `0`. Suggested fix options:
- track a separate boolean “panic occurred” flag in TLS, or
- return a distinct status code for “no panic recorded,” or
- preserve empty panic as non-empty sentinel (e.g., `"<empty panic>"`) when storing.