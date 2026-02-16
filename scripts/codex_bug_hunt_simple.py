#!/usr/bin/env python3
from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path


DEFAULT_TEMPLATE = """## Summary

- <one-sentence bug summary>

## Severity

- Severity: <critical|major|minor|trivial>
- Priority: <P0|P1|P2|P3>

## Evidence

- <exact file/line references and behavior>

## Root Cause Hypothesis

- <why this is happening>

## Suggested Fix

- <short, concrete fix>
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
}


def resolve_path(repo_root: Path, value: str) -> Path:
    path = Path(value)
    if path.is_absolute():
        return path
    return (repo_root / path).resolve()


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


def build_prompt(file_path: Path, template: str, extra_message: str | None = None) -> str:
    extra = f"\nExtra context:\n{extra_message}\n" if extra_message else ""
    return (
        "You are a static analysis agent doing a focused bug audit.\n"
        f"Target file: {file_path}\n\n"
        "Instructions:\n"
        "- Use the bug report template below.\n"
        "- Fill all sections.\n"
        f"- If no bug is found, set Summary to: 'No concrete bug found in {file_path}'.\n"
        "- Include file paths and line numbers in evidence when possible.\n"
        f"{extra}\n"
        "Bug report template:\n"
        f"{template}\n"
    )


def run_codex_once(*, repo_root: Path, prompt: str, output_path: Path, model: str | None) -> None:
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

    result = subprocess.run(cmd, cwd=repo_root, capture_output=True, text=True, check=False)
    if result.returncode != 0:
        stderr = result.stderr.strip()
        if len(stderr) > 500:
            stderr = stderr[:500] + "... (truncated)"
        raise RuntimeError(f"codex exec failed (exit {result.returncode}): {stderr}")


def run_with_retries(
    *,
    repo_root: Path,
    prompt: str,
    output_path: Path,
    model: str | None,
    max_attempts: int,
    retry_delay_s: float,
) -> None:
    last_error: Exception | None = None
    for attempt in range(1, max_attempts + 1):
        try:
            run_codex_once(repo_root=repo_root, prompt=prompt, output_path=output_path, model=model)
            return
        except Exception as exc:  # noqa: BLE001
            last_error = exc
            if attempt >= max_attempts:
                break
            delay = retry_delay_s * (2 ** (attempt - 1))
            print(
                f"Retry {attempt}/{max_attempts - 1} for {output_path.name} after error: {exc}",
                file=sys.stderr,
            )
            time.sleep(delay)

    assert last_error is not None
    raise last_error


def report_priority(report_path: Path) -> str:
    text = report_path.read_text(encoding="utf-8")
    match = re.search(r"\bP[0-3]\b", text)
    return match.group(0) if match else "P3"


def main() -> int:
    parser = argparse.ArgumentParser(description="Simple per-file Codex bug hunt runner.")
    parser.add_argument("--root", default=".", help="Directory to scan (default: current repo root).")
    parser.add_argument("--output-dir", default="docs/bugs/generated", help="Where to write reports.")
    parser.add_argument("--template", default=None, help="Optional bug template file.")
    parser.add_argument("--extensions", default="py", help="Comma-separated file extensions (default: py).")
    parser.add_argument("--include-tests", action="store_true", help="Include files under tests/ and test_*.")
    parser.add_argument("--skip-existing", action="store_true", help="Skip files with existing reports.")
    parser.add_argument("--max-attempts", type=int, default=3, help="Total attempts per file (default: 3).")
    parser.add_argument("--retry-delay", type=float, default=2.0, help="Base delay in seconds (default: 2).")
    parser.add_argument("--model", default=None, help="Optional model override passed to codex.")
    parser.add_argument("--limit", type=int, default=None, help="Only process the first N files.")
    parser.add_argument("--dry-run", action="store_true", help="Show files and exit.")
    parser.add_argument("--organize-by-priority", action="store_true", help="Copy reports into by-priority/P*/.")
    parser.add_argument("--extra-message", default=None, help="Optional context note to append to the prompt.")
    args = parser.parse_args()

    if args.max_attempts < 1:
        raise ValueError("--max-attempts must be >= 1")
    if args.retry_delay < 0:
        raise ValueError("--retry-delay must be >= 0")

    if shutil.which("codex") is None:
        raise RuntimeError("codex CLI not found on PATH")

    repo_root = Path(__file__).resolve().parents[1]
    root_dir = resolve_path(repo_root, args.root)
    output_dir = resolve_path(repo_root, args.output_dir)
    if not root_dir.exists() or not root_dir.is_dir():
        raise RuntimeError(f"Scan root does not exist or is not a directory: {root_dir}")

    extensions = {f".{ext.strip().lstrip('.').lower()}" for ext in args.extensions.split(",") if ext.strip()}
    if not extensions:
        raise ValueError("--extensions produced an empty set")

    template_text = (
        resolve_path(repo_root, args.template).read_text(encoding="utf-8") if args.template else DEFAULT_TEMPLATE
    )

    files = list_files(root_dir, extensions, exclude_tests=not args.include_tests)
    if args.limit is not None:
        files = files[: args.limit]

    if not files:
        print(f"No matching files found under {root_dir}")
        return 1

    if args.dry_run:
        print(f"Would analyze {len(files)} files:")
        for path in files:
            print(f"  {path.relative_to(repo_root)}")
        return 0

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

        prompt = build_prompt(file_path=file_path, template=template_text, extra_message=args.extra_message)
        print(f"[{idx}/{len(files)}] analyze {file_path.relative_to(repo_root)}")
        try:
            run_with_retries(
                repo_root=repo_root,
                prompt=prompt,
                output_path=output_path,
                model=args.model,
                max_attempts=args.max_attempts,
                retry_delay_s=args.retry_delay,
            )
            ok += 1
        except Exception as exc:  # noqa: BLE001
            failed += 1
            print(f"  FAILED: {exc}", file=sys.stderr)

    if args.organize_by_priority:
        for report in output_dir.rglob("*.md"):
            if "by-priority" in report.parts:
                continue
            priority = report_priority(report)
            destination = output_dir / "by-priority" / priority / report.name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(report, destination)

    print("\nSummary")
    print(f"- Total: {len(files)}")
    print(f"- Succeeded: {ok}")
    print(f"- Failed: {failed}")
    print(f"- Skipped: {skipped}")
    print(f"- Output: {output_dir.relative_to(repo_root)}")

    return 0 if failed == 0 else 2


if __name__ == "__main__":
    raise SystemExit(main())
