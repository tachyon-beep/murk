# Professionalization Design

Date: 2026-02-15
Status: Approved

## Context

Murk v0.1.0 is a well-engineered simulation engine with strong internals
(580+ tests, `#![deny(missing_docs)]`, Miri verification, excellent
documentation). However, it lacks the packaging, tooling, and governance
infrastructure expected of a production open-source project. This design
addresses those gaps across six work areas.

## Approach

Bottom-up infrastructure first (Approach A): harden CI and supply chain
safety, then layer packaging metadata, release automation, governance
files, developer ergonomics, and a documentation site on top. Each layer
builds on the previous one.

---

## Section 1: Supply Chain Safety & CI Hardening

### 1.1 Add deny.toml

Create `deny.toml` at workspace root:
- Ban licenses incompatible with MIT (GPL, AGPL, SSPL, etc.)
- Enable advisory-db checks (RustSec) for known vulnerabilities
- Detect duplicate dependency versions
- Allow-list specific exceptions as needed

### 1.2 Add cargo-deny to CI

Add `cargo deny check` step to `ci.yml` running on every push/PR.

### 1.3 Add Dependabot

Create `.github/dependabot.yml` covering:
- `cargo` ecosystem (weekly schedule)
- `github-actions` ecosystem (weekly schedule)

### 1.4 Add code coverage

Add a CI job using `cargo-tarpaulin` or `cargo-llvm-cov`:
- Generate lcov/cobertura report
- Upload to Codecov
- Add coverage badge to README

### 1.5 Add concurrency groups

Add `concurrency` blocks to all four workflows (ci, docs, bench, stress)
to cancel duplicate runs on the same branch.

### 1.6 Add job timeouts

Add `timeout-minutes` to all CI jobs (ci: 15, docs: 10, bench: 30,
stress: already has 30).

---

## Section 2: Packaging Metadata

### 2.1 Enrich workspace Cargo.toml

Add to `[workspace.package]`:
- `homepage = "https://github.com/tachyon-beep/murk"`
- `documentation = "https://tachyon-beep.github.io/murk/"`
- `keywords = ["simulation", "reinforcement-learning", "arena", "gymnasium", "tick-based"]`
- `categories = ["simulation", "science", "game-development"]`
- `rust-version = "1.75"`

### 2.2 Mark internal crates publish = false

Add `publish = false` to:
- murk-test-utils
- murk-bench
- murk-python (published via PyPI, not crates.io)

### 2.3 Enrich pyproject.toml

Add to `[project]`:
- `license = {text = "MIT"}`
- `authors = [{name = "John Morrissey"}]`
- `readme = "../../README.md"`
- `classifiers` (Development Status :: 4, License :: OSI Approved :: MIT,
  Programming Language :: Rust, Programming Language :: Python :: 3,
  Topic :: Scientific/Engineering :: Artificial Intelligence)
- `[project.urls]` (Homepage, Documentation, Repository, Issues)
- `[project.optional-dependencies]` dev = ["pytest", "ruff", "mypy"]

### 2.4 Verify cargo publish --dry-run

Run `cargo publish --dry-run` for each publishable crate in dependency
order to catch packaging issues before real publication.

---

## Section 3: Release Automation

### 3.1 Add release workflow

Create `.github/workflows/release.yml` triggered by `v*` tags:
- Run full CI checks
- Create GitHub Release with notes from CHANGELOG
- Publish Rust crates to crates.io in dependency order:
  murk-core -> murk-arena -> murk-space -> murk-propagator -> murk-obs
  -> murk-engine -> murk-replay -> murk-ffi -> murk-propagators
- Build Python wheels via maturin and publish to PyPI

### 3.2 Add release-plz config

Configure release-plz for automated:
- Version bumping across workspace
- CHANGELOG updates from conventional commits
- Release PR creation

### 3.3 Document release process

Add "Releasing" section to CONTRIBUTING.md explaining:
- How to cut a release (tag push triggers workflow)
- What gets published where
- How to do a dry-run release

---

## Section 4: Governance & Community Files

### 4.1 CODE_OF_CONDUCT.md

Adopt Contributor Covenant v2.1 with enforcement contact.

### 4.2 SECURITY.md

Standard security policy:
- Private disclosure via GitHub Security Advisories or email
- Supported versions (0.1.x)
- Disclosure timeline

### 4.3 Issue templates

Create `.github/ISSUE_TEMPLATE/`:
- `bug_report.yml` (structured form)
- `feature_request.yml` (structured form)
- `config.yml` (link to Discussions for questions)

### 4.4 Commit conventions

Add "Commit Messages" section to CONTRIBUTING.md documenting the
Conventional Commits format already in use (feat, fix, ci, docs, etc.).

### 4.5 Fix hex_pursuit consistency

Add `requirements.txt` to `examples/hex_pursuit/` matching the pattern
of the other two examples.

---

## Section 5: Developer Ergonomics

### 5.1 Add justfile

Create `justfile` at workspace root with recipes:
- `check`: cargo check + clippy + fmt --check
- `test`: cargo test --workspace
- `test-python`: maturin develop --release + pytest
- `miri`: cargo +nightly miri test -p murk-arena
- `coverage`: run coverage tool and open report
- `bench`: cargo bench --workspace
- `doc`: cargo doc --workspace --no-deps --open
- `deny`: cargo deny check
- `ci`: run full local CI suite

### 5.2 Add pre-commit hooks

Create `.pre-commit-config.yaml`:
- cargo fmt --check
- Trailing whitespace / end-of-file fixers
- Large file detection

Alternatively: lighter approach with `just pre-commit` recipe.

### 5.3 Add .editorconfig

Standard `.editorconfig`:
- UTF-8 encoding
- LF line endings
- 4-space indent for Rust, Python
- Trim trailing whitespace
- Insert final newline

---

## Section 6: Documentation Site

### 6.1 Set up mdBook

Create mdBook at repo root (`book.toml` + `book/src/`):
- Introduction (from README)
- Getting Started (install, quick start, first simulation)
- Concepts (from docs/CONCEPTS.md)
- User Guide (Python tutorial, Rust tutorial)
- Reference (links to rustdoc, error reference)
- Design (links to HLD, design decisions)
- Examples (walkthrough with expected output)
- Troubleshooting

### 6.2 Deploy via GitHub Pages

Update `docs.yml` to build mdBook + rustdoc together. mdBook as landing
page, rustdoc linked from within.

### 6.3 Add troubleshooting content

Common issues: maturin build failures, Python version compatibility,
Miri setup on nightly.

---

## Execution Order

| Phase | Section | Dependencies |
|-------|---------|-------------|
| 1 | Supply Chain Safety & CI Hardening | None |
| 2 | Packaging Metadata | Phase 1 (deny.toml validates licenses) |
| 3 | Release Automation | Phase 2 (metadata must be correct) |
| 4 | Governance & Community | None (can parallel with 2-3) |
| 5 | Developer Ergonomics | Phase 1 (justfile wraps deny, coverage) |
| 6 | Documentation Site | Phase 4 (governance files linked from docs) |

Phases 4 and 5 can run in parallel with phases 2-3.
