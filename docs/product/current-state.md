# Current State - Murk

Checkpoint: 2026-06-12 14:08 UTC - commit 0cd5a36

## The bet right now
Make the entity model real enough to unblock Echelon-style fixed-shape entity observations, without weakening identity, replay, or FFI safety.

## In flight
- `murk-d10dc88f0f` - generation wrap policy decision - claimed by `codex`; decision recorded as retire-on-wrap in the entity design and M1 plan.
- `murk-e4505f17ed` - proptest coverage for entity IDs and staging - tracker is premature because `murk-entity` is not implemented on `main`.
- `murk-5cebd6aca0` - integration tests for `murk-entity` cross-module behavior - tracker is premature because `murk-entity` is not implemented on `main`.

## Open questions / blocked-on-owner
- Confirm whether the next major release target should be `v0.2` entity-slot observations, with line-of-sight following, or whether line-of-sight should remain the headline v0.2 bet from `ROADMAP.md`.
- Decide whether to create a fresh Filigree milestone plan for entity M1-M5 or rewrite the existing loose ready issues into milestone children.

## Last checkpoint did
- Installed/verified Filigree 3.0.0rc12, Loomweave 1.1.0rc4 with Rust plugin, Wardline 1.0.0rc4 with Rust scanner support, and Legis 1.0.0.
- Ran Loomweave analysis successfully: 3,246 entities, 4,464 edges, 21 subsystems, SEIs populated.
- Ran Wardline Rust scan successfully: 0 active ERROR findings; one coverage metric finding.
- Ran `cargo test --workspace --all-targets` successfully.
- Established that no branch/worktree contains the planned `murk-entity` implementation.

## Next session, start here
Turn the entity M1 plan into executable tracker structure, then implement M1 in a branch/worktree using the updated retire-on-wrap policy and tests.
