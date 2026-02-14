# Contributing to Murk

## Development environment

**Requirements:**
- Rust stable (1.75+) via [rustup](https://rustup.rs/)
- Rust nightly (for Miri only): `rustup toolchain install nightly --component miri`
- Python 3.9+
- [maturin](https://www.maturin.rs/): `pip install maturin`

**Setup:**

```bash
git clone https://github.com/tachyon-beep/murk.git
cd murk

# Build Rust workspace
cargo build --workspace

# Build Python extension (development mode)
cd crates/murk-python
maturin develop --release
cd ../..
```

## Project structure

```
murk/
├── crates/
│   ├── murk-core/          # Leaf crate: IDs, field definitions, commands, traits
│   ├── murk-arena/         # Double-buffered ping-pong arena allocator
│   ├── murk-space/         # Space trait + 7 lattice backends
│   ├── murk-propagator/    # Propagator trait, pipeline validation, step context
│   ├── murk-propagators/   # Reference propagators (diffusion, agent movement)
│   ├── murk-obs/           # Observation specification and tensor extraction
│   ├── murk-engine/        # LockstepWorld, RealtimeAsyncWorld, TickEngine
│   ├── murk-replay/        # Deterministic replay recording/verification
│   ├── murk-ffi/           # C ABI with handle tables
│   ├── murk-python/        # Python/PyO3 bindings + Gymnasium adapters
│   ├── murk-bench/         # Benchmark profiles
│   └── murk-test-utils/    # Shared test fixtures
├── examples/               # Python examples (heat_seeker, hex_pursuit, crystal_nav)
└── docs/                   # Design documents and concepts guide
```

**Dependency graph** (simplified):

```
murk-core
  ↑
murk-arena, murk-space
  ↑
murk-propagator, murk-obs
  ↑
murk-engine
  ↑
murk-ffi → murk-python
```

## Running tests

```bash
# Full workspace test suite (580+ tests)
cargo test --workspace

# Single crate
cargo test -p murk-space

# Python tests
cd crates/murk-python
pytest tests/ -v

# Memory safety (requires nightly)
cargo +nightly miri test -p murk-arena

# Clippy lints (must pass with zero warnings)
cargo clippy --workspace -- -D warnings

# Format check
cargo fmt --all -- --check
```

## Code style

### Rust

- **`#![deny(missing_docs)]`** on all crates — every public item needs a doc comment.
- **`#![forbid(unsafe_code)]`** on all crates except `murk-arena` and `murk-ffi`.
  If your change needs `unsafe`, it belongs in one of those two crates.
- **Clippy with `-D warnings`** — all clippy suggestions must be resolved.
- **`cargo fmt`** — standard rustfmt formatting, no custom config.

### Python

- Type annotations on all public functions.
- Docstrings on all public classes and methods.
- Type stubs (`.pyi`) must be updated when the Python API changes.

## CI expectations

Every push and PR triggers:

| Job | What it checks |
|-----|---------------|
| `cargo check` | Compilation across all crates |
| `cargo test` | Full test suite |
| `clippy` | Lint warnings (zero tolerance) |
| `rustfmt` | Formatting |
| `miri` | Memory safety for `murk-arena` |

All five must pass before merging.

## Adding a new space backend

Space backends implement the `Space` trait in `murk-space`. Follow the pattern
of `Square4` or `Hex2D`:

1. Create `crates/murk-space/src/your_space.rs`.
2. Implement the `Space` trait:
   - `ndim()`, `cell_count()`, `neighbours()`, `distance()`
   - `compile_region()`, `iter_region()`, `map_coord_to_tensor_index()`
   - `canonical_ordering()`, `canonical_rank()`
   - `instance_id()`
3. Add `pub mod your_space;` and `pub use your_space::YourSpace;` to `lib.rs`.
4. Run the **compliance test suite** — this is critical:

```rust
// In your_space.rs, at the bottom:
#[cfg(test)]
mod tests {
    use super::*;
    use crate::compliance::compliance_tests;

    compliance_tests!(YourSpace, || YourSpace::new(4, 4, EdgeBehavior::Absorb).unwrap());
}
```

The compliance test suite (`crates/murk-space/src/compliance.rs`) automatically
tests all Space trait invariants: canonical ordering consistency, neighbor
symmetry, distance triangle inequality, region compilation, and more. If your
backend passes compliance tests, it works with the rest of Murk.

5. Add FFI support in `murk-ffi` and Python bindings in `murk-python` if needed.

## Adding a new propagator

Propagators implement the `Propagator` trait in `murk-propagator`:

1. Create your propagator struct (must be `Send + 'static`).
2. Implement:
   - `name()` — human-readable name
   - `reads()` — fields read via in-tick overlay (Euler style)
   - `reads_previous()` — fields read from frozen tick-start (Jacobi style)
   - `writes()` — fields written, with `WriteMode::Full` or `Incremental`
   - `step(&self, ctx: &mut StepContext)` — the per-tick logic
3. Optionally implement `max_dt()` for CFL stability constraints.

See `crates/murk-engine/examples/quickstart.rs` for a complete example, or
`crates/murk-test-utils/src/fixtures.rs` for minimal test propagators.

**Key rules:**
- `step()` must be deterministic (same inputs → same outputs).
- `&self` only — propagators are stateless. All mutable state goes through fields.
- Copy read data to a local buffer before grabbing the write handle
  (split-borrow limitation in `StepContext`).

## Pull request process

1. Fork the repository and create a branch.
2. Make your changes with tests.
3. Ensure all CI checks pass locally (`cargo test --workspace && cargo clippy --workspace -- -D warnings`).
4. Open a PR with a clear description of what changed and why.

## Commit messages

This project uses [Conventional Commits](https://www.conventionalcommits.org/):

| Prefix | When to use |
|--------|------------|
| `feat:` | New feature |
| `fix:` | Bug fix |
| `docs:` | Documentation only |
| `ci:` | CI/CD changes |
| `chore:` | Maintenance (deps, config) |
| `refactor:` | Code change that neither fixes nor adds |
| `test:` | Adding or updating tests |
| `perf:` | Performance improvement |

Use a scope when helpful: `feat(space):`, `fix(python):`, `ci(release):`.

## Releasing

Releases are managed by [release-plz](https://release-plz.ieni.dev/):

1. **Automatic:** On every push to `main`, release-plz opens (or updates) a
   release PR that bumps versions and updates CHANGELOG.md based on conventional
   commits since the last release.
2. **Merge the release PR** when ready to publish.
3. **On merge:** release-plz creates git tags, which trigger the release workflow.
4. **The release workflow** publishes Rust crates to crates.io and Python wheels
   to PyPI, and creates a GitHub Release.

**Dry-run a release locally:**

```bash
cargo publish --dry-run -p murk-core
```

**Secrets required** (set in GitHub repo settings > Secrets):
- `CARGO_REGISTRY_TOKEN` — crates.io API token
- `PYPI_API_TOKEN` — PyPI API token
- `CODECOV_TOKEN` — Codecov upload token
