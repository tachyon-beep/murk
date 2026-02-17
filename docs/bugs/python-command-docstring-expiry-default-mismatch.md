# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
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
- [ ] murk-ffi
- [x] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`Command.set_field` docstring says `expires_after_tick` has "default 0 = never" but the actual Python default is `u64::MAX`. The same applies to `Command.set_parameter`. A user who reads the docs and explicitly passes `0` expecting "never expires" will get a command that expires immediately after tick 0.

## Steps to Reproduce

1. Read the `set_field` docstring which says "default 0 = never".
2. Explicitly pass `expires_after_tick=0`.
3. The command expires after tick 0 (immediately), not "never".

## Expected Behavior

The docstring should accurately reflect the default value and the semantics of the expiry parameter.

## Actual Behavior

Docstring says "default 0 = never" but the actual default is `u64::MAX` and `0` means "expires after tick 0" (i.e., immediately stale).

## Reproduction Rate

- N/A (documentation bug).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)
- **Python version (if murk-python):** 3.10+

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```python
import murk

# User reads docs: "default 0 = never" and explicitly passes 0
cmd = murk.Command.set_field(field_id=0, coord=[0, 0], value=1.0, expires_after_tick=0)
# Command expires immediately after tick 0, not "never"
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-python/src/command.rs.md
**Verified lines:** `crates/murk-python/src/command.rs:38,40`
**Root cause:** Docstring was not updated when the default was changed from 0 to `u64::MAX`.
**Suggested fix:** Update the docstring to say "default u64::MAX = never expires" and remove the "0 = never" claim.
