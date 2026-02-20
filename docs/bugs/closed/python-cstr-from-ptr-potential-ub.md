# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-python

## Engine Mode

- [x] Both / Unknown

## Summary

In `metrics.rs:89`, `CStr::from_ptr(name_buf.as_ptr() as *const c_char)` reads from a 256-byte stack buffer. If the FFI function writes exactly 256 bytes with no null terminator, `CStr::from_ptr` reads past the buffer boundary -- undefined behavior.

The buffer is zero-initialized (line 79), so this is safe as long as the FFI writes fewer than 256 bytes. However, the reliance on zero-initialized trailing bytes as the null terminator is fragile.

## Expected Behavior

Use `CStr::from_bytes_until_nul(&name_buf)` (stable since Rust 1.69), which is safe and bounded.

## Actual Behavior

`CStr::from_ptr` which is unsafe and could read past the buffer.

## Additional Context

**Source:** murk-python audit, F-24
**File:** `crates/murk-python/src/metrics.rs:89`
**Suggested fix:**
```rust
let name = CStr::from_bytes_until_nul(&name_buf)
    .map(|c| c.to_string_lossy().into_owned())
    .unwrap_or_else(|_| "<unknown>".to_string());
```
