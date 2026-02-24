# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
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
- [x] Both / Unknown

## Summary

The trampoline functions `trampoline_read` (line 150), `trampoline_read_previous` (line 169), and `trampoline_write` (line 188) in `propagator.rs` dereference `out_ptr` and `out_len` raw pointers unconditionally on the success path (lines 160-161, 179-180, 198-199) without null checks. These trampolines are exposed to C code via function pointers in `MurkStepContext` and are called by user-written C propagator callbacks. If a C callback passes null for `out_ptr` or `out_len`, the dereference is undefined behavior (null pointer write).

The trampolines already return `MurkStatus::InvalidArgument` for invalid field IDs, so they have an error return path -- they simply fail to validate the output pointers before using them.

## Steps to Reproduce

```c
// In a C propagator step function:
int my_step(void* user_data, const MurkStepContext* ctx) {
    // Bug: pass NULL for out_ptr
    const float* ptr = NULL;
    size_t len = 0;
    int rc = ctx->read_fn(ctx->opaque, 0, NULL, &len); // UB: null out_ptr
    return rc;
}
```

## Expected Behavior

Trampoline functions should check `out_ptr` and `out_len` for null before dereferencing and return `MurkStatus::InvalidArgument` if either is null.

## Actual Behavior

Null `out_ptr` or `out_len` causes undefined behavior (null pointer dereference in unsafe code). On most platforms this is a segfault/SIGSEGV that crashes the process.

## Reproduction Rate

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

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

```c
// C propagator callback that triggers the bug:
int bad_step(void* ud, const MurkStepContext* ctx) {
    // Passing NULL for out_ptr -- triggers null dereference in trampoline_read
    size_t len;
    return ctx->read_fn(ctx->opaque, 0, NULL, &len);
}
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-ffi/src/propagator.rs.md
**Verified lines:** propagator.rs:149-166 (trampoline_read), propagator.rs:168-185 (trampoline_read_previous), propagator.rs:187-204 (trampoline_write)
**Root cause:** Missing null-pointer validation on `out_ptr`, `out_len`, and `opaque` parameters in all three trampoline functions before dereferencing.
**Suggested fix:**
1. Add null checks at the top of each trampoline:
   ```rust
   if opaque.is_null() || out_ptr.is_null() || out_len.is_null() {
       return MurkStatus::InvalidArgument as i32;
   }
   ```
2. On the error path, optionally zero-initialize outputs before returning to prevent stale values.
3. Add unit tests that invoke trampolines with null output pointers and assert `InvalidArgument` is returned.
