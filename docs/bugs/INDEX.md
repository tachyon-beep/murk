# Bug Index

Generated 2026-02-17 from static analysis triage of 110 source reports.
Updated 2026-02-20 with wave-4 deep audit findings (#54-#94).

**Status (updated 2026-02-20):** 63 fixed, 0 partially fixed, 32 open.

## Open Bugs

### Critical (0 open)

| # | Ticket | Crate | Summary | Status |
|---|--------|-------|---------|--------|
| — | (none) | — | — | — |

### High (7 open)

| # | Ticket | Crate | Summary | Status |
|---|--------|-------|---------|--------|
| 20 | [ffi-mutex-poisoning-panic-in-extern-c](ffi-mutex-poisoning-panic-in-extern-c.md) | murk-ffi | 43+ `lock().unwrap()` calls in extern "C" functions; poisoned mutex = UB | Open |
| 23 | [python-propagator-trampoline-leak-on-cstring-error](python-propagator-trampoline-leak-on-cstring-error.md) | murk-python | TrampolineData leaks on CString/add_propagator_handle error paths | Open |
| 60 | [propagators-resolve-axis-duplicated](propagators-resolve-axis-duplicated.md) | murk-propagators | `resolve_axis`/`neighbours_flat` copy-pasted across 5 files | Open |
| 62 | [python-trampoline-panic-across-ffi](python-trampoline-panic-across-ffi.md) | murk-python | `python_trampoline` has no `catch_unwind`; panic = UB across extern "C" | Open |
| 63 | [python-missing-type-stubs-library-propagators](python-missing-type-stubs-library-propagators.md) | murk-python | `.pyi` stubs missing all 9 library propagator classes | Open |
| 64 | [bench-missing-black-box](bench-missing-black-box.md) | murk-bench | Missing `black_box` + arena benchmark TickId/generation divergence | Open |
| 66 | [ffi-callback-propagator-missing-sync](ffi-callback-propagator-missing-sync.md) | murk-ffi | `CallbackPropagator` has `Send` but not `Sync`; unsound if design changes | Open |

### Medium (21 open)

| # | Ticket | Crate | Summary | Status |
|---|--------|-------|---------|--------|
| 27 | [engine-quickstart-setfield-noop](engine-quickstart-setfield-noop.md) | murk-engine | Quickstart example's SetField injection overwritten by diffusion propagator | Open |
| 29 | [arena-sparse-segment-memory-leak](arena-sparse-segment-memory-leak.md) | murk-arena | Sparse CoW bump-allocates but never reclaims dead segment memory | Open |
| 30 | [propagator-agent-movement-tick0-actions](propagator-agent-movement-tick0-actions.md) | murk-propagators | Actions processed on tick 0 alongside initialization | Open |
| 37 | [ffi-accessor-ambiguous-zero-return](ffi-accessor-ambiguous-zero-return.md) | murk-ffi | World accessors return 0 for invalid handles, indistinguishable from valid state | Open |
| 38 | [ffi-handle-generation-wraparound](ffi-handle-generation-wraparound.md) | murk-ffi | u32 generation wraps after 4B cycles; ABA handle resurrection | Open |
| 39 | [python-metrics-race-between-step-and-propagator-query](python-metrics-race-between-step-and-propagator-query.md) | murk-python | Per-propagator timings fetched via separate FFI call; race with concurrent step | Open |
| 40 | [python-vecenv-false-sb3-compatibility-claim](python-vecenv-false-sb3-compatibility-claim.md) | murk-python | MurkVecEnv claims SB3 compatibility but follows Gymnasium conventions | Open |
| 41 | [python-command-docstring-expiry-default-mismatch](python-command-docstring-expiry-default-mismatch.md) | murk-python | Docstring says "0 = never" but default is u64::MAX; 0 means immediate expiry | Open |
| 42 | [python-error-hints-reference-unexposed-config](python-error-hints-reference-unexposed-config.md) | murk-python | Error hints reference config knobs not exposed in Python API | Open |
| 43 | [example-warmup-ticks-shorten-episode-length](example-warmup-ticks-shorten-episode-length.md) | examples | Warmup ticks consume global tick budget; episodes 27-31% shorter than MAX_STEPS | Open |
| 74 | [python-cstr-from-ptr-potential-ub](python-cstr-from-ptr-potential-ub.md) | murk-python | `CStr::from_ptr` on stack buffer; UB if FFI writes exactly 256 bytes | Open |
| 75 | [python-reset-all-no-seeds-validation](python-reset-all-no-seeds-validation.md) | murk-python | `BatchedWorld::reset_all()` doesn't validate `seeds.len() == num_worlds` | Open |
| 77 | [arena-descriptor-clone-per-tick](arena-descriptor-clone-per-tick.md) | murk-arena | `publish()` clones `FieldDescriptor` (String per field) every tick | Open |
| 78 | [ffi-config-not-consumed-on-null](ffi-config-not-consumed-on-null.md) | murk-ffi | `murk_lockstep_create` leaks config handle on null `world_out`; contradicts docs | Open |
| 79 | [ffi-inconsistent-mutex-poisoning](ffi-inconsistent-mutex-poisoning.md) | murk-ffi | `batched.rs` handles poisoning gracefully; `world.rs`/`config.rs`/`obs.rs` panic | Open |
| 80 | [ffi-usize-in-repr-c-struct](ffi-usize-in-repr-c-struct.md) | murk-ffi | `usize` in `#[repr(C)]` structs not ABI-portable; no compile-time size assertions | Open |
| 81 | [ffi-obs-conversion-duplicated](ffi-obs-conversion-duplicated.md) | murk-ffi | ObsEntry conversion logic duplicated between `obs.rs` and `batched.rs` | Open |
| 82 | [ffi-obsplan-lock-ordering](ffi-obsplan-lock-ordering.md) | murk-ffi | `OBS_PLANS` held during full observation execution; undocumented lock ordering | Open |
| 83 | [obs-per-agent-scratch-allocation](obs-per-agent-scratch-allocation.md) | murk-obs | Per-agent Vec alloc in pooled gather; fixed entries re-gathered per agent | Open |
| 86 | [python-close-skips-obsplan-destroy](python-close-skips-obsplan-destroy.md) | murk-python | `MurkEnv.close()` destroys World but not ObsPlan; `BatchedVecEnv` no double-close guard | Open |
| 87 | [python-batched-vecenv-missing-spaces](python-batched-vecenv-missing-spaces.md) | murk-python | `BatchedVecEnv` missing `observation_space`/`action_space`; breaks SB3 compat | Open |

### Low (4 open)

| # | Ticket | Crate | Summary | Status |
|---|--------|-------|---------|--------|
| 51 | [bench-space-ops-degenerate-q-distribution](bench-space-ops-degenerate-q-distribution.md) | murk-bench | LCG multiplier not coprime to modulus; only 4/20 q values exercised | Open |
| 52 | [script-organize-by-priority-basename-collision](script-organize-by-priority-basename-collision.md) | scripts | --organize-by-priority flattens paths; duplicate basenames overwrite | Open |
| 53 | [ffi-cbindgen-missing-c-header](ffi-cbindgen-missing-c-header.md) | murk-ffi | No generated C header; C consumers must hand-write 28+ extern declarations | Open |
| 94 | [propagators-performance-hotspots](propagators-performance-hotspots.md) | murk-propagators | Per-cell BFS alloc, O(n) agent scan, wasted Box-Muller samples | Open |

## Closed Bugs (63 fixed)

Tickets moved to [closed/](closed/).

| # | Ticket | Crate | Summary | Fix Commit |
|---|--------|-------|---------|------------|
| 45 | [space-hex2d-disk-overflow](closed/space-hex2d-disk-overflow.md) | murk-space | `compile_hex_disk` uses `checked_mul` for bounding area; returns `InvalidRegion` on overflow | (this session) |
| 46 | [space-compliance-ordering-membership](closed/space-compliance-ordering-membership.md) | murk-space | Compliance suite cross-validates `canonical_ordering()` against `compile_region(All)` coords | (this session) |
| 88 | [space-regionplan-public-fields](closed/space-regionplan-public-fields.md) | murk-space | `RegionPlan` fields `pub(crate)` with accessors; `cell_count` derived; `metric_distance` returns `Result` | (this session) |
| 26 | [engine-stepresult-receipts-doc-mismatch](closed/engine-stepresult-receipts-doc-mismatch.md) | murk-engine | Doc updated: StepResult.receipts includes submission-rejected receipts | (this session) |
| 48 | [obs-metadata-doc-says-six-fields](closed/obs-metadata-doc-says-six-fields.md) | murk-obs | Doc fixed: "six fields" → "five fields" | (this session) |
| 50 | [core-command-ordering-doc-missing-source-seq](closed/core-command-ordering-doc-missing-source-seq.md) | murk-core | Doc updated: sort key includes `source_seq` as third component | (this session) |
| 67 | [engine-cell-count-u32-truncation](closed/engine-cell-count-u32-truncation.md) | murk-engine | `as u32` casts replaced with `try_from()`; `ConfigError::CellCountOverflow` added | (this session) |
| 76 | [core-fieldtype-zero-dims-constructible](closed/core-fieldtype-zero-dims-constructible.md) | murk-core | `FieldDef::validate()` rejects zero dims/n_values, inverted/NaN bounds | (this session) |
| 85 | [propagator-scratch-bytes-slots-mismatch](closed/propagator-scratch-bytes-slots-mismatch.md) | murk-propagator | Doc clarified: `scratch_bytes()` returns bytes, `ScratchRegion::new()` takes slots | (this session) |
| 35 | [replay-codec-unbounded-alloc-from-wire](closed/replay-codec-unbounded-alloc-from-wire.md) | murk-replay | Decode limits added; rejects oversized strings, blobs, and command counts | (this session) |
| 72 | [replay-write-path-u32-truncation](closed/replay-write-path-u32-truncation.md) | murk-replay | All `as u32` casts replaced with `u32::try_from()`; returns `DataTooLarge` error | (this session) |
| 73 | [replay-expires-arrival-seq-not-serialized](closed/replay-expires-arrival-seq-not-serialized.md) | murk-replay | FORMAT_VERSION bumped to 3; `expires_after_tick` and `arrival_seq` now serialized per command | (this session) |
| 93 | [replay-writer-no-flush-on-drop](closed/replay-writer-no-flush-on-drop.md) | murk-replay | `Drop` impl added; flushes on drop via `Option<W>` pattern | (this session) |
| 54 | [arena-publish-no-state-guard](closed/arena-publish-no-state-guard.md) | murk-arena | `publish()` guarded by `tick_in_progress` flag; uses `next_generation` from `begin_tick()` | (this session) |
| 56 | [arena-segment-panics-in-library-code](closed/arena-segment-panics-in-library-code.md) | murk-arena | `Segment::slice`/`slice_mut` return `Option`; `SegmentList` bounds-checked | (this session) |
| 84 | [arena-cell-count-components-overflow](closed/arena-cell-count-components-overflow.md) | murk-arena | `from_field_defs` returns `Result`; `checked_mul` on `cell_count * components` | (this session) |
| 47 | [arena-scratch-alloc-overflow](closed/arena-scratch-alloc-overflow.md) | murk-arena | Growth factor uses `checked_mul(2).unwrap_or(new_cursor)` | (this session) |
| 25 | [engine-egress-double-epoch-read](closed/engine-egress-double-epoch-read.md) | murk-engine | Use snapshot.tick_id() instead of second epoch_counter.current() call | (this session) |
| 57 | [obs-flatbuf-signed-unsigned-cast-corruption](closed/obs-flatbuf-signed-unsigned-cast-corruption.md) | murk-obs | i32↔u32 casts use try_from; rejects negative/overflow on both ser/deser | (this session) |
| 71 | [obs-normalize-inverted-range](closed/obs-normalize-inverted-range.md) | murk-obs | compile() validates min <= max and finite for Normalize transform | (this session) |
| 89 | [obs-canonical-rank-negative-coord](closed/obs-canonical-rank-negative-coord.md) | murk-obs | debug_assert guards negative coords in canonical_rank | (this session) |
| 55 | [propagators-diffusion-alpha-unbounded](closed/propagators-diffusion-alpha-unbounded.md) | murk-propagators | Alpha clamped to [0,1]; constructor validates; max_dt returns None for zero | (this session) |
| 58 | [propagators-agent-presence-issues](closed/propagators-agent-presence-issues.md) | murk-propagators | Bounds removed; reads_previous declares AGENT_PRESENCE | (this session) |
| 59 | [propagators-nan-infinity-validation-gaps](closed/propagators-nan-infinity-validation-gaps.md) | murk-propagators | Validation uses `!(x >= 0.0) \|\| !x.is_finite()` pattern | (this session) |
| 68 | [engine-egress-epoch-tick-mismatch](closed/engine-egress-epoch-tick-mismatch.md) | murk-engine | Same fix as #25; snapshot carries its own tick ID | (this session) |
| 69 | [engine-observe-buffer-bounds](closed/engine-observe-buffer-bounds.md) | murk-engine | Both observe methods now return Err on buffer size mismatch | (this session) |
| 70 | [engine-reset-wrong-error-variant](closed/engine-reset-wrong-error-variant.md) | murk-engine | New ConfigError::EngineRecoveryFailed variant replaces misleading InvalidTickRate | (this session) |
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
| 61 | [space-is-multiple-of-msrv-compat](closed/space-is-multiple-of-msrv-compat.md) | murk-space, murk-obs | `is_multiple_of()` replaced with `% 2 != 0` for MSRV 1.87 compat | (this session) |
| 65 | [umbrella-snapshot-not-importable](closed/umbrella-snapshot-not-importable.md) | murk | murk-arena added as dep; Snapshot/OwnedSnapshot re-exported in prelude | (this session) |
| 90 | [workspace-unused-indexmap-deps](closed/workspace-unused-indexmap-deps.md) | murk-core, murk-replay | Removed unused `indexmap` dependency from both crates | (this session) |
| 91 | [workspace-missing-must-use](closed/workspace-missing-must-use.md) | murk-core, murk-arena, murk-propagator | `#[must_use]` added to FieldSet ops, SpaceInstanceId::next, TickGuard, etc. | (this session) |
| 92 | [workspace-error-types-missing-partialeq](closed/workspace-error-types-missing-partialeq.md) | murk-core, murk-engine, murk-space, murk-propagator | PartialEq/Eq derived bottom-up through error type dependency chain | (this session) |
| 13 | [arena-placeholder-pertick-handles-in-snapshot](closed/arena-placeholder-pertick-handles-in-snapshot.md) | murk-arena | Placeholder handles readable via snapshot before any begin_tick/publish cycle | (this session) |
| 28 | [arena-segment-slice-beyond-cursor](closed/arena-segment-slice-beyond-cursor.md) | murk-arena | Segment::slice checks capacity not cursor; reads stale data | (this session) |
| 24 | [space-product-weighted-metric-truncation](closed/space-product-weighted-metric-truncation.md) | murk-space | Weighted metric silently drops trailing component distances via zip truncation | (this session) |
| 14 | [arena-static-arena-duplicate-field-ids](closed/arena-static-arena-duplicate-field-ids.md) | murk-arena | Duplicate FieldIds silently accepted; earlier allocations orphaned | (this session) |
| 12 | [arena-missing-segment-size-validation](closed/arena-missing-segment-size-validation.md) | murk-arena | `segment_size` power-of-two/minimum constraints documented but never enforced | (this session) |
| 9 | [engine-backoff-config-not-validated](closed/engine-backoff-config-not-validated.md) | murk-engine | `BackoffConfig` invariants not checked; `initial_max_skew > max_skew_cap` allowed | (this session) |
| 4 | [engine-adaptive-backoff-output-unused](closed/engine-adaptive-backoff-output-unused.md) | murk-engine | Adaptive backoff computes output but result is discarded; dead code in practice | (this session) |
| 6 | [engine-ring-latest-spurious-none](closed/engine-ring-latest-spurious-none.md) | murk-engine | `SnapshotRing::latest()` returns None when snapshots exist under overwrite races | 0560090 |
| 7 | [engine-realtime-shutdown-blocks-on-slow-tick](closed/engine-realtime-shutdown-blocks-on-slow-tick.md) | murk-engine | Shutdown blocks indefinitely with low `tick_rate_hz` due to uninterruptible sleep | 0560090 |
| 32 | [propagator-reward-stale-heat-gradient-dependency](closed/propagator-reward-stale-heat-gradient-dependency.md) | murk-propagators | reads() declares HEAT_GRADIENT but step() never reads it | f456f7b |
| 33 | [obs-flatbuf-silent-u16-truncation](closed/obs-flatbuf-silent-u16-truncation.md) | murk-obs | serialize truncates entry count to u16 but writes all entries | 1ce373d |
| 34 | [obs-geometry-is-interior-missing-dim-check](closed/obs-geometry-is-interior-missing-dim-check.md) | murk-obs | is_interior missing dimension validation; empty center returns true | 1ce373d |
| 36 | [replay-hash-empty-snapshot-returns-nonzero](closed/replay-hash-empty-snapshot-returns-nonzero.md) | murk-replay | Doc says returns 0 for no fields but returns FNV_OFFSET | 0560090 |
| 49 | [replay-compare-sentinel-zero-divergence](closed/replay-compare-sentinel-zero-divergence.md) | murk-replay | Length/presence mismatches reported with hardcoded 0.0 sentinel | 0560090 |
| — | [propagator-scratch-byte-capacity-rounds-down](closed/propagator-scratch-byte-capacity-rounds-down.md) | murk-propagator | `with_byte_capacity()` floor division under-allocates non-aligned byte counts | f456f7b |

## By Crate (open only)

| Crate | Critical | High | Medium | Low | Total Open |
|-------|----------|------|--------|-----|------------|
| murk-engine | 0 | 0 | 1 | 0 | 1 |
| murk-arena | 0 | 0 | 2 | 0 | 2 |
| murk-ffi | 0 | 2 | 7 | 1 | 10 |
| murk-python | 0 | 3 | 8 | 0 | 11 |
| murk-propagator | 0 | 0 | 0 | 0 | 0 |
| murk-propagators | 0 | 1 | 1 | 1 | 3 |
| murk-obs | 0 | 0 | 1 | 0 | 1 |
| murk-replay | 0 | 0 | 0 | 0 | 0 |
| murk-space | 0 | 0 | 0 | 0 | 0 |
| murk-core | 0 | 0 | 0 | 0 | 0 |
| murk-bench | 0 | 1 | 0 | 1 | 2 |
| murk (umbrella) | 0 | 0 | 0 | 0 | 0 |
| examples | 0 | 0 | 1 | 0 | 1 |
| scripts | 0 | 0 | 0 | 1 | 1 |
| workspace (cross-crate) | 0 | 0 | 0 | 0 | 0 |
| **Total** | **0** | **7** | **21** | **4** | **32** |

Note: Workspace-wide tickets (#90-#92) affect multiple crates and are counted once under "workspace".

## Triage Summaries

Detailed per-batch classification (confirmed/false-positive/design-as-intended) with rationale.
These are point-in-time snapshots from the initial triage (2026-02-17); some bugs
referenced as "confirmed" have since been fixed. See the Closed table above for current status.

- [engine-TRIAGE-SUMMARY](engine-TRIAGE-SUMMARY.md) — 15 reports → 11 confirmed, 4 skipped
- [arena-TRIAGE-SUMMARY](arena-TRIAGE-SUMMARY.md) — 13 reports → 8 confirmed, 4 skipped, 1 doc-only
- [space-TRIAGE-SUMMARY](space-TRIAGE-SUMMARY.md) — 14 reports → 4 confirmed, 6 design-as-intended, 4 skipped
- [propagator-TRIAGE-SUMMARY](propagator-TRIAGE-SUMMARY.md) — 11 reports → 6 confirmed, 1 design-as-intended, 4 skipped
- [obs-replay-core-TRIAGE-SUMMARY](obs-replay-core-TRIAGE-SUMMARY.md) — 22 reports → 9 confirmed, 1 design-as-intended, 12 skipped
- [ffi-TRIAGE-SUMMARY](ffi-TRIAGE-SUMMARY.md) — 9 reports → 6 confirmed, 3 skipped
- [python-examples-TRIAGE-SUMMARY](python-examples-TRIAGE-SUMMARY.md) — 24 reports → 9 confirmed, 1 design-as-intended, 12 skipped, 2 declined
