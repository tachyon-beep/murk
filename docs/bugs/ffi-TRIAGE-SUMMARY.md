# FFI Triage Summary

**Date:** 2026-02-17
**Scope:** murk-ffi crate static analysis reports
**Reports triaged:** 9
**Tickets filed:** 6

---

## Disposition Table

| Source File | Report Verdict | Triage Disposition | Severity | Ticket |
|---|---|---|---|---|
| `lib.rs` | No concrete bug / trivial | **SKIPPED** | -- | -- |
| `status.rs` | No concrete bug / trivial | **SKIPPED** | -- | -- |
| `types.rs` | No concrete bug / trivial | **SKIPPED** | -- | -- |
| `config.rs` | Unchecked f64-to-usize casts in ProductSpace | **CONFIRMED** | High | [ffi-productspace-unchecked-float-cast](ffi-productspace-unchecked-float-cast.md) |
| `handle.rs` | Generation counter wraparound ABA | **CONFIRMED** | Medium | [ffi-handle-generation-wraparound](ffi-handle-generation-wraparound.md) |
| `metrics.rs` | Mutex poisoning panic in extern "C" | **CONFIRMED** (scope expanded) | High | [ffi-mutex-poisoning-panic-in-extern-c](ffi-mutex-poisoning-panic-in-extern-c.md) |
| `obs.rs` | Negative i32-to-unsigned casts | **CONFIRMED** | High | [ffi-obs-negative-to-unsigned-cast](ffi-obs-negative-to-unsigned-cast.md) |
| `propagator.rs` | Null pointer dereference in trampolines | **CONFIRMED** | High | [ffi-trampoline-null-pointer-dereference](ffi-trampoline-null-pointer-dereference.md) |
| `world.rs` | Ambiguous zero return for invalid handles | **CONFIRMED** | Medium | [ffi-accessor-ambiguous-zero-return](ffi-accessor-ambiguous-zero-return.md) |

---

## Summary Statistics

- **CONFIRMED:** 6
- **FALSE_POSITIVE:** 0
- **DESIGN_AS_INTENDED:** 0
- **ALREADY_FIXED:** 0
- **SKIPPED (trivial/no-bug):** 3

### By Severity

- **Critical:** 0
- **High:** 4
- **Medium:** 2
- **Low:** 0

---

## Systemic Findings

### 1. No panic guard on FFI boundary (affects all files)

The most pervasive issue is that **none** of the 30+ `extern "C"` functions in murk-ffi use `std::panic::catch_unwind` or any other panic guard. Combined with `lock().unwrap()` (43+ sites), unchecked `as` casts, and `Vec::with_capacity` on untrusted inputs, there are numerous paths where a panic can escape through the FFI boundary -- which is undefined behavior in Rust.

**Recommendation:** Introduce a macro or wrapper function that wraps every FFI function body in `catch_unwind`, converting caught panics to a defined error status code. This is defense-in-depth that would mitigate all four High-severity bugs simultaneously.

### 2. Unchecked `as` casts at the FFI boundary (config.rs, obs.rs)

Two separate reports identified the same root cause: `f64` and `i32` values from C callers are cast to `usize`/`u32` using `as` without validation. This is a pattern that should be eliminated crate-wide.

**Recommendation:** Create checked conversion helpers (`f64_to_usize`, `i32_to_u32`, etc.) that return `Option` and enforce non-negative, finite, and in-range constraints. Use them consistently at all FFI boundary parse points.

### 3. Missing null-pointer validation on output pointers (propagator.rs, partially elsewhere)

The trampoline functions are the most critical instance, but the pattern of "SAFETY: caller guarantees pointer is valid" without defensive null checks appears elsewhere too. For pointers that originate from external C code (as opposed to internal Rust code), defensive null checks should be mandatory.

---

## Recommended Fix Priority

1. **ffi-mutex-poisoning-panic-in-extern-c** -- Systemic, affects entire crate, most impactful single fix (catch_unwind wrapper)
2. **ffi-trampoline-null-pointer-dereference** -- Direct UB from common C programming mistakes
3. **ffi-productspace-unchecked-float-cast** -- Reachable panic from any C caller
4. **ffi-obs-negative-to-unsigned-cast** -- Reachable panic from any C caller
5. **ffi-accessor-ambiguous-zero-return** -- API quality, no safety impact
6. **ffi-handle-generation-wraparound** -- Theoretical, requires 2^32 cycles per slot
