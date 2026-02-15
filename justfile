# Murk development task runner
# Install: cargo install just
# Usage: just <recipe>

# Default: show available recipes
default:
    @just --list

# Run cargo check + clippy + fmt check
check:
    cargo check --workspace
    cargo clippy --workspace -- -D warnings
    cargo fmt --all -- --check

# Run full test suite
test:
    cargo test --workspace

# Build and test Python extension
test-python:
    cd crates/murk-python && maturin develop --release
    cd crates/murk-python && pytest tests/ -v

# Run Miri memory safety checks (requires nightly)
miri:
    cargo +nightly miri test -p murk-arena

# Run code coverage and generate report
coverage:
    cargo tarpaulin --workspace --out html --skip-clean
    @echo "Coverage report: tarpaulin-report.html"

# Run benchmarks
bench:
    cargo bench --workspace

# Build and open documentation locally
doc:
    cargo doc --workspace --no-deps --open

# Run cargo-deny checks
deny:
    cargo deny check

# Run the full local CI suite
ci: check test deny miri
    @echo "All CI checks passed."

# Pre-commit checks (fast subset)
pre-commit: check
    @echo "Pre-commit checks passed."
