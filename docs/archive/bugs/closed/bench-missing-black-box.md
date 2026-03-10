# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-bench

## Engine Mode

- [x] Lockstep

## Summary

All three benchmarks in `reference_profile.rs` call `world.step_sync(vec![]).unwrap()` but never pass the return value through `criterion::black_box`. The `StepResult` (containing Snapshot + Vec<Receipt>) is dropped immediately. LLVM is free to observe the unused return and optimize away portions of the computation. While side effects (mutation of `&mut world`) prevent full dead-code elimination, the benchmark is fragile — especially `bench_1000_ticks_10k` which creates the world inside the closure.

Additionally, arena benchmarks (`arena_ops.rs`) have methodology issues:
- `bench_arena_write_10k` and `bench_arena_snapshot` publish with constant `TickId(1)`/`TickId(2)` while the internal generation counter monotonically increases — measuring diverging state, not steady-state.
- `bench_arena_alloc_10k` measures construction + begin_tick combined, not isolating per-tick cost.

## Expected Behavior

Benchmark results wrapped in `black_box`. Arena benchmarks use monotonically increasing TickIds.

## Actual Behavior

Results discarded. Arena benchmark state diverges from real usage.

## Additional Context

**Source:** murk-bench audit, Findings 1-5
**Files:** `crates/murk-bench/benches/reference_profile.rs:16-48`, `crates/murk-bench/benches/arena_ops.rs:102-135`
**Suggested fix:** Add `black_box(&result)` after every `step_sync` call. Use incrementing TickId in arena benchmarks.
