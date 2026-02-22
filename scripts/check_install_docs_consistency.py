#!/usr/bin/env python3
"""Check install/version guidance consistency across user-facing docs."""

from __future__ import annotations

import pathlib
import re
import sys


def extract_python_floor(pyproject: str) -> str:
    match = re.search(r'requires-python\s*=\s*">=\s*([0-9]+\.[0-9]+)"', pyproject)
    if not match:
        raise ValueError("Could not parse requires-python from crates/murk-python/pyproject.toml")
    return match.group(1)


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent.parent
    pyproject = (root / "crates/murk-python/pyproject.toml").read_text(encoding="utf-8")
    readme = (root / "README.md").read_text(encoding="utf-8")
    getting_started = (root / "book/src/getting-started.md").read_text(encoding="utf-8")

    py_floor = extract_python_floor(pyproject)
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

    if errors:
        print("Install/docs consistency checks failed:")
        for err in errors:
            print(f"- {err}")
        return 1

    print(
        "Install/docs consistency checks passed "
        f"(requires-python>={py_floor}, published+source instructions present)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
