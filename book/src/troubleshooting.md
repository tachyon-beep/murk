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

Murk requires Rust 1.75 or later. Update with:

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
[determinism-catalogue.md](determinism.md) for details.
