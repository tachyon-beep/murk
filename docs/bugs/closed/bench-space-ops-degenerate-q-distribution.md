# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [x] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [ ] murk-ffi
- [ ] murk-python
- [x] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

The `bench_distance_product_space` benchmark generates degenerate `q` coordinates because the LCG multiplier `6364136223846793005` is not coprime to the modulus `20`. Since `6364136223846793005 % 20 == 5`, the expression `(i * 6364136223846793005) % 20` can only produce values in `{0, 5, 10, 15}` -- exercising only 4 of 20 possible `q` values. The `r` and `l` coordinates are not affected (their multipliers produce full coverage).

This means the benchmark only exercises ~20% of the hex `q`-axis, potentially masking performance issues related to boundary conditions or specific coordinate ranges.

## Steps to Reproduce

1. Enumerate `(i * 6364136223846793005) % 20` for `i in 0..1000`.
2. Observe only 4 distinct values: `{0, 5, 10, 15}`.

## Expected Behavior

Benchmark coordinates should cover all 20 `q` values to exercise the full coordinate space.

## Actual Behavior

Only 4 of 20 `q` values are generated, producing a biased benchmark.

## Reproduction Rate

- 100%, deterministic.

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
fn main() {
    let mut seen = std::collections::HashSet::new();
    for i in 0u64..1000 {
        seen.insert((i.wrapping_mul(6364136223846793005) % 20) as i32);
    }
    println!("q values: {:?}", seen); // {0, 5, 10, 15}
}
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/crates/murk-bench/benches/space_ops.rs.md
**Verified lines:** `crates/murk-bench/benches/space_ops.rs:53,58`
**Root cause:** LCG multiplier 6364136223846793005 shares a factor of 5 with the modulus 20, collapsing the residue set.
**Suggested fix:** Use a multiplier coprime to 20 (any odd value not divisible by 5), e.g., `6364136223846793007` (which is coprime to 20), or use a different deterministic sampling strategy.
