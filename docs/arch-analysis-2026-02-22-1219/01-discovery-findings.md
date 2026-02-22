# Discovery Findings

## Directory Structure (Top-Level)

- `crates/`: Rust workspace crates (core engine + bindings)
- `examples/`: end-to-end example projects (mostly Python RL training loops)
- `docs/`: design docs (architecture, concepts, determinism, replay, error reference, bug writeups)
- `book/`: mdBook wrapper around `docs/` + built output under `book/build/`
- `scripts/`: repo automation helpers
- `tests/`: integration/determinism/stress scaffolding (plus crate-local test suites under `crates/*`)
- `.github/workflows/`: CI, release, docs, bench, stress automation
- `justfile`: local task runner mirroring CI (check/test/deny/miri/python)

## Technology Stack

- Rust 2021 workspace (`Cargo.toml`) with MSRV pinned to 1.87 (CI `msrv` job).
- Python bindings via PyO3 + maturin (`crates/murk-python`), configured for `abi3-py312` and `requires-python = ">=3.12"`.
- C ABI exposed by `murk-ffi` with explicit ABI versioning and handle tables.

## Major Subsystems (From Public Docs + Workspace Layout)

- **Facade crate**: the public Rust entrypoint that re-exports sub-crates (`crates/murk`)
- **Core types**: IDs, field defs, commands, errors (`crates/murk-core`)
- **Arena allocator**: ping-pong generations + sparse CoW slab (`crates/murk-arena`)
- **Space/topology**: lattice backends + region planning (`crates/murk-space`)
- **Propagator model**: stateless operators + pipeline validation (`crates/murk-propagator`, `crates/murk-propagators`)
- **Observation pipeline**: `ObsSpec -> ObsPlan -> execute` into flat tensors (`crates/murk-obs`)
- **Engine runtimes**: lockstep + realtime async + batched stepping (`crates/murk-engine`)
- **Replay**: binary wire format + determinism verification (`crates/murk-replay`)
- **Bindings**: C ABI + PyO3/Gymnasium adapters (`crates/murk-ffi`, `crates/murk-python`)

## Delivery/Quality Signals

- Strong CI coverage: `cargo check`, MSRV check, multi-OS `cargo test`, `clippy -D warnings`, `rustfmt --check`, `miri` (arena), `cargo-deny`, Python tests + example smoke tests, and tarpaulin coverage upload (`.github/workflows/ci.yml`).
- Weekly scheduled stress tests (`.github/workflows/stress.yml`) and nightly benchmark regression tracking to `gh-pages` (`.github/workflows/bench.yml`).
- Security hygiene present: supported versions policy, private vuln reporting, Miri + cargo-deny + Dependabot (`SECURITY.md`).

## Inconsistencies / Fast-Follow Doc Fixes

- Python minimum version is documented as 3.9+ in some docs, but the packaging requires Python 3.12+ (`crates/murk-python/pyproject.toml`).
- “Published vs install from source” guidance diverges between different docs (root README / mdBook Getting Started / release workflows). Aligning these would reduce contributor friction and user confusion.
