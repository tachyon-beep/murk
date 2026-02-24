# Bug Report

**Date:** February 23, 2026  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [x] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`/home/john/murk/crates/murk-ffi/include/murk.h` defines duplicate unscoped C enum constants (`Absorb`, `Clamp`, `Wrap`) across two enums, causing C compilation failure.

## Steps to Reproduce

1. Create a minimal C TU that includes `murk.h`.
2. Run:  
   `printf '#include "/home/john/murk/crates/murk-ffi/include/murk.h"\nint main(void){return 0;}\n' | cc -xc - -fsyntax-only`
3. Observe compiler errors for redeclared enumerators.

## Expected Behavior

Including `murk.h` in C should compile cleanly.

## Actual Behavior

Compilation fails with redeclaration errors:
- `Absorb` redefined at `crates/murk-ffi/include/murk.h:65`, previously at `crates/murk-ffi/include/murk.h:35`
- `Clamp` redefined at `crates/murk-ffi/include/murk.h:69`, previously at `crates/murk-ffi/include/murk.h:27`
- `Wrap` redefined at `crates/murk-ffi/include/murk.h:73`, previously at `crates/murk-ffi/include/murk.h:39`

## Reproduction Rate

Always

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):**
- **C compiler (if murk-ffi C header/source):** `cc`

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```c
#include "/home/john/murk/crates/murk-ffi/include/murk.h"
int main(void) { return 0; }
```

## Additional Context

Root cause is C enum namespace collision: C enumerators are unscoped identifiers, so reusing names across `enum MurkBoundaryBehavior` (`Clamp`, `Absorb`, `Wrap` at lines 27/35/39) and `enum MurkEdgeBehavior` (`Absorb`, `Clamp`, `Wrap` at lines 65/69/73) is invalid.  
Suggested fix: configure cbindgen to prefix enum variants (e.g., `MURK_BOUNDARY_CLAMP`, `MURK_EDGE_CLAMP`) or otherwise emit non-colliding constants.