# Troubleshooting

## Build Issues

### maturin develop fails with "pyo3 not found"

Ensure you have a compatible Python version (3.9+) and that your virtual
environment is activated:

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install maturin
cd crates/murk-python
maturin develop --release
```

### cargo build fails with MSRV error

Murk requires Rust 1.87 or later. Update with:

```bash
rustup update stable
```

### Miri fails to run

Miri requires the nightly toolchain with the miri component:

```bash
rustup toolchain install nightly --component miri
cargo +nightly miri test -p murk-arena
```

## Runtime Issues

### Python import error: "No module named murk._murk"

The native extension needs to be built first:

```bash
cd crates/murk-python
maturin develop --release
```

### Determinism test failures

Determinism tests are sensitive to floating-point ordering. Ensure you're
running on the same platform and Rust version as CI. See
[Determinism Guarantees](determinism.md) for details.

## Import Issues

### `maturin develop` succeeds but `import murk` fails

The native extension was built, but Python cannot find it. Check the
following:

1. **Virtual environment not activated.** The extension is installed
   into the virtualenv that was active during `maturin develop`. Make
   sure you activate the same environment before importing:

   ```bash
   source .venv/bin/activate
   python -c "import murk; print(murk.__version__)"
   ```

2. **Python version mismatch.** If you have multiple Python versions,
   `maturin develop` may have built against a different one. Verify:

   ```bash
   python --version          # should match the version used during build
   maturin develop --release # rebuild if in doubt
   ```

3. **Missing numpy.** Murk requires numpy >= 1.24. If numpy is not
   installed, the extension may fail to load:

   ```bash
   pip install "numpy>=1.24"
   ```

## Runtime Performance Issues

### Simulation unexpectedly slow

If step throughput is much lower than expected, check these common
causes:

1. **Debug mode.** Ensure you built with `--release`. Debug builds are
   10-50x slower:

   ```bash
   maturin develop --release    # Python
   cargo run --release           # Rust
   ```

2. **Propagator complexity.** A Python propagator that does heavy
   per-cell work will bottleneck the tick. Profile with `cProfile`
   or `py-spy` to confirm.

3. **Observation extraction frequency.** If you are calling `observe()`
   more often than you need observations, reduce the frequency. Each
   call copies data from the arena.

4. **Batched vs single env.** For RL training with many environments,
   `BatchedVecEnv` steps all worlds in a single Rust call with one GIL
   release. `MurkVecEnv` releases the GIL N times. Switching to
   `BatchedVecEnv` can yield significant speedups at scale.

## CI Failures

### Tests pass locally but fail in CI

1. **Miri nightly version.** Miri is pinned to a specific nightly
   toolchain. If CI uses a different nightly than your local machine,
   Miri behaviour may differ. Check the CI configuration for the
   expected nightly date:

   ```bash
   rustup toolchain install nightly-2025-12-01 --component miri
   cargo +nightly-2025-12-01 miri test -p murk-arena
   ```

2. **Platform differences.** Floating-point results can vary across
   operating systems and CPU architectures. Murk targets Tier B
   determinism (same build + ISA + toolchain), so cross-platform
   mismatches are expected for bitwise comparisons.

3. **Test isolation.** Some tests create temporary files or rely on
   ordering. If tests run in parallel and share state, they may fail
   non-deterministically. Use `cargo test -- --test-threads=1` to
   check for isolation issues.

## Simulation Behavior Issues

### Results don't match expectations

If the simulation produces unexpected output, check these common
causes:

1. **Determinism catalogue.** Review the
   [Determinism Guarantees](determinism.md) page. Some sources of
   non-determinism (hash ordering, threading, fast-math) are documented
   with mitigations.

2. **Propagator read/write declarations.** If a propagator declares
   `reads` (Euler) but should use `reads_previous` (Jacobi), it will
   see partially-updated values from earlier propagators in the same
   tick. Double-check the read mode for each field.

3. **Timestep too large (CFL violation).** If `dt` exceeds the CFL
   stability limit for your propagator, diffusion can blow up or
   oscillate. The engine checks topology-aware `max_dt(space)` at
   startup, but only if the propagator declares it. Reduce `dt` or add
   a `max_dt` declaration.
