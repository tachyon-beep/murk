# Bug Index

Generated 2026-02-17 from static analysis triage of 110 source reports.
53 confirmed bugs from 64 actionable reports (83% confirmation rate).

**Status (updated 2026-02-19):** 23 fixed, 0 partially fixed, 30 still open.

## Open Bugs

### High (4 open)

| # | Ticket | Crate | Summary | Status |
|---|--------|-------|---------|--------|
| 6 | [engine-ring-latest-spurious-none](engine-ring-latest-spurious-none.md) | murk-engine | `SnapshotRing::latest()` returns None when snapshots exist under overwrite races | Open |
| 7 | [engine-realtime-shutdown-blocks-on-slow-tick](engine-realtime-shutdown-blocks-on-slow-tick.md) | murk-engine | Shutdown blocks indefinitely with low `tick_rate_hz` due to uninterruptible sleep | Open |
| 20 | [ffi-mutex-poisoning-panic-in-extern-c](ffi-mutex-poisoning-panic-in-extern-c.md) | murk-ffi | 43+ `lock().unwrap()` calls in extern "C" functions; poisoned mutex = UB | Open |
| 23 | [python-propagator-trampoline-leak-on-cstring-error](python-propagator-trampoline-leak-on-cstring-error.md) | murk-python | TrampolineData leaks on CString/add_propagator_handle error paths | Open |


### Medium (16 open)

| # | Ticket | Crate | Summary | Status |
|---|--------|-------|---------|--------|
| 25 | [engine-egress-double-epoch-read](engine-egress-double-epoch-read.md) | murk-engine | Double epoch read allows age_ticks overstated by 1+ | Open |
| 26 | [engine-stepresult-receipts-doc-mismatch](engine-stepresult-receipts-doc-mismatch.md) | murk-engine | StepResult.receipts doc says no rejections but code includes them | Open |
| 27 | [engine-quickstart-setfield-noop](engine-quickstart-setfield-noop.md) | murk-engine | Quickstart example's SetField injection overwritten by diffusion propagator | Open |
| 29 | [arena-sparse-segment-memory-leak](arena-sparse-segment-memory-leak.md) | murk-arena | Sparse CoW bump-allocates but never reclaims dead segment memory | Open |
| 30 | [propagator-agent-movement-tick0-actions](propagator-agent-movement-tick0-actions.md) | murk-propagators | Actions processed on tick 0 alongside initialization | Open |
| 32 | [propagator-reward-stale-heat-gradient-dependency](propagator-reward-stale-heat-gradient-dependency.md) | murk-propagators | reads() declares HEAT_GRADIENT but step() never reads it | Open |
| 33 | [obs-flatbuf-silent-u16-truncation](obs-flatbuf-silent-u16-truncation.md) | murk-obs | serialize truncates entry count to u16 but writes all entries | Open |
| 34 | [obs-geometry-is-interior-missing-dim-check](obs-geometry-is-interior-missing-dim-check.md) | murk-obs | is_interior missing dimension validation; empty center returns true | Open |
| 35 | [replay-codec-unbounded-alloc-from-wire](replay-codec-unbounded-alloc-from-wire.md) | murk-replay | decode_frame allocates from untrusted u32 lengths; DoS vector | Open |
| 36 | [replay-hash-empty-snapshot-returns-nonzero](replay-hash-empty-snapshot-returns-nonzero.md) | murk-replay | Doc says returns 0 for no fields but returns FNV_OFFSET | Open |
| 37 | [ffi-accessor-ambiguous-zero-return](ffi-accessor-ambiguous-zero-return.md) | murk-ffi | World accessors return 0 for invalid handles, indistinguishable from valid state | Open |
| 38 | [ffi-handle-generation-wraparound](ffi-handle-generation-wraparound.md) | murk-ffi | u32 generation wraps after 4B cycles; ABA handle resurrection | Open |
| 39 | [python-metrics-race-between-step-and-propagator-query](python-metrics-race-between-step-and-propagator-query.md) | murk-python | Per-propagator timings fetched via separate FFI call; race with concurrent step | Open |
| 40 | [python-vecenv-false-sb3-compatibility-claim](python-vecenv-false-sb3-compatibility-claim.md) | murk-python | MurkVecEnv claims SB3 compatibility but follows Gymnasium conventions | Open |
| 41 | [python-command-docstring-expiry-default-mismatch](python-command-docstring-expiry-default-mismatch.md) | murk-python | Docstring says "0 = never" but default is u64::MAX; 0 means immediate expiry | Open |
| 42 | [python-error-hints-reference-unexposed-config](python-error-hints-reference-unexposed-config.md) | murk-python | Error hints reference config knobs not exposed in Python API | Open |
| 43 | [example-warmup-ticks-shorten-episode-length](example-warmup-ticks-shorten-episode-length.md) | examples | Warmup ticks consume global tick budget; episodes 27-31% shorter than MAX_STEPS | Open |
| 45 | [space-hex2d-disk-overflow](space-hex2d-disk-overflow.md) | murk-space | compile_hex_disk i64 overflow when radius near i32::MAX | Open |
| 46 | [space-compliance-ordering-membership](space-compliance-ordering-membership.md) | murk-space | Compliance test checks cardinality/uniqueness but not cell membership | Open |

### Low (6 open)

| # | Ticket | Crate | Summary | Status |
|---|--------|-------|---------|--------|
| 47 | [arena-scratch-alloc-overflow](arena-scratch-alloc-overflow.md) | murk-arena | Unchecked `*2` growth can overflow usize (practically unreachable on 64-bit) | Open |
| 48 | [obs-metadata-doc-says-six-fields](obs-metadata-doc-says-six-fields.md) | murk-obs | Doc says "six fields" but struct has five | Open |
| 49 | [replay-compare-sentinel-zero-divergence](replay-compare-sentinel-zero-divergence.md) | murk-replay | Length/presence mismatches reported with hardcoded 0.0 sentinel | Open |
| 50 | [core-command-ordering-doc-missing-source-seq](core-command-ordering-doc-missing-source-seq.md) | murk-core | Command ordering doc omits source_seq from sort key description | Open |
| 51 | [bench-space-ops-degenerate-q-distribution](bench-space-ops-degenerate-q-distribution.md) | murk-bench | LCG multiplier not coprime to modulus; only 4/20 q values exercised | Open |
| 52 | [script-organize-by-priority-basename-collision](script-organize-by-priority-basename-collision.md) | scripts | --organize-by-priority flattens paths; duplicate basenames overwrite | Open |

## Closed Bugs (23 fixed)

Fixed in commits `02c12f3`, `dd52604`, `c0f5d55`. Tickets moved to [closed/](closed/).

| # | Ticket | Crate | Summary | Fix Commit |
|---|--------|-------|---------|------------|
| 1 | [python-world-step-receipt-buffer-oob](closed/python-world-step-receipt-buffer-oob.md) | murk-python | `World::step` panics on OOB slice when FFI reports more receipts than buffer capacity | 02c12f3 |
| 2 | [engine-tick-non-setfield-commands-silently-accepted](closed/engine-tick-non-setfield-commands-silently-accepted.md) | murk-engine | Non-SetField commands silently accepted with success receipt but never executed | dd52604 |
| 3 | [engine-ingress-anonymous-command-misordering](closed/engine-ingress-anonymous-command-misordering.md) | murk-engine | Anonymous commands misordered when `source_id=None` but `source_seq=Some(_)` | c0f5d55 |
| 5 | [engine-epoch-inconsistent-stall-snapshot](closed/engine-epoch-inconsistent-stall-snapshot.md) | murk-engine | Stall detection reads two atomics non-atomically; false cancellation of healthy workers | c0f5d55 |
| 8 | [engine-overlay-empty-buffer-conflation](closed/engine-overlay-empty-buffer-conflation.md) | murk-engine | Overlay cache conflates missing field with present-but-empty field | dd52604 |
| 10 | [arena-generation-counter-overflow](closed/arena-generation-counter-overflow.md) | murk-arena | u32 generation overflows after ~4B ticks; panic in debug, wrap breaks correctness in release | c0f5d55 |
| 11 | [arena-sparse-cow-generation-rollover](closed/arena-sparse-cow-generation-rollover.md) | murk-arena | Sparse CoW `<` comparison fails after generation wrap; silent field data loss | c0f5d55 |
| 16 | [propagator-diffusion-cfl-hardcoded-degree](closed/propagator-diffusion-cfl-hardcoded-degree.md) | murk-propagators | CFL bound hardcodes degree=4; numerically unstable on Hex2D (6) and Fcc12 (12) | c0f5d55 |
| 17 | [obs-plan-fast-path-unchecked-index-panic](closed/obs-plan-fast-path-unchecked-index-panic.md) | murk-obs | Fast-path gather uses unchecked indexing; short field buffer panics | c0f5d55 |
| 18 | [obs-pool-nan-produces-infinity](closed/obs-pool-nan-produces-infinity.md) | murk-obs | pool_2d emits -inf/+inf with valid mask when all window cells are NaN | c0f5d55 |
| 21 | [ffi-productspace-unchecked-float-cast](closed/ffi-productspace-unchecked-float-cast.md) | murk-ffi | f64→usize cast without validation; INFINITY becomes usize::MAX, panics in Vec | c0f5d55 |
| 15 | [propagator-writemode-incremental-not-implemented](closed/propagator-writemode-incremental-not-implemented.md) | murk-propagator | `WriteMode::Incremental` buffers now seeded from previous generation | (this session) |
| 19 | [ffi-trampoline-null-pointer-dereference](closed/ffi-trampoline-null-pointer-dereference.md) | murk-ffi | Null pointer guards added to all three trampoline functions | (this session) |
| 44 | [space-fcc12-parity-overflow](closed/space-fcc12-parity-overflow.md) | murk-space | XOR-based parity replaces arithmetic overflow in all 5 sites | (this session) |
| 22 | [ffi-obs-negative-to-unsigned-cast](closed/ffi-obs-negative-to-unsigned-cast.md) | murk-ffi | Negative i32 params cast to u32/usize; -1 becomes u32::MAX | c0f5d55 |
| 31 | [propagator-pipeline-nan-maxdt-bypass](closed/propagator-pipeline-nan-maxdt-bypass.md) | murk-propagator | NaN from max_dt() bypasses all stability constraints | (this session) |
| 13 | [arena-placeholder-pertick-handles-in-snapshot](closed/arena-placeholder-pertick-handles-in-snapshot.md) | murk-arena | Placeholder handles readable via snapshot before any begin_tick/publish cycle | (this session) |
| 28 | [arena-segment-slice-beyond-cursor](closed/arena-segment-slice-beyond-cursor.md) | murk-arena | Segment::slice checks capacity not cursor; reads stale data | (this session) |
| 24 | [space-product-weighted-metric-truncation](closed/space-product-weighted-metric-truncation.md) | murk-space | Weighted metric silently drops trailing component distances via zip truncation | (this session) |
| 14 | [arena-static-arena-duplicate-field-ids](closed/arena-static-arena-duplicate-field-ids.md) | murk-arena | Duplicate FieldIds silently accepted; earlier allocations orphaned | (this session) |
| 12 | [arena-missing-segment-size-validation](closed/arena-missing-segment-size-validation.md) | murk-arena | `segment_size` power-of-two/minimum constraints documented but never enforced | (this session) |
| 9 | [engine-backoff-config-not-validated](closed/engine-backoff-config-not-validated.md) | murk-engine | `BackoffConfig` invariants not checked; `initial_max_skew > max_skew_cap` allowed | (this session) |
| 4 | [engine-adaptive-backoff-output-unused](closed/engine-adaptive-backoff-output-unused.md) | murk-engine | Adaptive backoff computes output but result is discarded; dead code in practice | (this session) |

## By Crate (open only)

| Crate | High | Medium | Low | Total Open |
|-------|------|--------|-----|------------|
| murk-engine | 1 | 3 | 0 | 4 |
| murk-arena | 0 | 1 | 1 | 2 |
| murk-ffi | 1 | 2 | 0 | 3 |
| murk-python | 1 | 4 | 0 | 5 |
| murk-propagator | 0 | 0 | 0 | 0 |
| murk-propagators | 0 | 2 | 0 | 2 |
| murk-obs | 0 | 2 | 1 | 3 |
| murk-replay | 0 | 2 | 1 | 3 |
| murk-space | 1 | 2 | 0 | 3 |
| murk-core | 0 | 0 | 1 | 1 |
| murk-bench | 0 | 0 | 1 | 1 |
| examples | 0 | 1 | 0 | 1 |
| scripts | 0 | 0 | 1 | 1 |
| **Total** | **3** | **16** | **6** | **30** |

## Triage Summaries

Detailed per-batch classification (confirmed/false-positive/design-as-intended) with rationale:

- [engine-TRIAGE-SUMMARY](engine-TRIAGE-SUMMARY.md) — 15 reports → 11 confirmed, 4 skipped
- [arena-TRIAGE-SUMMARY](arena-TRIAGE-SUMMARY.md) — 13 reports → 8 confirmed, 4 skipped, 1 doc-only
- [space-TRIAGE-SUMMARY](space-TRIAGE-SUMMARY.md) — 14 reports → 4 confirmed, 6 design-as-intended, 4 skipped
- [propagator-TRIAGE-SUMMARY](propagator-TRIAGE-SUMMARY.md) — 11 reports → 6 confirmed, 1 design-as-intended, 4 skipped
- [obs-replay-core-TRIAGE-SUMMARY](obs-replay-core-TRIAGE-SUMMARY.md) — 22 reports → 9 confirmed, 1 design-as-intended, 12 skipped
- [ffi-TRIAGE-SUMMARY](ffi-TRIAGE-SUMMARY.md) — 9 reports → 6 confirmed, 3 skipped
- [python-examples-TRIAGE-SUMMARY](python-examples-TRIAGE-SUMMARY.md) — 24 reports → 9 confirmed, 1 design-as-intended, 12 skipped, 2 declined
