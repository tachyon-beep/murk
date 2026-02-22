# Murk Roadmap

This document describes the planned development trajectory for Murk.
Priorities are driven by **Echelon** (working title), a DRL mech combat
demo that serves as both the primary proving ground and showcase for the
engine.

> **Versioning**: Murk follows 0.x semver. The API is not yet stable.
> Breaking changes are expected between minor versions until 1.0.

---

## Current State — v0.1.8 (Release Prep)

As of February 22, 2026, Murk v0.1.8 is in release prep. Everything
below is implemented and tested on the release branch; publication is
the next operational step.

### Engine

| Subsystem | What's There | Evidence |
|-----------|-------------|----------|
| **Spatial backends** | 7 lattice types: Line1D, Ring1D, Square4, Square8, Hex2D, Fcc12, ProductSpace | Compliance test suite verifies all backends satisfy Space trait invariants |
| **Arena allocation** | Double-buffered ping-pong arenas with Static/PerTick/Sparse field classes | Miri-verified memory safety; zero GC pauses |
| **Propagator pipeline** | Stateless `&self` propagators, Euler/Jacobi read modes, write-conflict detection, CFL validation | 90+ unit tests; 3 reference propagators (diffusion, agent movement, reward) |
| **Observation extraction** | ObsSpec → ObsPlan compilation, foveation (AgentDisk, AgentRect), pooling, multi-agent batching | 51 unit tests; flat f32 tensor output ready for PyTorch |
| **Lockstep mode** | `step_sync(&mut self)` — borrow checker enforces single-threaded access | Send but !Sync by design; no runtime mode checks |
| **Realtime-async mode** | Background tick thread, epoch-based reclamation, adaptive backoff, shutdown FSM | 5 stress tests including death spiral and rejection oscillation |
| **Deterministic replay** | Binary format v2 with per-tick snapshot hashing, divergence reports | 177 determinism tests; FNV-1a hash verification per tick |
| **Batched engine** | `BatchedEngine` with N-world `step_and_observe()`, single GIL release, `BatchedVecEnv` SB3-compatible adapter | Unit tests + Python integration tests |
| **0.1.8 hardening** | Panic-safe FFI boundary, topology-aware CFL validation, non-mutating realtime preflight telemetry path | Regression coverage for panic status, full-topology CFL, and preflight metrics integrity |

### Bindings and Tooling

| Layer | What's There | Evidence |
|-------|-------------|----------|
| **C FFI** | 41+ extern functions, slot+generation handle tables, panic-safe boundary, versioned ABI (v3.0) | Safe double-destroy, null validation, panic-to-status conversion; `#![forbid(unsafe_code)]` on everything above FFI |
| **Python** | PyO3/maturin bindings, Gymnasium `Env` + `VecEnv` adapters, `BatchedWorld` + `BatchedVecEnv` high-throughput training, 28+ exposed types, PEP 561 type stubs | 87 passing Python tests including batched engine and PPO training smoke test |
| **CI/CD** | 7 CI jobs (check, MSRV, test, clippy, fmt, Miri, deny), cross-platform (Ubuntu/macOS/Windows) | Manual release workflow publishing to crates.io and PyPI |
| **Documentation** | Architecture guide, concepts guide, error reference (19K), replay format spec, determinism catalogue | `#![deny(missing_docs)]` enforced across all 11 public crates |

### What v0.1.x Proved

The numbers tell a story about architectural fitness:

- **~32K LOC across 13 crates** with clean dependency layering
  (murk-core is a leaf; murk is a facade; everything composes)
- **700+ tests** — unit, integration, property-based (proptest), stress,
  and end-to-end (PPO training)
- **Zero `unsafe` in simulation logic** — `#![forbid(unsafe_code)]` on
  10 of 13 crates. Only murk-arena (allocation) and murk-ffi (C ABI)
  are permitted unsafe blocks
- **Release hardening closed for v0.1.8** — panic-safe FFI paths,
  topology-aware CFL validation, and realtime preflight observability
  corrections are implemented with regression coverage
- **14K steps/sec per environment** (70% of MuJoCo), 3μs framework
  overhead per tick
- **9 critical issues, 14 important issues** identified in design
  review — all resolved with 18 design refinements, zero TBD gaps
  remaining
- **17 unanimous decisions** from a 3-expert domain review panel
  (systems engineering, DRL, simulation) — all implemented

The core architecture — arena allocation, split-borrow propagators,
tick-expressible time, the three-interface model — is validated. The
design review process (architectural review → domain expert panel →
consolidated refinements) produced a foundation that held up through
full implementation without requiring structural rework.

---

## v0.2 — Echelon-Driven Features

The next release is shaped by what Echelon needs. Echelon is a
multi-agent DRL mech combat demo on an Fcc12 (3D face-centred cubic)
lattice with 50–100 agents, multiple sensor modalities, destructible
terrain, and self-play training.

### Line-of-Sight Queries

**Priority**: Critical — blocks visual and radar sensor modalities.

Echelon's mechs need visual and radar observations that respect
occlusion. A mech behind a building shouldn't appear in another mech's
visual channel; a radar return should penetrate smoke but attenuate
through structures.

**Scope**:
- New `RegionSpec` variant or `Space` trait method for visibility
  queries on lattice topologies
- Per-field opacity maps (smoke blocks vision, not radar; walls block
  both but with different attenuation)
- Efficient implementation on Fcc12 — likely a 3D Bresenham or
  DDA-style raycast through the lattice

**Impact**: Enables the core gameplay loop where positioning, cover, and
scouting matter.

### Batched Engine (Remaining)

The core `BatchedEngine` shipped in v0.1.7 and is production-ready
for training workloads (N-world `step_and_observe()` with single GIL
release, SB3-compatible `BatchedVecEnv`). What remains for v0.2 are
advanced features for competitive self-play:

- **Per-world policy assignment** for self-play and league training
- **Checkpoint/restore any world in the batch** (leverages deterministic
  replay)
- **Rayon parallelism for `step_and_observe()`** — currently sequential;
  the design supports a 3-line upgrade to `par_iter_mut`

### Render Adapter Interface

**Priority**: High — blocks the "cool tech demo" visualisation layer.

The observation pipeline answers "what does the agent see?" The render
pipeline answers "what does the human see?" These are structurally
similar (both read from snapshots, both extract spatial data) but differ
in fidelity, format, and frequency.

**Scope**:
- `RenderSpec` → `RenderPlan` compilation (mirroring ObsSpec → ObsPlan)
- Output: structured scene description (positions, field values, events)
  rather than flat tensors
- Adapter trait for pluggable renderers (Bevy, terminal, web, etc.)
- Stateless per-tick scene output initially (renderer maintains its own
  interpolation/state)

**Design decision**: full scene description per tick (stateless) vs
delta stream (stateful). Stateless is simpler, swaps renderers freely,
and is "free" for replay visualisation since snapshots already contain
full state. Delta optimisation can come later.

**Impact**: Makes Echelon visible. Enables replay visualisation for
debugging and presentation.

### Agent-Type Observation Composition

**Priority**: High — required for heterogeneous mech types.

Echelon features distinct mech types (scouts, heavy armour, artillery)
with different sensor loadouts:

| Sensor | Fields Read | Region | Notes |
|--------|-------------|--------|-------|
| Thermal | temperature | AgentDisk | All mech types |
| Visual | agent_presence, terrain | LOS-occluded | Blocked by smoke/walls |
| Radar | agent_presence, terrain | LOS-occluded | Penetrates smoke, attenuated by walls |
| Sound | sound_level | AgentDisk (large) | Propagated field, no occlusion |
| Link-22 | (relay) | Network | Scout targeting data shared to fire-support |

**Scope**:
- Per-agent-type `ObsSpec` binding — different mechs compile different
  observation plans
- Multi-policy batching — extract observations for N agents across M
  distinct specs in a single pass

The Link-22 relay (scout shares targeting data with artillery) is
architecturally novel: it's an observation that depends on another
agent's observation output. This may need a new concept — an
"observation relay" or "shared channel" — rather than a direct field
read. Design TBD.

**Impact**: Enables the heterogeneous multi-agent gameplay that makes
Echelon interesting.

### Propagator Ecosystem for Echelon

**Priority**: Medium — does not require engine changes.

Echelon needs several environmental propagators beyond the existing
reference set:

| Propagator | Reads | Reads Previous | Writes | Mode |
|------------|-------|----------------|--------|------|
| Fire | fuel, wind | temperature | temperature, fuel | Euler |
| Smoke | temperature, wind | smoke_density | smoke_density | Jacobi |
| Water | terrain_height | water_level | water_level | Jacobi |
| Concussive | — | blast_energy | blast_energy, structural_hp | Euler |

These compose through the existing propagator pipeline with no engine
changes. The interesting emergent behaviour — explosions damaging
structures which alters fire/water/smoke flow — falls out of orthogonal
propagator interaction, not special-cased logic.

**Impact**: Enables the rich environmental simulation that makes Echelon
a stress test, not just a grid game.

---

## v0.3 — Performance and Scale

Features driven by real profiling data from Echelon training runs.
Priorities within this release will be determined by where actual
bottlenecks appear.

### Phase 2 Arena Optimisation

- `MaybeUninit` backing for arena segments (skip zero-init for
  `WriteMode::Full` fields)
- `FullWriteGuard` debug coverage tracking (verify all cells written
  before publish)
- Deferred from v0.1 deliberately — correctness before performance

### Parallel Propagator Execution

- The Euler/Jacobi read-mode split already defines a partial order on
  propagators
- Propagators with non-overlapping write sets and compatible read modes
  can execute concurrently
- Scheduling across a thread pool using the existing dependency
  information

### Observation Extraction Optimisation

- Profile the ObsPlan execution path under Echelon's multi-sensor,
  multi-agent workload
- Potential SIMD acceleration for field reads and transforms
- Evaluate GPU-side extraction if CPU becomes the bottleneck (tensors
  are already flat f32)

---

## v1.0 — API Stability

The 1.0 release freezes the public API surfaces. Target: after Echelon
has shipped its first public demo and the API has been validated by real
use.

### Stability Surfaces

The following APIs must be frozen at 1.0:

- `Propagator` trait signature
- `Space` trait and `RegionSpec` variants
- `ObsSpec` / `ObsPlan` format
- Replay binary format (wire compatibility)
- C FFI ABI version
- `RenderSpec` / `RenderPlan` format (if stabilised by then)

### What Stays Unstable

- Internal engine scheduling and arena layout (implementation details)
- Specific spatial backend implementations (new backends can be added)
- Python binding surface (follows Rust API but may lag)

---

## Post-1.0 — Platform of Choice for Gamified RL

The long-term vision is for Murk to be the default simulation engine
when researchers and developers think "I want to train agents in a
game-like environment." The post-1.0 roadmap broadens the engine from
discrete lattices to a general-purpose spatial simulation platform.

### Continuous Spaces (v1.x)

Continuous 2D/3D spatial backends for robotics, physics simulation, and
particle-based multi-agent environments. The `Space` trait is
topology-agnostic by design; continuous backends would implement it with
spatial hashing, AABB trees, or grid-based partitioning rather than
lattice indexing.

This opens Murk to:
- Robotics and locomotion (the MuJoCo/Isaac Gym audience)
- Particle-based multi-agent simulation
- Continuous-action environments with physics constraints

The discrete-lattice API must be frozen and battle-tested (via Echelon)
before this work begins. Getting the `Space` abstraction right for both
discrete and continuous topologies is the key design challenge.

### VoxelOctreeSpace (v1.5)

Hierarchical 3D spatial backend with level-of-detail. Deferred from the
original M0 plan due to complexity. Relevant for large-scale 3D
environments where Fcc12 flat addressing becomes memory-prohibitive.

Enables:
- City-scale or terrain-scale 3D environments
- Variable-resolution simulation (detailed near agents, coarse at
  distance)
- Natural fit for the LOS/occlusion system built in v0.2

### Graph/Network Topologies (v1.x)

Arbitrary graph-structured spaces for social network simulation, traffic
modelling, epidemiology. The `Space` trait is already topology-agnostic
in principle; graph backends would implement it with adjacency-list
storage.

Enables:
- Supply chain and logistics simulation
- Social network agent interaction (influence, information spread)
- Transportation network optimisation

### GPU-Accelerated Observation Extraction

Move the observation pipeline (or parts of it) onto GPU. The
architecture is pre-aligned for this: observations are already flat f32
tensors, ObsPlan is a compiled execution plan, and the arena stores
fields as contiguous f32 slices.

Enables:
- Training throughput scaling beyond CPU limits
- On-device tensor generation (skip CPU→GPU copy for PyTorch)
- Massive batched observation for population-based training

### Multi-Framework Integrations

Expand beyond Gymnasium to first-class adapters for:
- **TorchRL** (`EnvBase`) — where serious PyTorch RL practitioners live
- **RLlib** — distributed training at scale
- **CleanRL** — lightweight single-file RL implementations
- **Godot/Bevy** — game engine integrations for the render adapter

### Benchmark Suite and Research Positioning

A citable benchmark suite of 10–20 standardised environments across
Murk's supported topologies, with published baseline results:

- Reproducible comparisons against PettingZoo, MeltingPot, Gymnasium
  grid worlds
- Per-environment training curves with stable-baselines3 / CleanRL
- Position paper: "Murk: Deterministic Arena-Based Simulation for
  Reproducible Multi-Agent RL"

---

## Principles

These guide roadmap decisions:

1. **Demand-driven development**. Every feature must be justified by a
   concrete use case (Echelon first, then community demand). No
   speculative features.

2. **Engine stays an engine**. Murk is a library, not a framework.
   Echelon is built *on* Murk, not *in* Murk. The boundary must remain
   clean.

3. **Profile before optimising**. Performance work (Phase 2 arena,
   parallel propagators, GPU extraction) waits for profiling data from
   real workloads, not synthetic benchmarks.

4. **Correctness over performance**. Zero-unsafe simulation logic.
   Deterministic replay. Borrow-checker-enforced thread safety. These
   are non-negotiable.

5. **Orthogonal composition**. New features (fire, smoke, LOS) should
   compose through existing abstractions (propagators, regions, fields)
   rather than requiring engine changes. When the engine must change,
   that signals a missing abstraction.

6. **Platform ambition**. The goal is not to be a niche lattice engine
   but the platform of choice for gamified RL. Every release should
   expand the set of problems Murk can solve while maintaining the
   architectural clarity that makes it trustworthy.
