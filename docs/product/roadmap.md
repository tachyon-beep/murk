# Roadmap - Murk

Updated: 2026-06-12 (PDR-0001)

This file records product bets as intent. Delivery sequencing, dated forecasts,
and flow mechanics live in Filigree and program-management planning, not here.

## Now
- **Entity model foundation for Echelon-class observations** - convert the 2026-04 entity model design from prose into a real Rust implementation and test suite. Tracker: `murk-d10dc88f0f`, `murk-e4505f17ed`, `murk-5cebd6aca0`. Metric: entity foundation acceptance checklist complete with full workspace tests passing.

## Next
- **Engine/propagator entity integration** - wire spawn/despawn/move lifecycle, rollback, StepContext entity reads/writes, and replay format compatibility after M1 exists.
- **Entity-slot observation extraction** - deliver fixed-shape entity contact tensors without Python-side per-step reshaping, including deterministic tie-breaking and dead/stale observer handling.
- **Tooling-backed release readiness** - keep Filigree, Loomweave Rust indexing, Wardline Rust scan, Legis doctor, and Cargo workspace tests green enough to make release state inspectable.

## Later
- **Line-of-sight and sensor modalities** - implement Echelon visual/radar sensor support once native entities exist.
- **Python training ergonomics for entity worlds** - expose entity-slot observations through batched Python APIs with one GIL release per batch.
- **Broader Echelon showcase loop** - use the mech-combat demo to validate throughput, determinism, and observation quality under self-play workloads.
