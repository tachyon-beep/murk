# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent
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

`parse_space()` in `config.rs` uses raw `as i32` casts on `f64` parameters destined for enum matching (edge behavior, product-space component type) instead of the validated `f64_to_u32`/`f64_to_usize` helpers used for dimension parameters. This silently accepts non-integer and non-finite `f64` values: `NaN` becomes `0` (maps to a valid enum tag), and non-integer values like `1.9` truncate to `1` (also a valid tag).

## Steps to Reproduce

1. Create a config via `murk_config_create`.
2. Call `murk_config_set_space(handle, 0 /*Line1D*/, params, 2)` with `params = {10.0, NAN}`.
3. Observe `MURK_OK` is returned instead of `MURK_ERROR_INVALID_ARGUMENT`.

## Expected Behavior

Invalid enum-like space parameters (edge behavior, product-space component type) should be rejected unless the `f64` value is finite, integral, and in range. Passing `NaN`, `Infinity`, or non-integer values for an enum discriminant should return `MURK_ERROR_INVALID_ARGUMENT`.

## Actual Behavior

Invalid `f64` enum parameters are silently accepted after lossy truncation:
- `NAN as i32` becomes `0` in Rust (saturating cast), accepted as `Absorb`
- `1.9 as i32` truncates to `1`, accepted as `Clamp`
- `f64::INFINITY as i32` becomes `i32::MAX` (2147483647), which does **not** match any enum arm and returns `None` (rejected) -- so infinity is caught, but `NaN` and non-integers are not

The `parse_edge_behavior` function correctly validates the resulting `i32` against known enum tags, but the problem is upstream: the cast from `f64` to `i32` silently corrupts the value before validation.

## Reproduction Rate

Always (deterministic).

## Environment

- **OS:** Any
- **Rust toolchain:** stable (1.45+, saturating float-to-int casts)
- **Murk version/commit:** HEAD (feat/release-0.1.9)

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
#include <math.h>
#include <stdio.h>

int main(void) {
    uint64_t cfg = 0;
    murk_config_create(&cfg);

    // Line1D: params = [length, edge_behavior]
    // edge_behavior should be 0, 1, or 2 only.
    double params_nan[2] = {10.0, NAN};
    int rc = murk_config_set_space(cfg, 0, params_nan, 2);
    printf("NaN edge_behavior: rc=%d\n", rc);
    // actual: 0 (MURK_OK, NaN silently becomes Absorb)
    // expected: -18 (MURK_ERROR_INVALID_ARGUMENT)

    double params_frac[2] = {10.0, 1.9};
    rc = murk_config_set_space(cfg, 0, params_frac, 2);
    printf("1.9 edge_behavior: rc=%d\n", rc);
    // actual: 0 (MURK_OK, 1.9 silently becomes Clamp)
    // expected: -18 (MURK_ERROR_INVALID_ARGUMENT)

    murk_config_destroy(cfg);
    return 0;
}
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-ffi/src/config.rs.md`

**Affected lines in `crates/murk-ffi/src/config.rs`:**
- Line 110: `parse_edge_behavior(p[1] as i32)` (Line1D)
- Line 128: `parse_edge_behavior(p[2] as i32)` (Square4)
- Line 139: `parse_edge_behavior(p[2] as i32)` (Square8)
- Line 163: `parse_edge_behavior(p[3] as i32)` (Fcc12)
- Line 183: `let comp_type = p[offset] as i32` (ProductSpace component type)

**Contrast with existing validated casts:** The same function already uses `f64_to_u32()` (line 84) and `f64_to_usize()` (line 93) for dimension/count parameters, which correctly reject non-finite, non-integer, and out-of-range values. The enum parameter casts were not updated when those helpers were introduced (fix for closed ticket #21, `ffi-productspace-unchecked-float-cast`).

**Root cause:** Enum-typed values embedded in `f64` parameter arrays are converted with unchecked `as i32` casts, bypassing the finite/integer/range validation applied to other parameters.

**Suggested fix:** Introduce an `f64_to_i32` helper analogous to `f64_to_u32`:
```rust
fn f64_to_i32(v: f64) -> Option<i32> {
    if !v.is_finite() || v < i32::MIN as f64 || v > i32::MAX as f64 || v != v.trunc() {
        return None;
    }
    Some(v as i32)
}
```
Replace all `p[x] as i32` casts in `parse_space` with `f64_to_i32(p[x])?`. This ensures non-finite and non-integer values are rejected before reaching `parse_edge_behavior` or the `parse_space` recursive call for product-space components.
