# Bug Report

**Date:** 2026-02-17
**Reporter:** static-analysis-triage
**Severity:** [ ] Critical | [ ] High | [ ] Medium | [x] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [ ] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

The `--organize-by-priority` feature in `codex_bug_hunt_simple.py` flattens report paths to basename only when copying into priority directories. If two source files in different directories share the same filename (e.g., `src/lib.rs.md` from two different crates), the later copy silently overwrites the earlier one, causing data loss.

## Steps to Reproduce

1. Run the bug hunt script against a codebase with two files named `lib.rs` in different crates (e.g., `murk/src/lib.rs` and `murk-bench/src/lib.rs`).
2. Pass `--organize-by-priority`.
3. The `by-priority/P3/` directory will contain only one `lib.rs.md`, the other is overwritten.

## Expected Behavior

All reports should be preserved, using relative paths or unique names under the priority directories.

## Actual Behavior

Reports with duplicate basenames silently overwrite each other.

## Reproduction Rate

- 100% when basename collisions exist.

## Environment

- **OS:** Any
- **Rust toolchain:** N/A
- **Murk version/commit:** HEAD (feat/release-0.1.7)
- **Python version (if murk-python):** 3.10+

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```python
# After running with --organize-by-priority:
# docs/bugs/generated/crates/murk/src/lib.rs.md -> by-priority/P3/lib.rs.md
# docs/bugs/generated/crates/murk-bench/src/lib.rs.md -> by-priority/P3/lib.rs.md (overwrites!)
```

## Additional Context

**Source report:** /home/john/murk/docs/bugs/generated/scripts/codex_bug_hunt_simple.py.md
**Verified lines:** `scripts/codex_bug_hunt_simple.py:254-260`
**Root cause:** Line 258 uses `report.name` (basename only) to construct the destination path, discarding the relative directory structure.
**Suggested fix:** Preserve relative paths when copying: `rel = report.relative_to(output_dir); destination = output_dir / "by-priority" / priority / rel` with `destination.parent.mkdir(parents=True, exist_ok=True)`.
