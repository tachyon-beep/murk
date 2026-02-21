# Bug Report

**Date:** YYYY-MM-DD
**Reporter:**
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [ ] Low

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
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [ ] Both / Unknown

## Summary

<!-- One sentence describing the bug. -->

## Steps to Reproduce

1.
2.
3.

## Expected Behavior

<!-- What should happen. -->

## Actual Behavior

<!-- What happens instead. Include error codes (e.g. MURK_ERROR_PROPAGATOR_FAILED) if applicable. -->

## Reproduction Rate

<!-- Always / Intermittent / Once -->

## Environment

- **OS:**
- **Rust toolchain:** (`rustc --version`)
- **Murk version/commit:**
- **Python version (if murk-python):**

## Determinism Impact

- [ ] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
<!-- Paste relevant output, RUST_BACKTRACE=1 trace, or murk metrics here. -->
```

## Minimal Reproducer

```rust
// If possible, provide a minimal code snippet that triggers the bug.
```

## Additional Context

<!-- Related issues, design decisions, or links to docs/error-reference.md entries. -->
