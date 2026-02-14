# Murk Professionalization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add supply chain safety, packaging metadata, release automation, governance files, developer ergonomics, and a documentation site to make Murk a production-quality open-source project.

**Architecture:** Bottom-up approach. Six phases build on each other: CI hardening first (so every subsequent change goes through a stronger pipeline), then packaging metadata, release automation, governance, dev ergonomics, and finally a documentation site. Each task is a single commit.

**Tech Stack:** cargo-deny, Dependabot, cargo-tarpaulin, Codecov, GitHub Actions, release-plz, just, pre-commit, mdBook.

**Design doc:** `docs/plans/2026-02-15-professionalization-design.md`

---

## Phase 1: Supply Chain Safety & CI Hardening

### Task 1: Add deny.toml for license and advisory checking

**Files:**
- Create: `deny.toml`

**Step 1: Create deny.toml**

```toml
# cargo-deny configuration
# Run: cargo deny check
# Docs: https://embarkstudios.github.io/cargo-deny/

[advisories]
db-path = "~/.cargo/advisory-db"
db-urls = ["https://github.com/rustsec/advisory-db"]
vulnerability = "deny"
unmaintained = "warn"
yanked = "warn"
notice = "warn"

[licenses]
unlicensed = "deny"
allow = [
    "MIT",
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unicode-3.0",
    "Unicode-DFS-2016",
    "Zlib",
]
copyleft = "deny"
confidence-threshold = 0.8

[[licenses.clarify]]
name = "ring"
expression = "MIT AND ISC AND OpenSSL"
license-files = [{ path = "LICENSE", hash = 0xbd0eed23 }]

[bans]
multiple-versions = "warn"
wildcards = "allow"

[sources]
unknown-registry = "warn"
unknown-git = "warn"
allow-registry = ["https://github.com/rust-lang/crates.io-index"]
allow-git = []
```

**Step 2: Install cargo-deny locally and verify**

Run: `cargo install cargo-deny && cargo deny check`
Expected: PASS (or warnings about duplicates, no errors)

If `ring` clarify fails (hash mismatch), remove the `[[licenses.clarify]]` section — it's only needed if ring is a transitive dependency. Adjust `allow` list based on any license failures.

**Step 3: Commit**

```bash
git add deny.toml
git commit -m "ci: add deny.toml for license and vulnerability checking"
```

---

### Task 2: Add cargo-deny job to CI

**Files:**
- Modify: `.github/workflows/ci.yml`

**Step 1: Add deny job to ci.yml**

Add this job after the existing `miri` job:

```yaml
  deny:
    name: cargo deny
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@v4
      - uses: EmbarkStudios/cargo-deny-action@v2
```

**Step 2: Verify YAML is valid**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`
Expected: No error (requires PyYAML; if not available, visually verify indentation)

**Step 3: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: add cargo-deny check to CI pipeline"
```

---

### Task 3: Add concurrency groups and timeouts to CI workflows

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `.github/workflows/stress.yml`

Note: `docs.yml` and `bench.yml` already have concurrency groups.

**Step 1: Add concurrency and timeouts to ci.yml**

Add at the top level (after `env:`):

```yaml
concurrency:
  group: ci-${{ github.ref }}
  cancel-in-progress: true
```

Add `timeout-minutes: 15` to each job: `check`, `test`, `clippy`, `fmt`, `miri`, `deny`.

**Step 2: Add concurrency to stress.yml**

Add at the top level:

```yaml
concurrency:
  group: stress
  cancel-in-progress: false
```

(stress.yml already has `timeout-minutes: 30` on its job.)

**Step 3: Verify YAML validity**

Run: `python3 -c "import yaml; [yaml.safe_load(open(f'.github/workflows/{f}')) for f in ['ci.yml','stress.yml']]"`

**Step 4: Commit**

```bash
git add .github/workflows/ci.yml .github/workflows/stress.yml
git commit -m "ci: add concurrency groups and job timeouts"
```

---

### Task 4: Add Dependabot configuration

**Files:**
- Create: `.github/dependabot.yml`

**Step 1: Create dependabot.yml**

```yaml
version: 2
updates:
  - package-ecosystem: cargo
    directory: /
    schedule:
      interval: weekly
      day: monday
    open-pull-requests-limit: 5
    labels:
      - dependencies
      - rust

  - package-ecosystem: github-actions
    directory: /
    schedule:
      interval: weekly
      day: monday
    open-pull-requests-limit: 5
    labels:
      - dependencies
      - ci
```

**Step 2: Commit**

```bash
git add .github/dependabot.yml
git commit -m "ci: add Dependabot for Cargo and GitHub Actions updates"
```

---

### Task 5: Add code coverage with cargo-tarpaulin

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `README.md` (add badge)

**Step 1: Add coverage job to ci.yml**

Add this job:

```yaml
  coverage:
    name: coverage
    runs-on: ubuntu-latest
    timeout-minutes: 20
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          shared-key: coverage
      - name: Install cargo-tarpaulin
        run: cargo install cargo-tarpaulin
      - name: Run coverage
        run: cargo tarpaulin --workspace --out xml --skip-clean
      - name: Upload to Codecov
        uses: codecov/codecov-action@v4
        with:
          file: cobertura.xml
          fail_ci_if_error: false
        env:
          CODECOV_TOKEN: ${{ secrets.CODECOV_TOKEN }}
```

**Step 2: Add coverage badge to README.md**

After the existing CI and Docs badges (line 3-4 of README.md), add:

```markdown
[![codecov](https://codecov.io/gh/tachyon-beep/murk/graph/badge.svg)](https://codecov.io/gh/tachyon-beep/murk)
```

Note: The badge will show "unknown" until the first coverage run completes and the Codecov repo is set up. The owner needs to:
1. Go to https://codecov.io and sign in with GitHub
2. Add the `tachyon-beep/murk` repository
3. Copy the upload token to the repo's GitHub Secrets as `CODECOV_TOKEN`

**Step 3: Commit**

```bash
git add .github/workflows/ci.yml README.md
git commit -m "ci: add code coverage with cargo-tarpaulin and Codecov"
```

---

## Phase 2: Packaging Metadata

### Task 6: Enrich workspace Cargo.toml metadata

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add metadata to [workspace.package]**

The current `[workspace.package]` section (lines 18-23 of `Cargo.toml`) should become:

```toml
[workspace.package]
version = "0.1.0"
edition = "2021"
license = "MIT"
repository = "https://github.com/tachyon-beep/murk"
authors = ["John Morrissey"]
homepage = "https://github.com/tachyon-beep/murk"
documentation = "https://tachyon-beep.github.io/murk/"
keywords = ["simulation", "reinforcement-learning", "arena", "gymnasium", "tick-based"]
categories = ["simulation", "science", "game-development"]
rust-version = "1.75"
```

**Step 2: Verify workspace builds**

Run: `cargo check --workspace`
Expected: PASS

**Step 3: Commit**

```bash
git add Cargo.toml
git commit -m "chore: add homepage, keywords, categories, and MSRV to workspace metadata"
```

---

### Task 7: Mark internal crates as publish = false

**Files:**
- Modify: `crates/murk-test-utils/Cargo.toml`
- Modify: `crates/murk-bench/Cargo.toml`
- Modify: `crates/murk-python/Cargo.toml`

**Step 1: Add publish = false to each crate**

In each of these three files, add `publish = false` under the `[package]` section, after the `description` line:

```toml
publish = false
```

**Step 2: Verify workspace builds**

Run: `cargo check --workspace`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/murk-test-utils/Cargo.toml crates/murk-bench/Cargo.toml crates/murk-python/Cargo.toml
git commit -m "chore: mark internal crates as publish = false"
```

---

### Task 8: Enrich pyproject.toml

**Files:**
- Modify: `crates/murk-python/pyproject.toml`

**Step 1: Replace pyproject.toml content**

The file should become:

```toml
[build-system]
requires = ["maturin>=1.0,<2.0"]
build-backend = "maturin"

[project]
name = "murk"
version = "0.1.0"
description = "Python bindings for the Murk simulation framework"
requires-python = ">=3.9"
license = {text = "MIT"}
authors = [{name = "John Morrissey"}]
readme = "../../README.md"
classifiers = [
    "Development Status :: 4 - Beta",
    "License :: OSI Approved :: MIT License",
    "Programming Language :: Rust",
    "Programming Language :: Python :: 3",
    "Programming Language :: Python :: 3.9",
    "Programming Language :: Python :: 3.10",
    "Programming Language :: Python :: 3.11",
    "Programming Language :: Python :: 3.12",
    "Programming Language :: Python :: 3.13",
    "Topic :: Scientific/Engineering :: Artificial Intelligence",
    "Topic :: Scientific/Engineering :: Physics",
    "Typing :: Typed",
]
dependencies = [
    "numpy>=1.24",
    "gymnasium>=0.29",
]

[project.optional-dependencies]
test = [
    "pytest>=7.0",
    "stable-baselines3>=2.0",
]
dev = [
    "pytest>=7.0",
    "ruff>=0.1",
    "mypy>=1.0",
]

[project.urls]
Homepage = "https://github.com/tachyon-beep/murk"
Documentation = "https://tachyon-beep.github.io/murk/"
Repository = "https://github.com/tachyon-beep/murk"
Issues = "https://github.com/tachyon-beep/murk/issues"

[tool.maturin]
python-source = "python"
module-name = "murk._murk"
features = ["pyo3/extension-module"]
```

**Step 2: Verify maturin develop still works**

Run: `cd crates/murk-python && maturin develop --release && cd ../..`
Expected: Build succeeds

**Step 3: Commit**

```bash
git add crates/murk-python/pyproject.toml
git commit -m "chore: enrich pyproject.toml with classifiers, URLs, and license"
```

---

### Task 9: Add inherited metadata to publishable crates

**Files:**
- Modify: Each publishable crate's `Cargo.toml` (murk-core, murk-arena, murk-space, murk-propagator, murk-obs, murk-engine, murk-replay, murk-ffi, murk-propagators)

**Step 1: Ensure each publishable crate inherits workspace metadata**

Check each crate's `Cargo.toml`. They should all have these lines under `[package]`:

```toml
homepage.workspace = true
documentation.workspace = true
keywords.workspace = true
categories.workspace = true
rust-version.workspace = true
```

Most crates already inherit `version`, `edition`, `license`, `repository`, `authors`. Add the five new fields to each.

**Step 2: Verify**

Run: `cargo check --workspace`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/*/Cargo.toml
git commit -m "chore: inherit homepage, docs, keywords, categories, MSRV in all publishable crates"
```

---

## Phase 3: Release Automation

### Task 10: Add release workflow

**Files:**
- Create: `.github/workflows/release.yml`

**Step 1: Create release.yml**

```yaml
name: Release

on:
  push:
    tags:
      - 'v[0-9]+.*'

env:
  CARGO_TERM_COLOR: always

permissions:
  contents: write

jobs:
  ci:
    name: CI checks
    uses: ./.github/workflows/ci.yml

  release:
    name: Create GitHub Release
    needs: ci
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Extract version from tag
        id: version
        run: echo "version=${GITHUB_REF_NAME#v}" >> "$GITHUB_OUTPUT"
      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          generate_release_notes: true
          draft: false

  publish-crates:
    name: Publish to crates.io
    needs: [ci, release]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Publish crates in dependency order
        env:
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
        run: |
          set -e
          for crate in murk-core murk-arena murk-space murk-propagator murk-obs \
                       murk-engine murk-replay murk-ffi murk-propagators; do
            echo "Publishing $crate..."
            cargo publish -p "$crate" --no-verify
            sleep 30  # Wait for crates.io index update
          done

  publish-pypi:
    name: Publish to PyPI
    needs: [ci, release]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: actions/setup-python@v5
        with:
          python-version: '3.12'
      - name: Install maturin
        run: pip install maturin
      - name: Build wheel
        working-directory: crates/murk-python
        run: maturin build --release
      - name: Publish to PyPI
        uses: pypa/gh-action-pypi-publish@release/v1
        with:
          packages-dir: crates/murk-python/target/wheels/
          password: ${{ secrets.PYPI_API_TOKEN }}
```

**Step 2: Verify YAML validity**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))"`

**Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add release workflow for crates.io and PyPI publishing"
```

---

### Task 11: Add release-plz configuration

**Files:**
- Create: `release-plz.toml`
- Create: `.github/workflows/release-plz.yml`

**Step 1: Create release-plz.toml**

```toml
[workspace]
changelog_update = true
git_tag_enable = true
publish_allow_dirty = false

# Don't publish internal crates
[[package]]
name = "murk-test-utils"
publish = false
changelog_update = false
git_tag_enable = false

[[package]]
name = "murk-bench"
publish = false
changelog_update = false
git_tag_enable = false

[[package]]
name = "murk-python"
publish = false
changelog_update = false
git_tag_enable = false
```

**Step 2: Create release-plz workflow**

```yaml
name: Release-plz

on:
  push:
    branches:
      - main

permissions:
  pull-requests: write
  contents: write

jobs:
  release-plz:
    name: Release-plz
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0
      - uses: dtolnay/rust-toolchain@stable
      - name: Run release-plz
        uses: MarcoIeni/release-plz-action@v0.5
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          CARGO_REGISTRY_TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
```

**Step 3: Commit**

```bash
git add release-plz.toml .github/workflows/release-plz.yml
git commit -m "ci: add release-plz for automated release PRs and changelog updates"
```

---

### Task 12: Document release process in CONTRIBUTING.md

**Files:**
- Modify: `CONTRIBUTING.md`

**Step 1: Add Releasing section**

Append before the end of the file (after "Pull request process" section):

```markdown
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

**Secrets required** (set in GitHub repo settings → Secrets):
- `CARGO_REGISTRY_TOKEN` — crates.io API token
- `PYPI_API_TOKEN` — PyPI API token
- `CODECOV_TOKEN` — Codecov upload token
```

**Step 2: Commit**

```bash
git add CONTRIBUTING.md
git commit -m "docs: add commit conventions and release process to CONTRIBUTING.md"
```

---

## Phase 4: Governance & Community Files

### Task 13: Add CODE_OF_CONDUCT.md

**Files:**
- Create: `CODE_OF_CONDUCT.md`

**Step 1: Create CODE_OF_CONDUCT.md**

Use the Contributor Covenant v2.1. The full text is available at https://www.contributor-covenant.org/version/2/1/code_of_conduct/. Create the file with:
- Standard Contributor Covenant v2.1 text
- Enforcement contact: the project maintainer's preferred contact method

**Step 2: Commit**

```bash
git add CODE_OF_CONDUCT.md
git commit -m "docs: add Contributor Covenant Code of Conduct"
```

---

### Task 14: Add SECURITY.md

**Files:**
- Create: `SECURITY.md`

**Step 1: Create SECURITY.md**

```markdown
# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes      |

## Reporting a Vulnerability

**Please do not open a public GitHub issue for security vulnerabilities.**

Instead, use one of these methods:

1. **GitHub Security Advisories** (preferred): Go to the
   [Security tab](https://github.com/tachyon-beep/murk/security/advisories/new)
   and create a new private advisory.
2. **Email:** Contact the maintainer directly.

### What to include

- Description of the vulnerability
- Steps to reproduce
- Affected versions
- Potential impact

### What to expect

- **Acknowledgment** within 48 hours
- **Assessment** within 1 week
- **Fix or mitigation** as soon as practical, typically within 30 days
- Credit in the release notes (unless you prefer to remain anonymous)

## Security Practices

- `#![forbid(unsafe_code)]` on all crates except `murk-arena` and `murk-ffi`
- Miri (memory safety verification) runs on every push via CI
- `cargo-deny` checks for known vulnerabilities in dependencies
- Dependabot monitors for dependency security updates
```

**Step 2: Commit**

```bash
git add SECURITY.md
git commit -m "docs: add security policy (SECURITY.md)"
```

---

### Task 15: Add GitHub issue templates

**Files:**
- Create: `.github/ISSUE_TEMPLATE/bug_report.yml`
- Create: `.github/ISSUE_TEMPLATE/feature_request.yml`
- Create: `.github/ISSUE_TEMPLATE/config.yml`

**Step 1: Create bug_report.yml**

```yaml
name: Bug Report
description: Report a bug in Murk
labels: [bug]
body:
  - type: markdown
    attributes:
      value: |
        Thank you for reporting a bug. Please fill out the sections below.
  - type: textarea
    id: description
    attributes:
      label: Description
      description: A clear description of what the bug is.
    validations:
      required: true
  - type: textarea
    id: reproduce
    attributes:
      label: Steps to Reproduce
      description: Minimal code or steps to reproduce the behavior.
      placeholder: |
        1. Create a config with ...
        2. Call world.step_sync(...)
        3. Observe ...
    validations:
      required: true
  - type: textarea
    id: expected
    attributes:
      label: Expected Behavior
      description: What you expected to happen.
    validations:
      required: true
  - type: textarea
    id: actual
    attributes:
      label: Actual Behavior
      description: What actually happened. Include error messages if applicable.
    validations:
      required: true
  - type: input
    id: version
    attributes:
      label: Murk Version
      description: Output of `cargo pkgid murk-core` or `pip show murk`
      placeholder: "0.1.0"
    validations:
      required: true
  - type: dropdown
    id: api
    attributes:
      label: API
      options:
        - Rust
        - Python
        - C FFI
        - Other
    validations:
      required: true
  - type: textarea
    id: environment
    attributes:
      label: Environment
      description: OS, Rust version, Python version, etc.
      placeholder: |
        - OS: Ubuntu 24.04
        - Rust: 1.78.0
        - Python: 3.12.3
```

**Step 2: Create feature_request.yml**

```yaml
name: Feature Request
description: Suggest a new feature or enhancement
labels: [enhancement]
body:
  - type: markdown
    attributes:
      value: |
        Thank you for your suggestion. Please describe the feature you'd like.
  - type: textarea
    id: problem
    attributes:
      label: Problem Statement
      description: What problem does this feature solve? What are you trying to do?
    validations:
      required: true
  - type: textarea
    id: solution
    attributes:
      label: Proposed Solution
      description: How do you think this should work?
    validations:
      required: true
  - type: textarea
    id: alternatives
    attributes:
      label: Alternatives Considered
      description: Any alternative approaches you've thought about.
  - type: dropdown
    id: area
    attributes:
      label: Area
      options:
        - Spatial backends (murk-space)
        - Propagator pipeline (murk-propagator)
        - Observation system (murk-obs)
        - Engine (murk-engine)
        - Python bindings (murk-python)
        - C FFI (murk-ffi)
        - Replay system (murk-replay)
        - Documentation
        - Other
    validations:
      required: true
```

**Step 3: Create config.yml**

```yaml
blank_issues_enabled: false
contact_links:
  - name: Question or Discussion
    url: https://github.com/tachyon-beep/murk/discussions
    about: Use Discussions for questions, ideas, and general conversation.
```

Note: This requires GitHub Discussions to be enabled on the repository (Settings → General → Features → Discussions). If Discussions are not enabled, change `blank_issues_enabled` to `true` and remove the `contact_links` section.

**Step 4: Commit**

```bash
git add .github/ISSUE_TEMPLATE/
git commit -m "docs: add GitHub issue templates for bugs and feature requests"
```

---

### Task 16: Add requirements.txt to hex_pursuit example

**Files:**
- Create: `examples/hex_pursuit/requirements.txt`

**Step 1: Create requirements.txt**

Check the other examples for the pattern:

```
numpy>=1.24
gymnasium>=0.29
stable-baselines3>=2.0
```

**Step 2: Commit**

```bash
git add examples/hex_pursuit/requirements.txt
git commit -m "docs: add missing requirements.txt to hex_pursuit example"
```

---

## Phase 5: Developer Ergonomics

### Task 17: Add justfile

**Files:**
- Create: `justfile`

**Step 1: Create justfile**

```just
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
```

**Step 2: Verify justfile parses**

Run: `just --list`
Expected: Lists all recipes without error. If `just` is not installed, run `cargo install just` first.

**Step 3: Add justfile to .gitignore exclusion check**

Verify `justfile` is not in `.gitignore` (it shouldn't be — current gitignore only excludes build artifacts and IDE files).

**Step 4: Commit**

```bash
git add justfile
git commit -m "chore: add justfile for common development tasks"
```

---

### Task 18: Add .editorconfig

**Files:**
- Create: `.editorconfig`

**Step 1: Create .editorconfig**

```ini
root = true

[*]
charset = utf-8
end_of_line = lf
insert_final_newline = true
trim_trailing_whitespace = true

[*.rs]
indent_style = space
indent_size = 4

[*.py]
indent_style = space
indent_size = 4

[*.toml]
indent_style = space
indent_size = 4

[*.yml]
indent_style = space
indent_size = 2

[*.yaml]
indent_style = space
indent_size = 2

[*.md]
trim_trailing_whitespace = false

[Makefile]
indent_style = tab

[justfile]
indent_style = space
indent_size = 4
```

**Step 2: Commit**

```bash
git add .editorconfig
git commit -m "chore: add .editorconfig for consistent editor settings"
```

---

### Task 19: Add pre-commit configuration

**Files:**
- Create: `.pre-commit-config.yaml`

**Step 1: Create .pre-commit-config.yaml**

```yaml
repos:
  - repo: https://github.com/pre-commit/pre-commit-hooks
    rev: v4.6.0
    hooks:
      - id: trailing-whitespace
        exclude: '\.md$'
      - id: end-of-file-fixer
      - id: check-yaml
      - id: check-toml
      - id: check-added-large-files
        args: ['--maxkb=500']
      - id: check-merge-conflict

  - repo: local
    hooks:
      - id: cargo-fmt
        name: cargo fmt
        entry: cargo fmt --all -- --check
        language: system
        types: [rust]
        pass_filenames: false
```

**Step 2: Commit**

```bash
git add .pre-commit-config.yaml
git commit -m "chore: add pre-commit configuration"
```

Note: Users install with `pip install pre-commit && pre-commit install`. This is optional — the justfile `pre-commit` recipe provides the same fast checks without requiring the pre-commit tool.

---

## Phase 6: Documentation Site

### Task 20: Set up mdBook structure

**Files:**
- Create: `book.toml`
- Create: `book/src/SUMMARY.md`
- Create: `book/src/introduction.md`
- Create: `book/src/getting-started.md`
- Create: `book/src/troubleshooting.md`

**Step 1: Create book.toml**

```toml
[book]
title = "Murk Documentation"
authors = ["John Morrissey"]
language = "en"
src = "book/src"

[build]
build-dir = "book/build"

[output.html]
git-repository-url = "https://github.com/tachyon-beep/murk"
edit-url-template = "https://github.com/tachyon-beep/murk/edit/main/{path}"
```

**Step 2: Create SUMMARY.md**

```markdown
# Summary

[Introduction](introduction.md)

# User Guide

- [Getting Started](getting-started.md)
- [Concepts](concepts.md)
- [Examples](examples.md)

# Reference

- [Error Reference](error-reference.md)
- [Replay Format](replay-format.md)
- [Determinism Guarantees](determinism.md)
- [API Reference (rustdoc)](https://tachyon-beep.github.io/murk/api/)

# Contributing

- [Development Guide](contributing.md)
- [Architecture](architecture.md)

# Appendix

- [Troubleshooting](troubleshooting.md)
```

**Step 3: Create introduction.md**

Adapt from README.md — project description, features list, architecture diagram. Keep it concise; link to Getting Started for setup.

**Step 4: Create getting-started.md**

Combine the README quick start section with more detail:
- Prerequisites
- Installation (from source, eventually from PyPI/crates.io)
- First Rust simulation (link to quickstart.rs)
- First Python simulation (minimal example)
- Next steps (link to Concepts)

**Step 5: Create symlinks or copies for existing docs**

For docs that already exist, use mdBook's ability to reference files outside `src/`:

In `book.toml`, add preprocessor to handle paths, or create thin wrapper files in `book/src/` that include the original content:

```markdown
<!-- book/src/concepts.md -->
{{#include ../../docs/CONCEPTS.md}}
```

```markdown
<!-- book/src/error-reference.md -->
{{#include ../../docs/error-reference.md}}
```

```markdown
<!-- book/src/replay-format.md -->
{{#include ../../docs/replay-format.md}}
```

```markdown
<!-- book/src/determinism.md -->
{{#include ../../docs/determinism-catalogue.md}}
```

```markdown
<!-- book/src/contributing.md -->
{{#include ../../CONTRIBUTING.md}}
```

```markdown
<!-- book/src/architecture.md -->
This section covers Murk's high-level architecture. For the full
design document, see [HLD.md](https://github.com/tachyon-beep/murk/blob/main/docs/HLD.md).

{{#include ../../docs/DESIGN.md}}
```

**Step 6: Create troubleshooting.md**

```markdown
# Troubleshooting

## Build Issues

### maturin develop fails with "pyo3 not found"

Ensure you have a compatible Python version (3.9+) and that your virtual
environment is activated:

\```bash
python3 -m venv .venv
source .venv/bin/activate
pip install maturin
cd crates/murk-python
maturin develop --release
\```

### cargo build fails with MSRV error

Murk requires Rust 1.75 or later. Update with:

\```bash
rustup update stable
\```

### Miri fails to run

Miri requires the nightly toolchain with the miri component:

\```bash
rustup toolchain install nightly --component miri
cargo +nightly miri test -p murk-arena
\```

## Runtime Issues

### Python import error: "No module named murk._murk"

The native extension needs to be built first:

\```bash
cd crates/murk-python
maturin develop --release
\```

### Determinism test failures

Determinism tests are sensitive to floating-point ordering. Ensure you're
running on the same platform and Rust version as CI. See
[determinism-catalogue.md](determinism.md) for details.
```

**Step 7: Create examples.md**

```markdown
# Examples

Murk ships with three Python example projects demonstrating different
spatial backends and RL integration patterns.

| Example | Space | Demonstrates |
|---------|-------|-------------|
| [heat_seeker](https://github.com/tachyon-beep/murk/tree/main/examples/heat_seeker) | Square4 | PPO RL, diffusion physics, Python propagator |
| [hex_pursuit](https://github.com/tachyon-beep/murk/tree/main/examples/hex_pursuit) | Hex2D | Multi-agent, AgentDisk foveation |
| [crystal_nav](https://github.com/tachyon-beep/murk/tree/main/examples/crystal_nav) | Fcc12 | 3D lattice navigation |

There is also a Rust example:

| Example | Demonstrates |
|---------|-------------|
| [quickstart.rs](https://github.com/tachyon-beep/murk/tree/main/crates/murk-engine/examples/quickstart.rs) | Rust API: config, propagator, commands, snapshots |

## Running the Python examples

\```bash
# Install murk first
cd crates/murk-python && maturin develop --release && cd ../..

# Run an example
cd examples/heat_seeker
pip install -r requirements.txt
python heat_seeker.py
\```
```

**Step 8: Verify mdbook builds**

Run: `mdbook build` (install with `cargo install mdbook` if needed)
Expected: Book builds to `book/build/` without errors

**Step 9: Add book/build/ to .gitignore**

Add to `.gitignore`:

```
# mdBook build output
book/build/
```

**Step 10: Commit**

```bash
git add book.toml book/ .gitignore
git commit -m "docs: set up mdBook documentation site"
```

---

### Task 21: Deploy mdBook via GitHub Pages

**Files:**
- Modify: `.github/workflows/docs.yml`

**Step 1: Update docs.yml to build mdBook + rustdoc**

Replace the docs workflow with a version that:
1. Builds rustdoc into `target/doc/`
2. Builds mdBook into `book/build/`
3. Copies rustdoc into `book/build/api/` so it's served as a subdirectory
4. Deploys `book/build/` as the GitHub Pages site

The updated `docs` job steps should be:

```yaml
  docs:
    name: build docs
    runs-on: ubuntu-latest
    timeout-minutes: 15
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with:
          shared-key: docs

      # Build rustdoc
      - run: cargo doc --workspace --no-deps

      # Build mdBook
      - name: Install mdbook
        run: cargo install mdbook
      - name: Build mdbook
        run: mdbook build

      # Combine: rustdoc as /api/ subdirectory of mdBook
      - name: Combine docs
        run: |
          cp -r target/doc book/build/api
          echo '<meta http-equiv="refresh" content="0; url=api/murk_core/index.html">' > book/build/api/index.html

      # Deploy (only on main push)
      - name: Upload Pages artifact
        if: github.ref == 'refs/heads/main' && github.event_name == 'push'
        uses: actions/upload-pages-artifact@v3
        with:
          path: book/build
```

The `deploy` job stays the same.

**Step 2: Update README rustdoc link**

In README.md, update the API reference link to point to the new path:

```markdown
- **[API Reference (rustdoc)](https://tachyon-beep.github.io/murk/api/)** — auto-published on every push to `main`
```

And update the redirect in the docs badge from the rustdoc root to the mdBook root:

```markdown
[![Docs](https://github.com/tachyon-beep/murk/actions/workflows/docs.yml/badge.svg)](https://tachyon-beep.github.io/murk/)
```

(The badge URL already points to the right place — `https://tachyon-beep.github.io/murk/` — which will now be the mdBook landing page instead of the rustdoc redirect.)

**Step 3: Commit**

```bash
git add .github/workflows/docs.yml README.md
git commit -m "ci: deploy mdBook + rustdoc to GitHub Pages"
```

---

### Task 22: Final verification and push

**Step 1: Run the full local CI suite**

```bash
just ci
```

Or manually:

```bash
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
cargo test --workspace
cargo deny check
cargo +nightly miri test -p murk-arena
```

Expected: All pass.

**Step 2: Verify mdBook builds**

```bash
mdbook build
```

Expected: Builds without errors.

**Step 3: Review all changes**

```bash
git log --oneline main~22..HEAD
```

Expected: ~22 clean commits, one per task.

**Step 4: Push**

```bash
git push
```
