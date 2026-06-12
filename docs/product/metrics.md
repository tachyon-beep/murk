# Metrics - Murk

Last read: 2026-06-12

## North-star
| Metric | Target | Current | Read on | Trend |
|--------|--------|---------|---------|-------|
| Echelon-blocking capability coverage | Entity model M1-M4 accepted before v0.2 release candidate | M1/M2 entity foundation merged to `main`; entity-slot observations remain follow-up scope | 2026-06-12 | improving |

## Input metrics
| Metric | Target | Current | Read on |
|--------|--------|---------|---------|
| Live tracker accuracy | 0 ready issues that reference nonexistent implementation as if present | `.weft/filigree/context.md` reports only P4 `Future` planning ready; plain `uvx filigree` is too old for the migrated DB | 2026-06-12 |
| Tooling index health | Loomweave fresh with Rust entities and SEIs populated | Fresh after merge: 3,450 entities, 4,681 edges, 36 subsystems, SEIs populated | 2026-06-12 |
| Verification health | Full Cargo workspace tests pass | `cargo fmt --check --all`, `cargo check --workspace`, `cargo test --workspace --all-targets`, `cargo clippy --workspace -- -D warnings`, and `cargo test --workspace --doc` passed | 2026-06-12 |

## Guardrails
| Metric | Floor / ceiling | Current | Read on |
|--------|-----------------|---------|---------|
| Wardline Rust ERROR findings | 0 active ERROR findings | 0 active ERROR findings; one `WLN-RUST-COVERAGE` metric finding at severity NONE | 2026-06-12 |
| Secret exposure in committed files | 0 committed secrets | Loomweave detected high entropy in ignored local `.env`; not committed | 2026-06-12 |
| Public release actions without owner sign-off | 0 | 0 | 2026-06-12 |
