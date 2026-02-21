# Bug Report

**Date:** 2026-02-20
**Reporter:** crate-audit-wave4
**Severity:** [x] Critical | [ ] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [x] murk-propagators

## Engine Mode

- [x] Lockstep

## Summary

The diffusion propagators (`ScalarDiffusion` and `DiffusionPropagator`) compute `alpha = coefficient * dt * neighbour_count`. When `alpha > 1.0`, the weight `(1.0 - alpha)` goes negative, causing value sign inversions and oscillatory blow-up. The `max_dt()` constraint prevents this for the worst-case degree (12 for FCC), but there is no runtime guard. If the engine's CFL check is bypassed (e.g., standalone propagator use, or a space with unusual degree distribution), blow-up occurs silently.

Additionally, `DiffusionPropagator::new(diffusivity)` (deprecated) accepts zero or negative values:
- Zero diffusivity: `max_dt()` returns `Some(1.0 / (12.0 * 0.0))` = `Some(+Infinity)`, not `None`.
- Negative diffusivity: produces negative `max_dt`, confusing CFL logic.

## Steps to Reproduce

```rust
// Alpha blow-up (if CFL bypassed):
let prop = ScalarDiffusion::builder()
    .output(FieldId(0)).coefficient(1.0).build().unwrap();
// With dt=0.5 and 4 neighbours: alpha = 1.0 * 0.5 * 4 = 2.0
// Weight (1.0 - 2.0) = -1.0 → sign inversion on every cell

// Zero diffusivity:
let prop = DiffusionPropagator::new(0.0);
assert_eq!(prop.max_dt(), Some(f64::INFINITY)); // Should be None
```

## Expected Behavior

1. Alpha clamped to `[0.0, 1.0]` inside the diffusion loop (unconditionally stable).
2. Zero/negative diffusivity rejected at construction time.

## Actual Behavior

Alpha unbounded; zero diffusivity produces infinite max_dt.

## Additional Context

**Source:** murk-propagators audit, C-1 + C-2
**Files:** `crates/murk-propagators/src/scalar_diffusion.rs:197-199`, `crates/murk-propagators/src/diffusion.rs:106-108,26-29,333`
**Suggested fix:**
1. Add `let alpha = (self.coefficient * dt * count as f64).min(1.0) as f32;` — one line, makes diffusion unconditionally stable.
2. Add validation in `DiffusionPropagator::new()`: `assert!(diffusivity >= 0.0)` or return `Result`.
