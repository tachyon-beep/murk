# Bug Report

**Date:** 2026-02-24
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

`write_receipts()` in `world.rs` reports a non-zero `*n_receipts_out` count even when the `receipts_out` buffer pointer is null, misleading callers into believing receipts were written when no data was actually stored.

## Steps to Reproduce

1. Create a valid lockstep world with at least one propagator and field.
2. Call `murk_lockstep_step(world, &cmd, 1, NULL, 4, &n_receipts, NULL)` with:
   - `receipts_out = NULL` (no output buffer)
   - `receipts_cap = 4` (non-zero capacity)
   - `n_receipts_out = &n_receipts` (valid output count pointer)
3. Observe that `n_receipts` is set to a non-zero value (e.g. 1) even though no receipts could have been written.

## Expected Behavior

Either:
1. If `receipts_cap > 0 && receipts_out == NULL`, return `MURK_ERROR_INVALID_ARGUMENT` (reject the inconsistent argument combination), or
2. Set `*n_receipts_out = 0` when `receipts_out` is null, since zero receipts were actually written.

## Actual Behavior

`write_receipts` computes `write_count = receipts.len().min(cap)` and writes that count to `*n_out` unconditionally. When `out` is null, the actual data copy is correctly skipped (line 504), but the count is still reported (line 515). This means:
- `*n_receipts_out` can be 1 or more
- Zero bytes were actually written to the null buffer
- The caller sees "1 receipt written" with no way to read it

## Reproduction Rate

Always (deterministic).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.9)

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
// In murk-ffi world tests (can reuse create_test_world()).
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
    4,                    // receipts_cap = 4 (non-zero)
    &mut n_receipts,
    std::ptr::null_mut(),
);

assert_eq!(status, MurkStatus::Ok as i32);
// BUG: n_receipts == 1, but no buffer existed to receive data
assert_eq!(n_receipts, 0, "should be 0 since receipts_out was null");
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-ffi/src/world.rs.md`

**Affected lines in `crates/murk-ffi/src/world.rs`:**
- Line 497-518: `write_receipts()` helper function
- Line 503: `let write_count = receipts.len().min(cap);` -- computes count from `cap`, ignoring null `out`
- Line 504: `if !out.is_null() && write_count > 0` -- correctly skips write when null
- Line 512-515: `if !n_out.is_null() { *n_out = write_count; }` -- unconditionally reports `write_count`
- Called from line 164 (`murk_lockstep_step` success path) and line 182 (error/rollback path)

**Root cause:** `write_count` is computed from `receipts.len().min(cap)` without considering whether `out` is null. The null check on `out` (line 504) only guards the actual copy, not the count report.

**Suggested fix:** Either:
1. **Validate at call site:** In `murk_lockstep_step`, if `receipts_cap > 0 && receipts_out.is_null()`, return `InvalidArgument`. This is the strictest contract and prevents the ambiguity entirely.
2. **Fix write_receipts:** Set `write_count = 0` when `out.is_null()`:
   ```rust
   let write_count = if out.is_null() {
       0
   } else {
       receipts.len().min(cap)
   };
   ```

Option 1 is preferred because it catches a likely caller bug (passing non-zero capacity with a null pointer).
