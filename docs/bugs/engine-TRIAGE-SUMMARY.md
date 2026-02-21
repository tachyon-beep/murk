# murk-engine Static Analysis Triage Summary

**Date:** 2026-02-17
**Triaged by:** static-analysis-triage
**Branch:** feat/release-0.1.7

## Overview

- **Reports reviewed:** 15
- **Confirmed bugs:** 11 (8 High, 3 Medium)
- **Skipped (trivial/no bug):** 4
- **False positives:** 0
- **Design as intended:** 0
- **Already fixed:** 0

---

## Report Classifications

| # | Source File | Classification | Severity | Ticket | One-line Reason |
|---|-----------|---------------|----------|--------|----------------|
| 1 | `src/config.rs` | **CONFIRMED** | High | [engine-backoff-config-not-validated.md](engine-backoff-config-not-validated.md) | `validate()` never checks BackoffConfig invariants; `initial_max_skew > max_skew_cap` passes silently and breaks runtime cap at decay reset |
| 2 | `src/egress.rs` | **CONFIRMED** | Medium | [engine-egress-double-epoch-read.md](engine-egress-double-epoch-read.md) | Worker reads `epoch_counter.current()` twice per task; second read can diverge from pinned epoch, inflating `age_ticks` |
| 3 | `src/epoch.rs` | **CONFIRMED** | High | [engine-epoch-inconsistent-stall-snapshot.md](engine-epoch-inconsistent-stall-snapshot.md) | `is_pinned()` and `pin_start_ns()` are separate atomics with no consistent snapshot API; stall detector can read mismatched values during concurrent unpin/repin |
| 4 | `src/ingress.rs` | **CONFIRMED** | High | [engine-ingress-anonymous-command-misordering.md](engine-ingress-anonymous-command-misordering.md) | `source_id=None, source_seq=Some(_)` breaks anonymous arrival ordering; reachable via FFI independent mapping |
| 5 | `src/lib.rs` | **SKIPPED** | Trivial | -- | Module declarations and re-exports only; no executable logic |
| 6 | `src/lockstep.rs` | **CONFIRMED** | Medium | [engine-stepresult-receipts-doc-mismatch.md](engine-stepresult-receipts-doc-mismatch.md) | Doc says StepResult.receipts excludes submission-rejected receipts, but code and tests confirm they are included |
| 7 | `src/metrics.rs` | **SKIPPED** | Trivial | -- | Plain data struct with no logic; no bug possible |
| 8 | `src/overlay.rs` | **CONFIRMED** | High | [engine-overlay-empty-buffer-conflation.md](engine-overlay-empty-buffer-conflation.md) | `read()` uses `!is_empty()` as freshness sentinel, conflating empty-but-valid fields (Vector{dims:0}) with stale/missing |
| 9 | `src/realtime.rs` | **CONFIRMED** | High | [engine-realtime-shutdown-blocks-on-slow-tick.md](engine-realtime-shutdown-blocks-on-slow-tick.md) | Shutdown does unbounded `join()` on tick thread that uses uninterruptible `std::thread::sleep`; low `tick_rate_hz` causes multi-second shutdown blocks |
| 10 | `src/ring.rs` | **CONFIRMED** | High | [engine-ring-latest-spurious-none.md](engine-ring-latest-spurious-none.md) | `latest()` returns `None` after `capacity` retries despite non-empty ring, violating its documented guarantee |
| 11 | `src/tick.rs` | **CONFIRMED** | High | [engine-tick-non-setfield-commands-silently-accepted.md](engine-tick-non-setfield-commands-silently-accepted.md) | All non-SetField commands (SetParameter, Move, Spawn, etc.) get `accepted+applied` receipts but are never executed; param_version never incremented |
| 12 | `src/tick_thread.rs` | **CONFIRMED** | High | [engine-adaptive-backoff-output-unused.md](engine-adaptive-backoff-output-unused.md) | Adaptive backoff computes `effective_max_skew` but return value is discarded; stall detection uses only fixed thresholds |
| 13 | `examples/quickstart.rs` | **CONFIRMED** | Medium | [engine-quickstart-setfield-noop.md](engine-quickstart-setfield-noop.md) | SetField injection for HEAT is overwritten by full-write propagator in same tick; example's "second heat spot" claim is incorrect |
| 14 | `examples/realtime_async.rs` | **SKIPPED** | Trivial | -- | No concrete bug; all accesses properly guarded |
| 15 | `examples/replay.rs` | **SKIPPED** | Trivial | -- | No concrete bug; bounds handling and hash comparison correct |

---

## Priority Ranking (Confirmed Bugs)

### High Severity (8)

1. **engine-tick-non-setfield-commands-silently-accepted** -- Silent command loss for all non-SetField command types. Most impactful: affects every user sending parameter commands.
2. **engine-ingress-anonymous-command-misordering** -- Breaks deterministic ordering contract. Reachable via FFI.
3. **engine-adaptive-backoff-output-unused** -- Entire adaptive backoff subsystem is dead code in practice.
4. **engine-epoch-inconsistent-stall-snapshot** -- False-positive force-unpin can disrupt healthy workers.
5. **engine-ring-latest-spurious-none** -- Violates documented guarantee; can cause observation failures.
6. **engine-realtime-shutdown-blocks-on-slow-tick** -- Shutdown can block for seconds with low tick rates.
7. **engine-overlay-empty-buffer-conflation** -- Zero-component fields misread as missing. Low practical risk (dims=0 is unusual) but architecturally unsound.
8. **engine-backoff-config-not-validated** -- Misconfigured backoff breaks cap invariant. Low practical risk with default config.

### Medium Severity (3)

9. **engine-stepresult-receipts-doc-mismatch** -- Documentation-only; behavior is correct (and tested).
10. **engine-egress-double-epoch-read** -- Metadata inaccuracy (age_ticks off by 1); narrow race window.
11. **engine-quickstart-setfield-noop** -- Example documentation is misleading; no runtime impact.
