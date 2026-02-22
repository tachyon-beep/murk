# Introduction

Murk is a world simulation engine for reinforcement learning and real-time
applications.

It provides a tick-based simulation core with pluggable spatial backends,
a modular propagator pipeline, ML-native observation extraction, and
Gymnasium-compatible Python bindings — all backed by arena-based
generational allocation for deterministic, zero-GC memory management.

## Features

- **Spatial backends** — Line1D, Ring1D, Square4, Square8, Hex2D, and
  composable ProductSpace (e.g. Hex2D × Line1D)
- **Propagator pipeline** — stateless per-tick operators with automatic
  write-conflict detection, Euler/Jacobi read modes, and topology-aware
  CFL validation (`max_dt(space)`)
- **Observation extraction** — ObsSpec → ObsPlan → flat `f32` tensors with
  validity masks, foveation, pooling, and multi-agent batching
- **Two runtime modes** — `LockstepWorld` (synchronous, borrow-checker
  enforced) and `RealtimeAsyncWorld` (background tick thread with epoch-based
  reclamation)
- **Batched engine** — `BatchedEngine` steps N worlds and extracts
  observations in one call with a single GIL release; `BatchedVecEnv`
  provides an SB3-compatible Python interface
- **Deterministic replay** — binary replay format with per-tick snapshot
  hashing and divergence reports
- **Arena allocation** — double-buffered ping-pong arenas with Static/PerTick/Sparse
  field mutability classes; no GC pauses, no `Box<dyn>` per cell
- **Step metrics observability** — per-step timings plus sparse retirement
  and sparse reuse counters (`sparse_retired_ranges`, `sparse_pending_retired`,
  `sparse_reuse_hits`, `sparse_reuse_misses`)
- **C FFI** — stable ABI v2.1 with handle tables (slot+generation),
  panic-safe boundary (`MurkStatus::Panicked`, `murk_last_panic_message`),
  and safe double-destroy
- **Python bindings** — PyO3/maturin native extension with Gymnasium `Env`/`VecEnv`
  and `BatchedVecEnv` for high-throughput training
- **Zero `unsafe` in simulation logic** — only `murk-arena` and `murk-ffi`
  are permitted `unsafe`; everything else is `#![forbid(unsafe_code)]`

## Architecture

```
┌─────────────────────────────────────────────────────┐
│  Python (murk)          │  C consumers              │
│  MurkEnv / BatchedVecEnv│  murk_lockstep_step()     │
├────────────┬────────────┴───────────────────────────┤
│ murk-python│           murk-ffi                     │
│ (PyO3)     │        (C ABI, handle tables)          │
├────────────┴────────────────────────────────────────┤
│                    murk-engine                       │
│  LockstepWorld · RealtimeAsyncWorld · BatchedEngine        │
│        TickEngine · IngressQueue · EgressPool        │
├──────────────┬──────────────┬───────────────────────┤
│ murk-propagator │  murk-obs │   murk-replay         │
│ Propagator trait│  ObsSpec  │   ReplayWriter/Reader  │
│ StepContext     │  ObsPlan  │   determinism verify   │
├──────────────┴──┴──────────┬┴───────────────────────┤
│       murk-arena           │      murk-space         │
│  PingPongArena · Snapshot  │  Space trait · backends │
│  ScratchRegion · Sparse    │  regions · edges        │
├────────────────────────────┴────────────────────────┤
│                     murk-core                        │
│    FieldDef · Command · SnapshotAccess · IDs         │
└─────────────────────────────────────────────────────┘
```

## Getting started

Head to the [Getting Started](getting-started.md) guide for installation
instructions and your first simulation.
