Skill used: `using-software-engineering` (code-review methodology for a focused bug audit).

# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [x] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`/home/john/murk/crates/murk-ffi/build.rs` writes generated artifacts into the crate source directory (`include/murk.h`), which causes deterministic build failure in read-only source environments.

## Steps to Reproduce

1. Make the crate source tree read-only (or build in an environment where sources are mounted read-only).
2. Run `cargo build -p murk-ffi`.
3. Observe build script failure when creating/writing `include/murk.h`.

## Expected Behavior

Build script should write generated files to Cargo’s writable output directory (`OUT_DIR`) and not require write access to the source checkout.

## Actual Behavior

Build script writes to `CARGO_MANIFEST_DIR/include/murk.h` and panics on permission errors via `expect(...)`.

Evidence:
- `/home/john/murk/crates/murk-ffi/build.rs:10` (`output_dir` set under manifest dir)
- `/home/john/murk/crates/murk-ffi/build.rs:11` (`create_dir_all(...).expect(...)`)
- `/home/john/murk/crates/murk-ffi/build.rs:18` (`write_to_file(output_dir.join("murk.h"))`)

## Reproduction Rate

Always (in read-only source environments)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** Any

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```text
N/A - found via static analysis
```

## Minimal Reproducer

```bash
cd /home/john/murk
chmod -R a-w crates/murk-ffi
cargo build -p murk-ffi
# build.rs fails at create_dir_all/write_to_file with permission denied
```

## Additional Context

Root cause: the build script writes generated header output into the source tree instead of `OUT_DIR`.  
Suggested fix: generate into `OUT_DIR` (from `env::var("OUT_DIR")`) and only copy/export to `include/` via an explicit, opt-in workflow if needed.