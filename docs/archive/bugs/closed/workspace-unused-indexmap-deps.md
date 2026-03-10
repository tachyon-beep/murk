# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [x] murk-core
- [x] murk-replay

## Engine Mode

- [x] Both / Unknown

## Summary

`indexmap` is listed as a dependency in both `murk-core/Cargo.toml` and `murk-replay/Cargo.toml` but is not imported or used in any source file in either crate. This adds unnecessary compilation time and dependency tree weight.

## Expected Behavior

Unused dependencies removed from `[dependencies]`.

## Actual Behavior

`indexmap` compiled but never used.

## Additional Context

**Source:** murk-core audit AR-2, murk-replay audit Finding 3.4
**Files:** `crates/murk-core/Cargo.toml`, `crates/murk-replay/Cargo.toml`
**Suggested fix:** Remove `indexmap` from both `Cargo.toml` files. Verify no transitive use via `cargo udeps`.
