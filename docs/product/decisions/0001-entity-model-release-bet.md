# PDR-0001 - Make entity model the next release bet

Date: 2026-06-12   Status: accepted   Author: codex   Owner sign-off: n/a
Supersedes: none   Related: roadmap.md (Now), metrics.md (north-star), `murk-d10dc88f0f`

## Context
The tracker's ready batch references `murk-entity`, `EntityStore`, `EntityId`, and `PropertyStaging`, but the current `main` branch and all visible local/remote branches contain only the design and M1 plan. The current public roadmap says v0.2 is Echelon-driven; the entity model is the foundation for Echelon-style entity observations and must exist before downstream engine, propagator, slot, and Python work can deliver value.

## Options considered
1. Treat the ready issues as immediately implementable test tasks - rejected because the referenced crate and files do not exist.
2. Close the entity issues as stale and return to line-of-sight as the v0.2 headline - rejected because it leaves the Echelon observation bottleneck in Python-side reshaping.
3. Promote entity M1-M4 as the Now bet, repair the policy gap, and reshape the tracker around implementation milestones - chosen.

## The call
The next release bet is native entity model support leading to fixed-shape entity-slot observations. The generation wrap policy is retire-on-wrap: an entity slot whose 12-bit generation would roll back to zero is permanently retired instead of recycled.

## Rationale
This bet turns the latest design work into product value, resolves the current tracker/code mismatch, and preserves Murk's safety positioning. Retire-on-wrap matches the existing `murk-ffi` `HandleTable` policy and avoids stale-ID ABA resurrection on hot slots.

## Reversal trigger
Reopen this decision if owner direction makes line-of-sight the release headline before entity-slot observations, or if M1 implementation proves the entity model is not needed for the near-term Echelon validation path.
