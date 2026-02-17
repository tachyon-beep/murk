# Bug Index

Generated 2026-02-17 from static analysis triage of 110 source reports.
53 confirmed bugs from 64 actionable reports (83% confirmation rate).

## By Severity

### Critical (1)

| # | Ticket | Crate | Summary |
|---|--------|-------|---------|
| 1 | [python-world-step-receipt-buffer-oob](python-world-step-receipt-buffer-oob.md) | murk-python | `World::step` panics on OOB slice when FFI reports more receipts than buffer capacity |

### High (22)

| # | Ticket | Crate | Summary |
|---|--------|-------|---------|
| 2 | [engine-tick-non-setfield-commands-silently-accepted](engine-tick-non-setfield-commands-silently-accepted.md) | murk-engine | Non-SetField commands silently accepted with success receipt but never executed |
| 3 | [engine-ingress-anonymous-command-misordering](engine-ingress-anonymous-command-misordering.md) | murk-engine | Anonymous commands misordered when `source_id=None` but `source_seq=Some(_)` |
| 4 | [engine-adaptive-backoff-output-unused](engine-adaptive-backoff-output-unused.md) | murk-engine | Adaptive backoff computes output but result is discarded; dead code in practice |
| 5 | [engine-epoch-inconsistent-stall-snapshot](engine-epoch-inconsistent-stall-snapshot.md) | murk-engine | Stall detection reads two atomics non-atomically; false cancellation of healthy workers |
| 6 | [engine-ring-latest-spurious-none](engine-ring-latest-spurious-none.md) | murk-engine | `SnapshotRing::latest()` returns None when snapshots exist under overwrite races |
| 7 | [engine-realtime-shutdown-blocks-on-slow-tick](engine-realtime-shutdown-blocks-on-slow-tick.md) | murk-engine | Shutdown blocks indefinitely with low `tick_rate_hz` due to uninterruptible sleep |
| 8 | [engine-overlay-empty-buffer-conflation](engine-overlay-empty-buffer-conflation.md) | murk-engine | Overlay cache conflates missing field with present-but-empty field |
| 9 | [engine-backoff-config-not-validated](engine-backoff-config-not-validated.md) | murk-engine | `BackoffConfig` invariants not checked; `initial_max_skew > max_skew_cap` allowed |
| 10 | [arena-generation-counter-overflow](arena-generation-counter-overflow.md) | murk-arena | u32 generation overflows after ~4B ticks; panic in debug, wrap breaks correctness in release |
| 11 | [arena-sparse-cow-generation-rollover](arena-sparse-cow-generation-rollover.md) | murk-arena | Sparse CoW `<` comparison fails after generation wrap; silent field data loss |
| 12 | [arena-missing-segment-size-validation](arena-missing-segment-size-validation.md) | murk-arena | `segment_size` power-of-two/minimum constraints documented but never enforced |
| 13 | [arena-placeholder-pertick-handles-in-snapshot](arena-placeholder-pertick-handles-in-snapshot.md) | murk-arena | Placeholder handles readable via snapshot before any begin_tick/publish cycle |
| 14 | [arena-static-arena-duplicate-field-ids](arena-static-arena-duplicate-field-ids.md) | murk-arena | Duplicate FieldIds silently accepted; earlier allocations orphaned |
| 15 | [propagator-writemode-incremental-not-implemented](propagator-writemode-incremental-not-implemented.md) | murk-propagator | `WriteMode::Incremental` documented but never implemented; buffers always zero-initialized |
| 16 | [propagator-diffusion-cfl-hardcoded-degree](propagator-diffusion-cfl-hardcoded-degree.md) | murk-propagators | CFL bound hardcodes degree=4; numerically unstable on Hex2D (6) and Fcc12 (12) |
| 17 | [obs-plan-fast-path-unchecked-index-panic](obs-plan-fast-path-unchecked-index-panic.md) | murk-obs | Fast-path gather uses unchecked indexing; short field buffer panics |
| 18 | [obs-pool-nan-produces-infinity](obs-pool-nan-produces-infinity.md) | murk-obs | pool_2d emits -inf/+inf with valid mask when all window cells are NaN |
| 19 | [ffi-trampoline-null-pointer-dereference](ffi-trampoline-null-pointer-dereference.md) | murk-ffi | Trampoline functions dereference out_ptr/out_len without null checks |
| 20 | [ffi-mutex-poisoning-panic-in-extern-c](ffi-mutex-poisoning-panic-in-extern-c.md) | murk-ffi | 43+ `lock().unwrap()` calls in extern "C" functions; poisoned mutex = UB |
| 21 | [ffi-productspace-unchecked-float-cast](ffi-productspace-unchecked-float-cast.md) | murk-ffi | f64→usize cast without validation; INFINITY becomes usize::MAX, panics in Vec |
| 22 | [ffi-obs-negative-to-unsigned-cast](ffi-obs-negative-to-unsigned-cast.md) | murk-ffi | Negative i32 params cast to u32/usize; -1 becomes u32::MAX |
| 23 | [python-propagator-trampoline-leak-on-cstring-error](python-propagator-trampoline-leak-on-cstring-error.md) | murk-python | TrampolineData leaks on CString/add_propagator_handle error paths |
| 24 | [space-product-weighted-metric-truncation](space-product-weighted-metric-truncation.md) | murk-space | Weighted metric silently drops trailing component distances via zip truncation |

### Medium (24)

| # | Ticket | Crate | Summary |
|---|--------|-------|---------|
| 25 | [engine-egress-double-epoch-read](engine-egress-double-epoch-read.md) | murk-engine | Double epoch read allows age_ticks overstated by 1+ |
| 26 | [engine-stepresult-receipts-doc-mismatch](engine-stepresult-receipts-doc-mismatch.md) | murk-engine | StepResult.receipts doc says no rejections but code includes them |
| 27 | [engine-quickstart-setfield-noop](engine-quickstart-setfield-noop.md) | murk-engine | Quickstart example's SetField injection overwritten by diffusion propagator |
| 28 | [arena-segment-slice-beyond-cursor](arena-segment-slice-beyond-cursor.md) | murk-arena | Segment::slice checks capacity not cursor; reads stale data |
| 29 | [arena-sparse-segment-memory-leak](arena-sparse-segment-memory-leak.md) | murk-arena | Sparse CoW bump-allocates but never reclaims dead segment memory |
| 30 | [propagator-agent-movement-tick0-actions](propagator-agent-movement-tick0-actions.md) | murk-propagators | Actions processed on tick 0 alongside initialization |
| 31 | [propagator-pipeline-nan-maxdt-bypass](propagator-pipeline-nan-maxdt-bypass.md) | murk-propagator | NaN from max_dt() bypasses all stability constraints |
| 32 | [propagator-reward-stale-heat-gradient-dependency](propagator-reward-stale-heat-gradient-dependency.md) | murk-propagators | reads() declares HEAT_GRADIENT but step() never reads it |
| 33 | [obs-flatbuf-silent-u16-truncation](obs-flatbuf-silent-u16-truncation.md) | murk-obs | serialize truncates entry count to u16 but writes all entries |
| 34 | [obs-geometry-is-interior-missing-dim-check](obs-geometry-is-interior-missing-dim-check.md) | murk-obs | is_interior missing dimension validation; empty center returns true |
| 35 | [replay-codec-unbounded-alloc-from-wire](replay-codec-unbounded-alloc-from-wire.md) | murk-replay | decode_frame allocates from untrusted u32 lengths; DoS vector |
| 36 | [replay-hash-empty-snapshot-returns-nonzero](replay-hash-empty-snapshot-returns-nonzero.md) | murk-replay | Doc says returns 0 for no fields but returns FNV_OFFSET |
| 37 | [ffi-accessor-ambiguous-zero-return](ffi-accessor-ambiguous-zero-return.md) | murk-ffi | World accessors return 0 for invalid handles, indistinguishable from valid state |
| 38 | [ffi-handle-generation-wraparound](ffi-handle-generation-wraparound.md) | murk-ffi | u32 generation wraps after 4B cycles; ABA handle resurrection |
| 39 | [python-metrics-race-between-step-and-propagator-query](python-metrics-race-between-step-and-propagator-query.md) | murk-python | Per-propagator timings fetched via separate FFI call; race with concurrent step |
| 40 | [python-vecenv-false-sb3-compatibility-claim](python-vecenv-false-sb3-compatibility-claim.md) | murk-python | MurkVecEnv claims SB3 compatibility but follows Gymnasium conventions |
| 41 | [python-command-docstring-expiry-default-mismatch](python-command-docstring-expiry-default-mismatch.md) | murk-python | Docstring says "0 = never" but default is u64::MAX; 0 means immediate expiry |
| 42 | [python-error-hints-reference-unexposed-config](python-error-hints-reference-unexposed-config.md) | murk-python | Error hints reference config knobs not exposed in Python API |
| 43 | [example-warmup-ticks-shorten-episode-length](example-warmup-ticks-shorten-episode-length.md) | examples | Warmup ticks consume global tick budget; episodes 27-31% shorter than MAX_STEPS |
| 44 | [space-fcc12-parity-overflow](space-fcc12-parity-overflow.md) | murk-space | (x+y+z)%2 overflows i32 at extreme coordinates |
| 45 | [space-hex2d-disk-overflow](space-hex2d-disk-overflow.md) | murk-space | compile_hex_disk i64 overflow when radius near i32::MAX |
| 46 | [space-compliance-ordering-membership](space-compliance-ordering-membership.md) | murk-space | Compliance test checks cardinality/uniqueness but not cell membership |

### Low (6)

| # | Ticket | Crate | Summary |
|---|--------|-------|---------|
| 47 | [arena-scratch-alloc-overflow](arena-scratch-alloc-overflow.md) | murk-arena | Unchecked `*2` growth can overflow usize (practically unreachable on 64-bit) |
| 48 | [obs-metadata-doc-says-six-fields](obs-metadata-doc-says-six-fields.md) | murk-obs | Doc says "six fields" but struct has five |
| 49 | [replay-compare-sentinel-zero-divergence](replay-compare-sentinel-zero-divergence.md) | murk-replay | Length/presence mismatches reported with hardcoded 0.0 sentinel |
| 50 | [core-command-ordering-doc-missing-source-seq](core-command-ordering-doc-missing-source-seq.md) | murk-core | Command ordering doc omits source_seq from sort key description |
| 51 | [bench-space-ops-degenerate-q-distribution](bench-space-ops-degenerate-q-distribution.md) | murk-bench | LCG multiplier not coprime to modulus; only 4/20 q values exercised |
| 52 | [script-organize-by-priority-basename-collision](script-organize-by-priority-basename-collision.md) | scripts | --organize-by-priority flattens paths; duplicate basenames overwrite |

## By Crate

| Crate | Critical | High | Medium | Low | Total |
|-------|----------|------|--------|-----|-------|
| murk-engine | 0 | 8 | 3 | 0 | 11 |
| murk-arena | 0 | 5 | 2 | 1 | 8 |
| murk-ffi | 0 | 4 | 2 | 0 | 6 |
| murk-python | 1 | 1 | 4 | 0 | 6 |
| murk-propagator | 0 | 1 | 2 | 1 | 4 |
| murk-propagators | 0 | 1 | 2 | 0 | 3 |
| murk-obs | 0 | 2 | 2 | 1 | 5 |
| murk-replay | 0 | 0 | 2 | 1 | 3 |
| murk-space | 0 | 1 | 3 | 0 | 4 |
| murk-core | 0 | 0 | 0 | 1 | 1 |
| murk-bench | 0 | 0 | 0 | 1 | 1 |
| examples | 0 | 0 | 1 | 0 | 1 |
| scripts | 0 | 0 | 0 | 1 | 1 |
| **Total** | **1** | **22** | **24** | **6** | **53** |

## Triage Summaries

Detailed per-batch classification (confirmed/false-positive/design-as-intended) with rationale:

- [engine-TRIAGE-SUMMARY](engine-TRIAGE-SUMMARY.md) — 15 reports → 11 confirmed, 4 skipped
- [arena-TRIAGE-SUMMARY](arena-TRIAGE-SUMMARY.md) — 13 reports → 8 confirmed, 4 skipped, 1 doc-only
- [space-TRIAGE-SUMMARY](space-TRIAGE-SUMMARY.md) — 14 reports → 4 confirmed, 6 design-as-intended, 4 skipped
- [propagator-TRIAGE-SUMMARY](propagator-TRIAGE-SUMMARY.md) — 11 reports → 6 confirmed, 1 design-as-intended, 4 skipped
- [obs-replay-core-TRIAGE-SUMMARY](obs-replay-core-TRIAGE-SUMMARY.md) — 22 reports → 9 confirmed, 1 design-as-intended, 12 skipped
- [ffi-TRIAGE-SUMMARY](ffi-TRIAGE-SUMMARY.md) — 9 reports → 6 confirmed, 3 skipped
- [python-examples-TRIAGE-SUMMARY](python-examples-TRIAGE-SUMMARY.md) — 24 reports → 9 confirmed, 1 design-as-intended, 12 skipped, 2 declined
