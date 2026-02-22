#!/usr/bin/env python3
"""Ensure replay wire-format docs match murk-replay FORMAT_VERSION."""

from __future__ import annotations

import pathlib
import re
import sys


def extract_code_version(lib_rs: str) -> int:
    match = re.search(r"pub const FORMAT_VERSION:\s*u8\s*=\s*(\d+)\s*;", lib_rs)
    if not match:
        raise ValueError("Could not find FORMAT_VERSION in crates/murk-replay/src/lib.rs")
    return int(match.group(1))


def extract_doc_current_version(doc_md: str) -> int:
    match = re.search(r"\*\*Current version:\*\*\s*(\d+)", doc_md)
    if not match:
        raise ValueError("Could not find '**Current version:** <n>' in docs/replay-format.md")
    return int(match.group(1))


def extract_doc_history_current_version(doc_md: str) -> int:
    match = re.search(r"### Version\s+(\d+)\s+\(current\)", doc_md)
    if not match:
        raise ValueError("Could not find '### Version <n> (current)' in docs/replay-format.md")
    return int(match.group(1))


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent.parent
    lib_rs_path = root / "crates/murk-replay/src/lib.rs"
    doc_path = root / "docs/replay-format.md"

    lib_rs = lib_rs_path.read_text(encoding="utf-8")
    doc_md = doc_path.read_text(encoding="utf-8")

    code_version = extract_code_version(lib_rs)
    doc_current_version = extract_doc_current_version(doc_md)
    doc_history_current_version = extract_doc_history_current_version(doc_md)

    errors: list[str] = []
    if doc_current_version != code_version:
        errors.append(
            f"Current version mismatch: docs={doc_current_version}, code={code_version}"
        )
    if doc_history_current_version != code_version:
        errors.append(
            "Version history mismatch: "
            f"docs current section={doc_history_current_version}, code={code_version}"
        )

    if errors:
        print("Replay format version checks failed:")
        for err in errors:
            print(f"- {err}")
        return 1

    print(f"Replay format version checks passed (v{code_version}).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
