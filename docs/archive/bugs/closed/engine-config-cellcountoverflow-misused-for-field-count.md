# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`WorldConfig::validate()` returns `ConfigError::CellCountOverflow` for a field-count overflow, but the `Display` impl always renders "cell count {value} exceeds u32::MAX", producing an incorrect diagnostic message when the actual overflow is from field count.

## Steps to Reproduce

1. Trigger field-count overflow in `WorldConfig::validate()` (config with more than `u32::MAX` fields, or observe the error variant in isolation).
2. Format the error with `Display`.
3. Observe the message says "cell count" when the actual overflow is from field count.

## Expected Behavior

A field-count overflow should produce a message that mentions "field count", not "cell count".

## Actual Behavior

`config.rs:246-249` returns `ConfigError::CellCountOverflow { value: self.fields.len() }` for the field-count check. The `Display` impl at `config.rs:157-158` renders this as `"cell count {value} exceeds u32::MAX"`, which is misleading. The doc comment at `config.rs:126` acknowledges the dual use ("Cell count or field count exceeds u32::MAX") but the Display format hardcodes "cell count".

## Reproduction Rate

Always (when the field-count overflow path is reached)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** 0.1.8 / HEAD (feat/release-0.1.9)

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
use murk_engine::config::ConfigError;

let err = ConfigError::CellCountOverflow { value: 4_294_967_296usize };
assert_eq!(err.to_string(), "cell count 4294967296 exceeds u32::MAX");
// Message says "cell count" but this may actually be a field-count overflow
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-engine/src/config.rs.md`

**Affected lines:**
- Variant definition: `crates/murk-engine/src/config.rs:126-130`
- Display impl: `crates/murk-engine/src/config.rs:157-158`
- Cell count check (correct use): `crates/murk-engine/src/config.rs:242-243`
- Field count check (incorrect reuse): `crates/murk-engine/src/config.rs:246-249`

**Root cause:** The `CellCountOverflow` variant is reused for field-count overflow with no discriminator, and the `Display` impl hardcodes "cell count".

**Suggested fix:** Add a `FieldCountOverflow` variant (or add a `kind: &'static str` discriminator field to `CellCountOverflow`), and update the Display impl accordingly.
