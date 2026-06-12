# Current State - Murk

Checkpoint: 2026-06-12 15:22 UTC - commit ed90bcf

## The bet right now
Prepare the merged entity-model batch as the `0.2.0` release candidate basis, without weakening identity, replay, or FFI safety.

## In flight
- Entity M1/M2 implementation branches are merged onto `main`.
- Workspace version metadata is advanced to `0.2.0`.
- Local release gates pass from the merged candidate.

## Open questions / blocked-on-owner
- Public release, crate publishing, PyPI publishing, or announcement still need owner sign-off.
- Entity-slot observations, FFI/Python entity surface, and line-of-sight remain follow-up scope after the entity foundation release candidate.

## Last checkpoint did
- Merged `codex/release-plan-refresh`, including M1 foundation, M2 engine integration, property coverage, and cross-module integration tests.
- Confirmed Filigree's `.weft/filigree/context.md` snapshot reports no actionable ready work besides the P4 `Future` planning item.
- Refreshed Loomweave after the merge: 3,450 entities, 4,681 edges, 36 subsystems, SEIs populated.
- Ran `cargo fmt --check --all`, `cargo check --workspace`, `cargo test --workspace --all-targets`, `cargo clippy --workspace -- -D warnings`, and `cargo test --workspace --doc` successfully.
- Ran `wardline scan . --lang rust --fail-on ERROR` successfully: 0 active findings; trust-surface marker coverage remains a telemetry follow-up.
- Found the plain `uvx filigree` CLI package is older than this repo's migrated `.weft/filigree` database, so tracker writes should wait for the project-pinned Filigree tool or MCP surface.

## Next session, start here
Push/create the release PR or tag only after owner sign-off for outward-facing release actions; then plan follow-up scope for entity-slot observations and FFI/Python exposure.
