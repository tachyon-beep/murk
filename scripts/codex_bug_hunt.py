#!/usr/bin/env python3
"""Per-file static-analysis bug hunter for the murk workspace.

Scans Rust and Python source files and dispatches each to a CLI agent
(codex or claude) in read-only sandbox mode.  The agent produces a bug
report using a murk-specific template that includes crate attribution,
engine-mode classification, and language-aware analysis hints.

Usage examples:

    # Dry-run: list files that would be scanned
    python scripts/codex_bug_hunt.py --dry-run

    # Scan Rust only, first 10 files, using Claude backend
    python scripts/codex_bug_hunt.py --extensions rs --limit 10 --backend claude

    # Full scan, skip already-generated reports, organise by priority
    python scripts/codex_bug_hunt.py --skip-existing --organize-by-priority

    # Scan a single crate
    python scripts/codex_bug_hunt.py --root crates/murk-engine

    # Include test files
    python scripts/codex_bug_hunt.py --include-tests --extensions rs,py
"""
from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

MURK_CRATES = [
    "murk-core",
    "murk-engine",
    "murk-arena",
    "murk-space",
    "murk-propagator",
    "murk-propagators",
    "murk-obs",
    "murk-replay",
    "murk-ffi",
    "murk-python",
    "murk-bench",
    "murk-test-utils",
    "murk",
]

# Language-specific analysis focus areas injected into the prompt.
LANG_HINTS: dict[str, str] = {
    ".rs": (
        "Rust-specific focus areas:\n"
        "- Arithmetic overflow (checked_add/checked_mul missing, unchecked * 2)\n"
        "- Unsafe blocks: raw pointer derefs, from_raw without matching into_raw, "
        "transmute misuse\n"
        "- extern \"C\" functions: panic across FFI boundary = UB, "
        "mutex .unwrap() in extern C\n"
        "- Iterator::zip silently truncating mismatched lengths\n"
        "- Off-by-one in slice indexing, capacity vs cursor confusion\n"
        "- Atomics: non-atomic reads of multiple atomics (TOCTOU)\n"
        "- Resource leaks: Box::into_raw without matching from_raw on error paths\n"
        "- Generation/epoch counter wraparound (u32, u64)\n"
    ),
    ".py": (
        "Python / PyO3 focus areas:\n"
        "- GIL safety: releasing GIL while holding Python references\n"
        "- Type mismatches between Python docstrings and actual Rust signatures\n"
        "- Default value mismatches between docstring and implementation\n"
        "- Error message hints referencing config knobs not exposed in the Python API\n"
        "- SB3/Gymnasium API contract violations (reset, step, observation_space)\n"
        "- Torch tensor dtype/device mismatches in examples\n"
        "- Episode length off-by-one from warmup ticks consuming tick budget\n"
    ),
}

# Crate-specific analysis hints (appended when a file belongs to a crate).
CRATE_HINTS: dict[str, str] = {
    "murk-arena": (
        "This crate implements arena-based generational allocation with "
        "PingPongArena, StaticArena, ScratchRegion, and CoW snapshot semantics. "
        "Watch for: segment bounds vs cursor, placeholder handles readable "
        "before publish, duplicate FieldId acceptance, generation overflow."
    ),
    "murk-engine": (
        "This crate contains the tick loop, ingress/egress, epoch reclamation, "
        "adaptive backoff, and SnapshotRing.  Two engine modes exist: Lockstep "
        "(callable struct) and RealtimeAsync (autonomous thread). Watch for: "
        "dead backoff output, uninterruptible sleep blocking shutdown, "
        "non-atomic multi-atomic reads, ring overwrite races."
    ),
    "murk-ffi": (
        "This crate provides the C FFI surface.  Every function is extern \"C\". "
        "Watch for: panic across FFI boundary (instant UB), mutex .unwrap() in "
        "extern C, null pointer dereference in trampolines, ambiguous zero "
        "returns conflating error with valid state, handle generation ABA."
    ),
    "murk-python": (
        "PyO3 bindings wrapping murk-ffi.  Watch for: TrampolineData leaks on "
        "error paths after Box::into_raw, CString interior NUL, metrics race "
        "between step() and propagator queries, docstring/default mismatches."
    ),
    "murk-space": (
        "Topology crate: Hex2D, Fcc12, VonNeumann, ProductSpace. Watch for: "
        "coordinate overflow in parity checks, disk compilation overflow at "
        "extreme radii, ProductSpace weighted metric zip truncation, "
        "compliance test checking cardinality but not cell membership."
    ),
    "murk-propagator": (
        "Propagator trait and pipeline.  Watch for: NaN from max_dt() bypassing "
        "stability constraints, WriteMode::Incremental documented but not wired, "
        "scratch byte capacity rounding down."
    ),
    "murk-obs": (
        "Observation builder and FlatBuffer serialisation.  Watch for: silent "
        "u16 truncation of entry counts, is_interior missing dimension check, "
        "pool_2d NaN→infinity."
    ),
    "murk-replay": (
        "Deterministic replay codec.  Watch for: unbounded allocation from "
        "untrusted wire lengths (DoS), hash returning FNV_OFFSET for empty "
        "snapshots vs documented 0, compare sentinel 0.0 for length mismatches."
    ),
}

DEFAULT_TEMPLATE = """\
# Bug Report

**Date:** {date}
**Reporter:** static-analysis-agent
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [ ] Low

## Affected Crate(s)

{crate_checklist}

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [ ] Both / Unknown

## Summary

<!-- One sentence describing the bug. If no concrete bug found, write:
     "No concrete bug found in {file_path}." -->

## Steps to Reproduce

1.
2.
3.

## Expected Behavior

<!-- What should happen. -->

## Actual Behavior

<!-- What happens instead. Include error codes if applicable. -->

## Reproduction Rate

<!-- Always / Intermittent / Once -->

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):**

## Determinism Impact

- [ ] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```
<!-- Provide a minimal code snippet (Rust or Python) that triggers the bug. -->
```

## Additional Context

<!-- Root cause hypothesis, suggested fix, related issues. -->
"""

EXCLUDE_DIRS = {
    ".git",
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".venv",
    "venv",
    "node_modules",
    "target",
    "book",
    ".beads",
}


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def resolve_path(repo_root: Path, value: str) -> Path:
    path = Path(value)
    if path.is_absolute():
        return path
    return (repo_root / path).resolve()


def detect_crate(file_path: Path) -> str | None:
    """Return the murk crate name that *file_path* belongs to, or None."""
    parts = file_path.parts
    if "crates" in parts:
        idx = parts.index("crates")
        if idx + 1 < len(parts):
            candidate = parts[idx + 1]
            if candidate in MURK_CRATES:
                return candidate
    return None


def build_crate_checklist(active_crate: str | None) -> str:
    lines: list[str] = []
    for crate in MURK_CRATES:
        mark = "x" if crate == active_crate else " "
        label = f"{crate} (umbrella)" if crate == "murk" else crate
        lines.append(f"- [{mark}] {label}")
    return "\n".join(lines)


def list_files(root: Path, extensions: set[str], exclude_tests: bool) -> list[Path]:
    files: list[Path] = []
    for path in root.rglob("*"):
        if not path.is_file():
            continue
        if any(part in EXCLUDE_DIRS for part in path.parts):
            continue
        if path.suffix.lower() not in extensions:
            continue
        if exclude_tests and (path.name.startswith("test_") or "tests" in path.parts):
            continue
        files.append(path)
    return sorted(files)


def build_prompt(
    file_path: Path,
    template: str,
    extra_message: str | None = None,
) -> str:
    crate = detect_crate(file_path)
    ext = file_path.suffix.lower()

    # Assemble language + crate hints
    hints_parts: list[str] = []
    if ext in LANG_HINTS:
        hints_parts.append(LANG_HINTS[ext])
    if crate and crate in CRATE_HINTS:
        hints_parts.append(f"Crate context ({crate}):\n{CRATE_HINTS[crate]}")
    if extra_message:
        hints_parts.append(f"Extra context:\n{extra_message}")
    hints = "\n".join(hints_parts)

    # Fill template placeholders
    crate_checklist = build_crate_checklist(crate)
    filled_template = template.replace("{crate_checklist}", crate_checklist).replace(
        "{file_path}", str(file_path)
    )

    return (
        "You are a static-analysis agent doing a focused bug audit of the murk "
        "simulation framework.  Murk is a Rust workspace with PyO3 Python "
        "bindings, arena-based generational allocation, and two engine modes "
        "(Lockstep and RealtimeAsync).\n\n"
        f"Target file: {file_path}\n\n"
        "Instructions:\n"
        "- Read the target file carefully.  Look for concrete, demonstrable bugs — "
        "not style issues, not missing docs, not hypothetical concerns.\n"
        "- Fill ALL sections of the bug report template below.\n"
        "- If no concrete bug is found, set Summary to: "
        f"'No concrete bug found in {file_path}.'\n"
        "- Include exact file paths and line numbers in evidence.\n"
        "- If you find multiple bugs in the same file, produce one report per bug "
        "separated by a markdown horizontal rule (---).\n"
        "- Classify severity: Critical = data corruption / UB / panic in production, "
        "High = incorrect results / resource leak, Medium = edge-case / doc mismatch, "
        "Low = cosmetic / practically unreachable.\n"
        f"\n{hints}\n\n"
        "Bug report template:\n"
        f"{filled_template}\n"
    )


# ---------------------------------------------------------------------------
# Backend runners
# ---------------------------------------------------------------------------


def run_codex_once(
    *, repo_root: Path, prompt: str, output_path: Path, model: str | None
) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        "codex",
        "exec",
        "--sandbox",
        "read-only",
        "-c",
        'approval_policy="never"',
        "--output-last-message",
        str(output_path),
    ]
    if model:
        cmd.extend(["--model", model])
    cmd.append(prompt)

    result = subprocess.run(
        cmd, cwd=repo_root, capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        stderr = result.stderr.strip()
        if len(stderr) > 500:
            stderr = stderr[:500] + "... (truncated)"
        raise RuntimeError(
            f"codex exec failed (exit {result.returncode}): {stderr}"
        )


def run_claude_once(
    *, repo_root: Path, prompt: str, output_path: Path, model: str | None
) -> None:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    cmd = [
        "claude",
        "-p", prompt,
        "--output-format", "text",
        "--max-turns", "1",
    ]
    if model:
        cmd.extend(["--model", model])

    result = subprocess.run(
        cmd, cwd=repo_root, capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        stderr = result.stderr.strip()
        if len(stderr) > 500:
            stderr = stderr[:500] + "... (truncated)"
        raise RuntimeError(
            f"claude exec failed (exit {result.returncode}): {stderr}"
        )
    output_path.write_text(result.stdout, encoding="utf-8")


BACKENDS = {
    "codex": run_codex_once,
    "claude": run_claude_once,
}


def run_with_retries(
    *,
    repo_root: Path,
    prompt: str,
    output_path: Path,
    model: str | None,
    backend: str,
    max_attempts: int,
    retry_delay_s: float,
) -> None:
    runner = BACKENDS[backend]
    last_error: Exception | None = None
    for attempt in range(1, max_attempts + 1):
        try:
            runner(
                repo_root=repo_root,
                prompt=prompt,
                output_path=output_path,
                model=model,
            )
            return
        except Exception as exc:  # noqa: BLE001
            last_error = exc
            if attempt >= max_attempts:
                break
            delay = retry_delay_s * (2 ** (attempt - 1))
            print(
                f"  retry {attempt}/{max_attempts - 1} for "
                f"{output_path.name}: {exc}",
                file=sys.stderr,
            )
            time.sleep(delay)

    assert last_error is not None  # noqa: S101
    raise last_error


def report_priority(report_path: Path) -> str:
    text = report_path.read_text(encoding="utf-8")
    match = re.search(r"\bP[0-3]\b", text)
    return match.group(0) if match else "P3"


def report_severity(report_path: Path) -> str:
    text = report_path.read_text(encoding="utf-8")
    for level in ("Critical", "High", "Medium", "Low"):
        if f"[x] {level}" in text:
            return level
    return "Unknown"


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Per-file bug hunter for the murk workspace (Rust + Python).",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=(
            "examples:\n"
            "  %(prog)s --dry-run\n"
            "  %(prog)s --extensions rs --limit 10 --backend claude\n"
            "  %(prog)s --root crates/murk-engine --skip-existing\n"
            "  %(prog)s --include-tests --organize-by-priority\n"
        ),
    )
    parser.add_argument(
        "--root",
        default=".",
        help="Directory to scan (default: repo root).",
    )
    parser.add_argument(
        "--output-dir",
        default="docs/bugs/generated",
        help="Where to write reports (default: docs/bugs/generated).",
    )
    parser.add_argument(
        "--template",
        default=None,
        help="Path to a custom bug-report template file.",
    )
    parser.add_argument(
        "--extensions",
        default="rs,py",
        help="Comma-separated file extensions (default: rs,py).",
    )
    parser.add_argument(
        "--include-tests",
        action="store_true",
        help="Include test files (test_*, tests/).",
    )
    parser.add_argument(
        "--skip-existing",
        action="store_true",
        help="Skip files that already have a report.",
    )
    parser.add_argument(
        "--max-attempts",
        type=int,
        default=3,
        help="Max retries per file (default: 3).",
    )
    parser.add_argument(
        "--retry-delay",
        type=float,
        default=2.0,
        help="Base retry delay in seconds, doubled each attempt (default: 2).",
    )
    parser.add_argument(
        "--backend",
        choices=list(BACKENDS),
        default="codex",
        help="CLI backend: codex or claude (default: codex).",
    )
    parser.add_argument(
        "--model",
        default=None,
        help="Model override passed to the backend CLI.",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=None,
        help="Only process the first N files.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="List files that would be scanned and exit.",
    )
    parser.add_argument(
        "--organize-by-priority",
        action="store_true",
        help="After scanning, copy reports into by-priority/P*/ subdirs.",
    )
    parser.add_argument(
        "--organize-by-severity",
        action="store_true",
        help="After scanning, copy reports into by-severity/<level>/ subdirs.",
    )
    parser.add_argument(
        "--extra-message",
        default=None,
        help="Extra context appended to every prompt.",
    )
    args = parser.parse_args()

    if args.max_attempts < 1:
        print("error: --max-attempts must be >= 1", file=sys.stderr)
        return 1
    if args.retry_delay < 0:
        print("error: --retry-delay must be >= 0", file=sys.stderr)
        return 1

    if shutil.which(args.backend) is None:
        print(
            f"error: {args.backend} CLI not found on PATH", file=sys.stderr
        )
        return 1

    repo_root = Path(__file__).resolve().parents[1]
    root_dir = resolve_path(repo_root, args.root)
    output_dir = resolve_path(repo_root, args.output_dir)
    if not root_dir.exists() or not root_dir.is_dir():
        print(
            f"error: scan root does not exist or is not a directory: {root_dir}",
            file=sys.stderr,
        )
        return 1

    extensions = {
        f".{ext.strip().lstrip('.').lower()}"
        for ext in args.extensions.split(",")
        if ext.strip()
    }
    if not extensions:
        print("error: --extensions produced an empty set", file=sys.stderr)
        return 1

    template_text = (
        resolve_path(repo_root, args.template).read_text(encoding="utf-8")
        if args.template
        else DEFAULT_TEMPLATE
    )

    files = list_files(root_dir, extensions, exclude_tests=not args.include_tests)
    if args.limit is not None:
        files = files[: args.limit]

    if not files:
        print(f"No matching files found under {root_dir}")
        return 1

    # Count by language
    lang_counts: dict[str, int] = {}
    for f in files:
        lang_counts[f.suffix] = lang_counts.get(f.suffix, 0) + 1
    lang_summary = ", ".join(
        f"{count} {ext}" for ext, count in sorted(lang_counts.items())
    )

    if args.dry_run:
        print(f"Would analyze {len(files)} files ({lang_summary}):")
        for path in files:
            crate = detect_crate(path)
            crate_tag = f"  [{crate}]" if crate else ""
            print(f"  {path.relative_to(repo_root)}{crate_tag}")
        return 0

    print(f"Scanning {len(files)} files ({lang_summary}) with {args.backend}...")

    ok = 0
    failed = 0
    skipped = 0

    for idx, file_path in enumerate(files, start=1):
        relative = file_path.relative_to(root_dir)
        output_path = output_dir / relative
        output_path = output_path.with_suffix(output_path.suffix + ".md")

        if args.skip_existing and output_path.exists():
            skipped += 1
            print(f"[{idx}/{len(files)}] skip {file_path.relative_to(repo_root)}")
            continue

        prompt = build_prompt(
            file_path=file_path,
            template=template_text,
            extra_message=args.extra_message,
        )
        crate = detect_crate(file_path) or "—"
        print(
            f"[{idx}/{len(files)}] {file_path.relative_to(repo_root)} "
            f"({crate})"
        )
        try:
            run_with_retries(
                repo_root=repo_root,
                prompt=prompt,
                output_path=output_path,
                model=args.model,
                backend=args.backend,
                max_attempts=args.max_attempts,
                retry_delay_s=args.retry_delay,
            )
            ok += 1
        except Exception as exc:  # noqa: BLE001
            failed += 1
            print(f"  FAILED: {exc}", file=sys.stderr)

    # Post-processing: organise reports
    if args.organize_by_priority:
        for report in output_dir.rglob("*.md"):
            if "by-priority" in report.parts or "by-severity" in report.parts:
                continue
            priority = report_priority(report)
            rel = report.relative_to(output_dir)
            dest = output_dir / "by-priority" / priority / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(report, dest)

    if args.organize_by_severity:
        for report in output_dir.rglob("*.md"):
            if "by-priority" in report.parts or "by-severity" in report.parts:
                continue
            severity = report_severity(report)
            rel = report.relative_to(output_dir)
            dest = output_dir / "by-severity" / severity / rel
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(report, dest)

    print(f"\n{'=' * 40}")
    print("Bug Hunt Summary")
    print(f"{'=' * 40}")
    print(f"  Backend:   {args.backend}")
    print(f"  Total:     {len(files)}")
    print(f"  Succeeded: {ok}")
    print(f"  Failed:    {failed}")
    print(f"  Skipped:   {skipped}")
    print(f"  Output:    {output_dir.relative_to(repo_root)}")

    return 0 if failed == 0 else 2


if __name__ == "__main__":
    raise SystemExit(main())
