#!/usr/bin/env python3
"""Check user-facing docs consistency across README/book/changelog surfaces."""

from __future__ import annotations

import pathlib
import re
import sys


def extract_python_floor(pyproject: str) -> str:
    match = re.search(r'requires-python\s*=\s*">=\s*([0-9]+\.[0-9]+)"', pyproject)
    if not match:
        raise ValueError("Could not parse requires-python from crates/murk-python/pyproject.toml")
    return match.group(1)


def extract_workspace_version(cargo_toml: str) -> str:
    match = re.search(
        r"\[workspace\.package\][\s\S]*?^version\s*=\s*\"([0-9]+\.[0-9]+\.[0-9]+)\"",
        cargo_toml,
        flags=re.MULTILINE,
    )
    if not match:
        raise ValueError("Could not parse workspace.package version from Cargo.toml")
    return match.group(1)


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent.parent
    cargo_toml = (root / "Cargo.toml").read_text(encoding="utf-8")
    pyproject = (root / "crates/murk-python/pyproject.toml").read_text(encoding="utf-8")
    readme = (root / "README.md").read_text(encoding="utf-8")
    getting_started = (root / "book/src/getting-started.md").read_text(encoding="utf-8")
    changelog = (root / "CHANGELOG.md").read_text(encoding="utf-8")

    py_floor = extract_python_floor(pyproject)
    workspace_version = extract_workspace_version(cargo_toml)
    expected_phrase = f"Python {py_floor}+"

    errors: list[str] = []

    if expected_phrase not in readme:
        errors.append(
            f"README.md missing expected Python floor phrase '{expected_phrase}'"
        )
    if expected_phrase not in getting_started:
        errors.append(
            f"book/src/getting-started.md missing expected Python floor phrase '{expected_phrase}'"
        )

    if "python -m pip install murk" not in readme:
        errors.append("README.md missing published install command 'python -m pip install murk'")
    if "python -m pip install murk" not in getting_started:
        errors.append(
            "book/src/getting-started.md missing published install command 'python -m pip install murk'"
        )

    if "maturin develop --release" not in readme:
        errors.append("README.md missing source-build command 'maturin develop --release'")
    if "maturin develop --release" not in getting_started:
        errors.append(
            "book/src/getting-started.md missing source-build command 'maturin develop --release'"
        )

    if "CHANGELOG.md" not in readme:
        errors.append("README.md missing link to CHANGELOG.md")

    if "## [Unreleased]" not in changelog:
        errors.append("CHANGELOG.md missing required '## [Unreleased]' section")

    release_heading_published = (
        rf"^## \[{re.escape(workspace_version)}\]\s+-\s+[0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}}"
    )
    release_heading_unreleased = (
        rf"^## \[{re.escape(workspace_version)}\]\s+-\s+[Uu]nreleased"
    )
    if not (
        re.search(release_heading_published, changelog, flags=re.MULTILINE)
        or re.search(release_heading_unreleased, changelog, flags=re.MULTILINE)
    ):
        errors.append(
            f"CHANGELOG.md missing heading for workspace version {workspace_version} "
            "(expected dated release or 'Unreleased')"
        )

    if "Keep a Changelog" not in changelog:
        errors.append("CHANGELOG.md missing Keep a Changelog format attribution")

    if errors:
        print("Docs consistency checks failed:")
        for err in errors:
            print(f"- {err}")
        return 1

    print(
        "Docs consistency checks passed "
        f"(requires-python>={py_floor}, install guidance aligned, README/changelog surfaces present)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
