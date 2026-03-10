# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-ffi

## Engine Mode

- [x] Both / Unknown

## Summary

`MurkStepMetrics` (metrics.rs:18) contains `memory_bytes: usize` in a `#[repr(C)]` struct. `usize` is 4 bytes on 32-bit, 8 bytes on 64-bit. If the C header uses `uint64_t`, it mismatches on 32-bit targets. Same issue with `cell_count: usize` in `MurkStepContext` (propagator.rs:48).

Additionally, `MurkCommand` has mixed-size fields with no compile-time size assertion to catch ABI mismatches across platforms/compilers.

## Expected Behavior

Use `u64` for FFI struct fields (converting from `usize` internally), and add `const _: () = assert!(std::mem::size_of::<MurkCommand>() == EXPECTED);` compile-time checks.

## Actual Behavior

`usize` crosses the FFI boundary; no layout assertions.

## Additional Context

**Source:** murk-ffi audit, F-03/F-05
**Files:** `crates/murk-ffi/src/metrics.rs:18`, `crates/murk-ffi/src/propagator.rs:48`, `crates/murk-ffi/src/command.rs:26-51`
