# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-engine

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`TickEngine::new` performs an unchecked `u32` multiplication for static field lengths (`cell_count * components`), which panics in debug builds or silently wraps in release when the product overflows `u32`.

## Steps to Reproduce

1. Build a `WorldConfig` with a `Static` vector field where `cell_count * dims` overflows `u32` (e.g., cell_count=3, dims=2_863_311_531).
2. Call `TickEngine::new(config)` in a debug build.
3. Observe arithmetic overflow panic at `tick.rs:146`.

## Expected Behavior

Construction should fail gracefully with `Err(ConfigError::...)`, not panic.

## Actual Behavior

At `tick.rs:143-147`, static field lengths are computed as:
```rust
let static_fields: Vec<(FieldId, u32)> = arena_field_defs
    .iter()
    .filter(|(_, d)| d.mutability == FieldMutability::Static)
    .map(|(id, d)| (*id, cell_count * d.field_type.components()))
    .collect();
```
The `cell_count * d.field_type.components()` at line 146 is an unchecked `u32 * u32` multiplication. `config.validate()` (called at tick.rs) checks that `cell_count` fits in `u32` and that `dims > 0`, but does NOT check the product `cell_count * components`. Ticket #84 added `checked_mul` to the arena's `from_field_defs`, but this code path (for `StaticArena::new`) bypasses that check because the product is computed before passing to the arena.

## Reproduction Rate

Always (debug build, overflowing input)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** 0.1.8 / HEAD (feat/release-0.1.9)

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
let cfg = WorldConfig {
    space: Box::new(Line1D::new(3, EdgeBehavior::Absorb).unwrap()),
    fields: vec![FieldDef {
        name: "static_vec".into(),
        field_type: FieldType::Vector { dims: 2_863_311_531 }, // 3 * dims overflows u32
        mutability: FieldMutability::Static,
        units: None,
        bounds: None,
        boundary_behavior: BoundaryBehavior::Clamp,
    }],
    propagators: vec![Box::new(Noop)],
    dt: 0.1,
    seed: 1,
    ring_buffer_size: 8,
    max_ingress_queue: 16,
    tick_rate_hz: None,
    backoff: BackoffConfig::default(),
};
// Debug build: panic at tick.rs:146 (overflow)
let _ = TickEngine::new(cfg);
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-engine/src/tick.rs.md`

**Affected lines:**
- Unchecked multiply: `crates/murk-engine/src/tick.rs:146`
- Static arena construction: `crates/murk-engine/src/tick.rs:148`

**Root cause:** The `cell_count * d.field_type.components()` multiplication is performed without overflow checking, and `config.validate()` does not validate the product for static fields.

**Suggested fix:** Use `cell_count.checked_mul(d.field_type.components()).ok_or(ConfigError::CellCountOverflow { ... })?` and propagate the error. Alternatively, add a product-overflow check to `WorldConfig::validate()` for all static fields.
