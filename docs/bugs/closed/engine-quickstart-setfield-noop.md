# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine (examples)

## Engine Mode

- [x] Lockstep
- [ ] RealtimeAsync
- [ ] Both / Unknown

## Summary

The `quickstart.rs` example's SetField command injection for the HEAT field is effectively a no-op because the diffusion propagator uses `WriteMode::Full` with `reads_previous()`, overwriting all cells (including the injected value) every tick before the injection can be observed.

## Steps to Reproduce

1. Run `cargo run --example quickstart`.
2. At tick 51, a `SetField` command sets `HEAT` at `(1,1)` to `10.0`.
3. The SetField is applied to staging before propagators (tick.rs:218-233).
4. The diffusion propagator runs with `WriteMode::Full`, reads from `reads_previous()` (frozen tick-start snapshot, which does not include the staging write), and overwrites all cells including `(1,1)`.
5. The injected value is lost; the "second heat spot" described in the example comments at lines 195 and 217 never materializes.

## Expected Behavior

The example should demonstrate that SetField commands produce observable effects in the simulation, as claimed by its comments.

## Actual Behavior

The SetField write is overwritten by the full-write propagator in the same tick, making it unobservable. The example's printed commentary is misleading.

## Reproduction Rate

Always

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.7)

## Determinism Impact

- [ ] Breaks bit-exact determinism
- [ ] May affect simulation behavior
- [x] No determinism impact (example/documentation issue only)

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// Run the quickstart example and observe that the heat map
// at tick 71 shows no evidence of the (1,1) injection.
```

## Additional Context

**Source report:** docs/bugs/generated/crates/murk-engine/examples/quickstart.rs.md
**Verified lines:** quickstart.rs:61-63 (WriteMode::Full), quickstart.rs:55-59 (reads_previous), quickstart.rs:90-125 (overwrites all cells), quickstart.rs:195-217 (misleading comments)
**Root cause:** The example mixes command staging with a single full-overwrite propagator that reads only frozen previous-tick data, making same-tick SetField writes non-observable.
**Suggested fix:** Either add a separate injectable field (e.g., HEAT_INJECTION) that the propagator reads and adds into HEAT, or update the example comments to not claim the SetField perturbation affects the heat field under full-write/reads_previous logic.
