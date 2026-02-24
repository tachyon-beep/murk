Using skill: `using-software-engineering` (this is a focused code-review/bug-audit task).

# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
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

No concrete bug found in /home/john/murk/crates/murk-core/src/field.rs.

## Steps to Reproduce

1. Inspect `/home/john/murk/crates/murk-core/src/field.rs`.
2. Audit arithmetic, indexing, iterator behavior, and validation logic.
3. Attempt to identify a concrete panic/UB/incorrect-results path.

## Expected Behavior

No concrete bug should be present in this file.

## Actual Behavior

No concrete bug was identified in this file.

## Reproduction Rate

N/A (no bug found)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** N/A

## Determinism Impact

- [ ] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```text
N/A - found via static analysis
```

## Minimal Reproducer

```text
N/A - no concrete bug found
```

## Additional Context

Reviewed the full target file and checked key risk points including bitset indexing and shifts at `/home/john/murk/crates/murk-core/src/field.rs:227`, resize/allocation behavior at `/home/john/murk/crates/murk-core/src/field.rs:230`, iterator emission logic at `/home/john/murk/crates/murk-core/src/field.rs:402`, and ID reconstruction cast at `/home/john/murk/crates/murk-core/src/field.rs:409`. No demonstrable panic/UB/incorrect-result defect was found from this file alone.