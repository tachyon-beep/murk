# Triage Summary: murk-python, Examples, Scripts, murk-bench, murk Umbrella

**Date:** 2026-02-17
**Triaged by:** static-analysis-triage
**Reports reviewed:** 24
**Tickets filed:** 9

---

## Overview

| Category       | Reports | No Bug | Confirmed | False Positive | Design as Intended |
|----------------|---------|--------|-----------|----------------|--------------------|
| murk-python    | 11      | 5      | 6         | 0              | 0                  |
| murk-bench     | 6       | 5      | 1         | 0              | 0                  |
| murk (umbrella)| 1       | 1      | 0         | 0              | 0                  |
| Examples       | 4       | 1      | 1*        | 0              | 0                  |
| Scripts        | 1       | 0      | 1         | 0              | 0                  |
| **Total**      | **24**  | **12** | **9**     | **0**          | **0**              |

*The 3 example warmup-tick bugs are consolidated into 1 ticket since they share the same root cause.

---

## Confirmed Bugs (Tickets Filed)

### Critical (1)

| Ticket | File | Summary |
|--------|------|---------|
| [python-world-step-receipt-buffer-oob](python-world-step-receipt-buffer-oob.md) | `crates/murk-python/src/world.rs` | `World::step` can panic with OOB slice when FFI reports more receipts than buffer capacity. FFI `write_receipts` sets `*n_out = receipts.len()` (total) but only writes `min(len, cap)`. |

### High (1)

| Ticket | File | Summary |
|--------|------|---------|
| [python-propagator-trampoline-leak-on-cstring-error](python-propagator-trampoline-leak-on-cstring-error.md) | `crates/murk-python/src/propagator.rs` | `TrampolineData` leaked when `CString::new()` fails or `add_propagator_handle` fails after `Box::into_raw`. |

### Medium (4)

| Ticket | File | Summary |
|--------|------|---------|
| [python-metrics-race-between-step-and-propagator-query](python-metrics-race-between-step-and-propagator-query.md) | `crates/murk-python/src/metrics.rs` | Aggregate metrics and per-propagator timings fetched from different snapshots; race possible with multi-threaded world access. |
| [python-vecenv-false-sb3-compatibility-claim](python-vecenv-false-sb3-compatibility-claim.md) | `crates/murk-python/python/murk/vec_env.py` | Docstring claims SB3 VecEnv compatibility but API follows Gymnasium conventions (5-tuple step, missing async methods). |
| [python-command-docstring-expiry-default-mismatch](python-command-docstring-expiry-default-mismatch.md) | `crates/murk-python/src/command.rs` | Docstring says "default 0 = never" but actual default is `u64::MAX`; passing 0 causes immediate expiry. |
| [python-error-hints-reference-unexposed-config](python-error-hints-reference-unexposed-config.md) | `crates/murk-python/src/error.rs` | Error recovery hints reference `ring_buffer_size`, `set_max_ingress_queue`, `AsyncConfig`, `max_epoch_hold_ms` -- none exposed in Python API. |

### Low (2) + Medium example (1)

| Ticket | File | Summary |
|--------|------|---------|
| [example-warmup-ticks-shorten-episode-length](example-warmup-ticks-shorten-episode-length.md) | `examples/{crystal_nav,heat_seeker,layered_hex}` | All 3 examples have warmup ticks that consume the global tick budget, shortening effective episode length by 27-31%. Medium severity as example code that could mislead users building their own envs. |
| [bench-space-ops-degenerate-q-distribution](bench-space-ops-degenerate-q-distribution.md) | `crates/murk-bench/benches/space_ops.rs` | LCG multiplier not coprime to modulus 20; `q` coordinate only covers 4/20 values in distance benchmark. |
| [script-organize-by-priority-basename-collision](script-organize-by-priority-basename-collision.md) | `scripts/codex_bug_hunt_simple.py` | `--organize-by-priority` flattens paths to basename, silently overwriting reports with duplicate filenames. |

---

## Skipped Reports (No Bug Found)

These reports explicitly concluded "no concrete bug" or "trivial / no change required":

| File | Reason |
|------|--------|
| `crates/murk-python/python/murk/__init__.py` | Clean re-exports, no issue. |
| `crates/murk-python/src/config.rs` | FFI handle lifecycle consistent. |
| `crates/murk-python/src/lib.rs` | Straightforward module wiring. |
| `crates/murk-python/src/obs.rs` | Consistent precondition checks and handle lifecycle. |
| `crates/murk-bench/benches/arena_ops.rs` | Valid benchmark patterns. |
| `crates/murk-bench/benches/codec_ops.rs` | No defect found. |
| `crates/murk-bench/benches/obs_ops.rs` | Buffer sizes consistent with plan. |
| `crates/murk-bench/benches/reference_profile.rs` | Valid benchmark patterns. |
| `crates/murk-bench/examples/lockstep_rl.rs` | Correct lock/index usage. |
| `crates/murk-bench/src/lib.rs` | Boundary handling consistent. |
| `crates/murk/src/lib.rs` | Facade re-exports, no executable logic. |
| `examples/hex_pursuit/hex_pursuit.py` | No defect found. |

---

## Declined Reports

### env.py reset() seed reuse (DESIGN_AS_INTENDED)

The report flagged `MurkEnv.reset()` for reusing the same seed when `seed=None`. After verification, this is intentional: murk's determinism contract requires explicit seed control. The examples demonstrate the intended pattern by varying agent placement via `self._episode_count` while keeping the world field state deterministic. The world is *supposed* to reset to the same initial field state each episode -- only agent placement changes. This is not a Gymnasium convention violation; it is the murk determinism contract.

---

## Risk Assessment

The **receipt buffer OOB** (Critical) is the most impactful finding. It can cause a Rust panic that crashes the Python process. It requires a specific condition (more receipts than commands submitted), which is possible during rollback scenarios.

The **TrampolineData leak** (High) is a memory leak on an uncommon error path (NUL byte in propagator name), but it leaks a Python object reference that could prevent GC.

The **metrics race** (Medium) only manifests with multi-threaded world access, which is uncommon in typical RL workflows but possible.

The three **documentation/hint bugs** (Medium) are collectively important for user experience -- they represent the main developer-facing API surface and could cause confusion during debugging.

The **example warmup bug** (Medium) is notable because these examples serve as templates for users building their own environments. The pattern of using absolute tick_id for truncation while consuming ticks during warmup is a pit users will fall into.

The **bench** and **script** bugs (Low) are minor and do not affect production code.
