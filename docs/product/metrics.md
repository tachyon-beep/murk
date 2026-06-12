# Metrics - Murk

Last read: 2026-06-12

## North-star
| Metric | Target | Current | Read on | Trend |
|--------|--------|---------|---------|-------|
| Echelon-blocking capability coverage | Entity model M1-M4 accepted before v0.2 release candidate | M1 design exists; implementation absent on `main` | 2026-06-12 | baseline |

## Input metrics
| Metric | Target | Current | Read on |
|--------|--------|---------|---------|
| Live tracker accuracy | 0 ready issues that reference nonexistent implementation as if present | 3 ready entity-model issues point at nonexistent `murk-entity` files | 2026-06-12 |
| Tooling index health | Loomweave fresh with Rust entities and SEIs populated | Fresh: 3,246 entities, 4,464 edges, 21 subsystems, SEIs populated | 2026-06-12 |
| Verification health | Full Cargo workspace tests pass | `cargo test --workspace --all-targets` passed | 2026-06-12 |

## Guardrails
| Metric | Floor / ceiling | Current | Read on |
|--------|-----------------|---------|---------|
| Wardline Rust ERROR findings | 0 active ERROR findings | 0 active ERROR findings; one `WLN-RUST-COVERAGE` metric finding at severity NONE | 2026-06-12 |
| Secret exposure in committed files | 0 committed secrets | Loomweave detected high entropy in ignored local `.env`; not committed | 2026-06-12 |
| Public release actions without owner sign-off | 0 | 0 | 2026-06-12 |
