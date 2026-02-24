# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [x] murk-replay

## Engine Mode

- [x] Both / Unknown

## Summary

`ReplayWriter` wraps a `W: Write` and provides `flush()`, but does not implement `Drop` to flush automatically. If the caller forgets to call `flush()` or `into_inner()` before dropping, buffered data in a `BufWriter<File>` is silently lost.

## Expected Behavior

Implement `Drop for ReplayWriter<W>` that calls `let _ = self.writer.flush();`, following the pattern of `csv::Writer` and `io::BufWriter`.

## Actual Behavior

No flush on drop; buffered replay data silently lost.

## Additional Context

**Source:** murk-replay audit, Finding 5.1
**File:** `crates/murk-replay/src/writer.rs:59-122`
