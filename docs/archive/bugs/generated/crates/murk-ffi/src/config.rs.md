# Bug Report

**Date:** 2026-02-23
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

`murk_config_set_space` accepts invalid non-integer/non-finite enum parameters because `f64` values are cast directly to `i32`, causing malformed inputs (including `NaN`) to be silently interpreted as valid enum tags in `/home/john/murk/crates/murk-ffi/src/config.rs`.

## Steps to Reproduce

1. Create a config via `murk_config_create`.
2. Call `murk_config_set_space(handle, 0 /*Line1D*/, params, 2)` with `params = {10.0, NAN}` (or `{10.0, 1.9}`).
3. Observe return code is `MURK_OK` instead of `MURK_ERROR_INVALID_ARGUMENT`.

## Expected Behavior

Invalid enum-like space parameters (edge behavior / nested component type) should be rejected unless finite, integral, and in-range.

## Actual Behavior

Invalid `f64` enum parameters are accepted after lossy cast:
- `NAN as i32` becomes `0` (accepted as `Absorb`)
- `1.9 as i32` becomes `1` (accepted as `Clamp`)

## Reproduction Rate

Always

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** Any

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```text
N/A - found via static analysis
```

## Minimal Reproducer

```c
#include <math.h>
#include <stdint.h>
#include <stdio.h>

// Assume murk.h is included and linked.

int main(void) {
    uint64_t cfg = 0, world = 0;
    int rc = murk_config_create(&cfg);
    if (rc != 0) return rc;

    // Line1D (0), params = [len, edge_behavior]
    // edge_behavior should be 0/1/2 only, but NAN is incorrectly accepted.
    double params[2] = {10.0, NAN};
    rc = murk_config_set_space(cfg, 0, params, 2);
    printf("murk_config_set_space rc=%d\n", rc); // actual: 0 (MURK_OK), expected: invalid argument

    murk_config_destroy(cfg);
    (void)world;
    return 0;
}
```

## Additional Context

Evidence in `/home/john/murk/crates/murk-ffi/src/config.rs`:
- `parse_edge_behavior(p[1] as i32)` at `:110`
- `parse_edge_behavior(p[2] as i32)` at `:128` and `:139`
- `parse_edge_behavior(p[3] as i32)` at `:163`
- `let comp_type = p[offset] as i32` at `:183`

Root cause: enum-like values embedded in `f64` parameter arrays are converted with unchecked `as i32` casts instead of strict finite/integer/range validation.

Suggested fix: introduce a helper for exact enum decoding from `f64` (finite + integral + bounds check), and use it for edge behavior and product-space component type parsing.