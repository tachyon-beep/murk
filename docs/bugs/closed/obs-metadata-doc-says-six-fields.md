# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [x] murk-obs
- [ ] murk-replay
- [ ] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

The doc comment on `ObsMetadata` (line 8) says "All six fields are guaranteed to be set," but the struct only defines five fields: `tick_id`, `age_ticks`, `coverage`, `world_generation_id`, `parameter_version`.

## Steps to Reproduce

1. Read `crates/murk-obs/src/metadata.rs` line 8.
2. Count the fields defined on lines 10-24.
3. Observe five fields, not six.

## Expected Behavior

The doc comment should say "All five fields" or a sixth field should exist.

## Actual Behavior

Doc says "six fields", struct has five.

## Reproduction Rate

- Always (doc is static)

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

```rust
// No runtime reproducer -- doc-only issue.
// crates/murk-obs/src/metadata.rs:8 says "All six fields"
// but only 5 fields exist: tick_id, age_ticks, coverage,
// world_generation_id, parameter_version
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-obs/src/metadata.rs.md
**Verified lines:** metadata.rs:8 (doc comment), metadata.rs:10-24 (struct fields)
**Root cause:** Stale doc comment from an earlier version with a different field count.
**Suggested fix:** Change "six" to "five" on line 8.
