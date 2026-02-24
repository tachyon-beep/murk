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

- [x] Lockstep
- [ ] RealtimeAsync
- [ ] Both / Unknown

## Summary

`murk_lockstep_step` can report receipts as written even when `receipts_out == NULL`, producing incorrect FFI results instead of rejecting invalid arguments.

## Steps to Reproduce

1. Create a valid lockstep world handle.
2. Call `murk_lockstep_step(...)` with at least one command, `receipts_out = NULL`, `receipts_cap > 0`, and `n_receipts_out` non-null.
3. Observe success status and non-zero `*n_receipts_out` even though no receipt buffer existed to receive data.

## Expected Behavior

If `receipts_cap > 0` and `receipts_out` is null, the call should fail with `InvalidArgument` (or at minimum report `n_receipts_out = 0` since nothing was written).

## Actual Behavior

`write_receipts` computes `write_count = min(receipts.len(), cap)` and writes that count to `n_out` unconditionally, but skips actual writes when `out` is null. This reports receipts as written when none were written.

Evidence:
- `crates/murk-ffi/src/world.rs:503`
- `crates/murk-ffi/src/world.rs:504`
- `crates/murk-ffi/src/world.rs:512`
- `crates/murk-ffi/src/world.rs:515`
- Called from `crates/murk-ffi/src/world.rs:164` and `crates/murk-ffi/src/world.rs:182`

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
// Inside murk-ffi world tests (can reuse create_test_world()).
let world_h = create_test_world();

let cmd = MurkCommand {
    command_type: MurkCommandType::SetField as i32,
    expires_after_tick: 100,
    source_id: 0,
    source_seq: 0,
    priority_class: 1,
    field_id: 0,
    param_key: 0,
    float_value: 1.0,
    double_value: 0.0,
    coord: [0; 4],
    coord_ndim: 1,
};

let mut n_receipts: usize = 0;
let status = murk_lockstep_step(
    world_h,
    &cmd,
    1,
    std::ptr::null_mut(), // receipts_out = NULL
    4,                    // receipts_cap > 0
    &mut n_receipts,
    std::ptr::null_mut(),
);

assert_eq!(status, MurkStatus::Ok as i32);
assert!(n_receipts > 0); // claims receipts written, but none could be written
```

## Additional Context

Root cause is inconsistent pointer/capacity handling in `write_receipts`: count is reported independently of whether output storage exists. Suggested fix:
1. Validate in `murk_lockstep_step`: if `receipts_cap > 0 && receipts_out.is_null()`, return `InvalidArgument`.
2. Keep `n_receipts_out` semantics consistent with actual writes (or explicitly change contract and return total-required count with a distinct status for truncation).