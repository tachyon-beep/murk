# Bug Index

Generated 2026-02-17 from static analysis triage of 110 source reports.
Updated 2026-02-21 with wave-4 deep audit findings (#54-#94).
Updated 2026-02-22: closed #53 (v0.1.7), #97 (v0.1.8).
Updated 2026-02-24: wave-5 static analysis (#99-#119), 21 new open bugs.

**Status (updated 2026-02-24):** 98 fixed, 0 partially fixed, 21 open.

## Open Bugs

### Critical (2 open)

| # | Ticket | Crate | Summary | Status | Wave |
|---|--------|-------|---------|--------|------|
| 99 | [engine-setfield-oob-receipt-marked-applied](engine-setfield-oob-receipt-marked-applied.md) | murk-engine | OOB SetField silently skipped but receipt reports `applied_tick_id` | open | A |
| 100 | [arena-begin-tick-reentry-sparse-corruption](arena-begin-tick-reentry-sparse-corruption.md) | murk-arena | No re-entry guard on `begin_tick()`; double call corrupts published sparse snapshots | open | A |

### High (11 open)

| # | Ticket | Crate | Summary | Status | Wave |
|---|--------|-------|---------|--------|------|
| 101 | [propagators-diffusion-generic-gradient-periodic-sign](propagators-diffusion-generic-gradient-periodic-sign.md) | murk-propagators | `step_generic` gradient uses raw coordinate deltas; wrong sign at wrap boundaries | open | A |
| 102 | [propagators-agent-movement-respawn-on-all-zero](propagators-agent-movement-respawn-on-all-zero.md) | murk-propagators | All-zero field heuristic instead of tick ID; agents respawn on any all-zero tick | open | A |
| 103 | [engine-realtime-new-panics-on-thread-spawn-failure](engine-realtime-new-panics-on-thread-spawn-failure.md) | murk-engine | `.expect()` on thread spawn panics + leaks already-started threads | open | B |
| 104 | [engine-tick-rate-hz-reciprocal-overflow](engine-tick-rate-hz-reciprocal-overflow.md) | murk-engine | Subnormal `tick_rate_hz` passes validation but `1/x = inf` panics `Duration::from_secs_f64` | open | B |
| 105 | [engine-stall-threshold-arithmetic-overflow](engine-stall-threshold-arithmetic-overflow.md) | murk-engine | Unchecked `u64` arithmetic in stall threshold / hold / grace computation | open | B |
| 106 | [engine-tick-static-field-length-overflow](engine-tick-static-field-length-overflow.md) | murk-engine | Unchecked `u32 * u32` for static field length in `TickEngine::new` bypasses arena's `checked_mul` | open | B |
| 108 | [engine-batched-obsspec-missing-field-atomicity](engine-batched-obsspec-missing-field-atomicity.md) | murk-engine | `BatchedEngine` steps worlds before observation fails on missing field | open | C |
| 109 | [arena-descriptor-duplicate-field-ids](arena-descriptor-duplicate-field-ids.md) | murk-arena | `FieldDescriptor::from_field_defs` silently accepts duplicate FieldIds (#14 only fixed StaticArena) | open | C |
| 110 | [obs-plan-agentrect-missing-halfextent-ndim-check](obs-plan-agentrect-missing-halfextent-ndim-check.md) | murk-obs | `AgentRect` half_extent not validated against `space.ndim()`; zip silently truncates | open | C |
| 111 | [obs-flatbuf-empty-coords-roundtrip-failure](obs-flatbuf-empty-coords-roundtrip-failure.md) | murk-obs | `serialize` accepts empty coords but `deserialize` rejects `ndim==0`; roundtrip failure | open | C |
| 114 | [ffi-c-header-enum-name-collision](ffi-c-header-enum-name-collision.md) | murk-ffi | Duplicate unscoped C enum constants (`Absorb`, `Clamp`, `Wrap`) make header uncompilable | open | D |

### Medium (6 open)

| # | Ticket | Crate | Summary | Status | Wave |
|---|--------|-------|---------|--------|------|
| 107 | [bench-init-agent-positions-overflow](bench-init-agent-positions-overflow.md) | murk-bench | Plain `*` instead of `wrapping_mul` in hash; panics in debug for ≥14 agents | open | B |
| 112 | [propagators-diffusion-unchecked-field-arity](propagators-diffusion-unchecked-field-arity.md) | murk-propagators | No field slice length validation before `[i*2+comp]` indexing | open | C |
| 115 | [ffi-config-edge-behavior-unchecked-f64-cast](ffi-config-edge-behavior-unchecked-f64-cast.md) | murk-ffi | Raw `as i32` casts for enum params; NaN→0 maps to valid variant | open | D |
| 116 | [ffi-write-receipts-null-buffer-count](ffi-write-receipts-null-buffer-count.md) | murk-ffi | Reports non-zero receipt count even when output buffer is null | open | D |
| 118 | [obs-geometry-graph-distance-hex-panic-short-input](obs-geometry-graph-distance-hex-panic-short-input.md) | murk-obs | Hex branch panics on short input in release builds (debug_assert elided) | open | E |
| 119 | [propagator-validate-pipeline-multi-call](propagator-validate-pipeline-multi-call.md) | murk-propagator | `validate_pipeline` calls `writes()`/`reads()` multiple times per propagator | open | E |

### Low (2 open)

| # | Ticket | Crate | Summary | Status | Wave |
|---|--------|-------|---------|--------|------|
| 113 | [propagators-agent-emission-additive-copy-before-length-check](propagators-agent-emission-additive-copy-before-length-check.md) | murk-propagators | `copy_from_slice` before length check in Additive mode | open | C |
| 117 | [engine-config-cellcountoverflow-misused-for-field-count](engine-config-cellcountoverflow-misused-for-field-count.md) | murk-engine | `CellCountOverflow` Display says "cell count" when used for field-count overflow | open | E |

## Resolution Waves

Suggested fix order clusters bugs by impact class, then by crate locality to minimize context-switching.

### Wave A — Silent Correctness (fix first)

These produce **wrong results without any error signal**. Callers cannot detect the problem.

| # | Ticket | Crate | Fix Locality |
|---|--------|-------|--------------|
| 99 | engine-setfield-oob-receipt-marked-applied | murk-engine | `tick.rs` — receipt handling |
| 100 | arena-begin-tick-reentry-sparse-corruption | murk-arena | `pingpong.rs` — add re-entry guard |
| 101 | propagators-diffusion-generic-gradient-periodic-sign | murk-propagators | `diffusion.rs` — signed displacement |
| 102 | propagators-agent-movement-respawn-on-all-zero | murk-propagators | `agent_movement.rs` — tick ID check |

### Wave B — Panic/Crash Hardening (fix second)

These **crash the process** via `.expect()` or unchecked arithmetic instead of returning errors. Four of five are in murk-engine.

| # | Ticket | Crate | Fix Locality |
|---|--------|-------|--------------|
| 103 | engine-realtime-new-panics-on-thread-spawn-failure | murk-engine | `realtime.rs` — Result propagation + rollback |
| 104 | engine-tick-rate-hz-reciprocal-overflow | murk-engine | `tick_thread.rs` — subnormal rejection |
| 105 | engine-stall-threshold-arithmetic-overflow | murk-engine | `tick_thread.rs` (same file as #104) |
| 106 | engine-tick-static-field-length-overflow | murk-engine | `tick.rs` — checked_mul |
| 107 | bench-init-agent-positions-overflow | murk-bench | `lib.rs` — wrapping_mul |

### Wave C — Validation Gaps (fix third)

Missing input/config validation that lets bad state through. Each fix is a localized guard.

| # | Ticket | Crate | Fix Locality |
|---|--------|-------|--------------|
| 108 | engine-batched-obsspec-missing-field-atomicity | murk-engine | `batched.rs` — field existence preflight |
| 109 | arena-descriptor-duplicate-field-ids | murk-arena | `descriptor.rs` — insert() return check |
| 110 | obs-plan-agentrect-missing-halfextent-ndim-check | murk-obs | `plan.rs` — ndim validation |
| 111 | obs-flatbuf-empty-coords-roundtrip-failure | murk-obs | `flatbuf.rs` — accept or reject consistently |
| 112 | propagators-diffusion-unchecked-field-arity | murk-propagators | `diffusion.rs` — length preflight |
| 113 | propagators-agent-emission-additive-copy-before-length-check | murk-propagators | `agent_emission.rs` — length check before copy |

### Wave D — FFI Boundary (fix fourth)

Affects C consumers only. The header collision (#114) blocks any C compilation.

| # | Ticket | Crate | Fix Locality |
|---|--------|-------|--------------|
| 114 | ffi-c-header-enum-name-collision | murk-ffi | `cbindgen.toml` — prefix_with_name |
| 115 | ffi-config-edge-behavior-unchecked-f64-cast | murk-ffi | `config.rs` — f64_to_i32 helper |
| 116 | ffi-write-receipts-null-buffer-count | murk-ffi | `world.rs` — null buffer guard |

### Wave E — Minor Polish (fix last)

Low-impact issues: wrong error message, edge-case panic in pub API, wasteful but correct multi-call.

| # | Ticket | Crate | Fix Locality |
|---|--------|-------|--------------|
| 117 | engine-config-cellcountoverflow-misused-for-field-count | murk-engine | `config.rs` — new error variant or message |
| 118 | obs-geometry-graph-distance-hex-panic-short-input | murk-obs | `geometry.rs` — length guard |
| 119 | propagator-validate-pipeline-multi-call | murk-propagator | `pipeline.rs` — precompute metadata

## Closed Bugs (98 fixed)

Tickets moved to [closed/](closed/).

| # | Ticket | Crate | Summary | Fix Commit |
|---|--------|-------|---------|------------|
| 53 | [ffi-cbindgen-missing-c-header](closed/ffi-cbindgen-missing-c-header.md) | murk-ffi | cbindgen auto-generates C header with 42+ functions, 8 structs, 8 enums | v0.1.7 |
| 97 | [arena-sparse-fragmentation-metric](closed/arena-sparse-fragmentation-metric.md) | murk-arena | Sparse reuse_hits/reuse_misses wired through engine, FFI, and Python | v0.1.8 |
| 29 | [arena-sparse-segment-memory-leak](closed/arena-sparse-segment-memory-leak.md) | murk-arena | Two-phase retired range reclamation in SparseSlab; pending→retired→reuse | (this session) |
| 39 | [python-metrics-race-between-step-and-propagator-query](closed/python-metrics-race-between-step-and-propagator-query.md) | murk-python | Thread-local propagator timing snapshot during step; eliminates TOCTOU race | (this session) |
| 95 | [arena-sparse-no-match-fallback-untested](closed/arena-sparse-no-match-fallback-untested.md) | murk-arena | Two regression tests: no-match fallback to bump, multi-field interleaved CoW | (this session) |
| 96 | [arena-sparse-retired-range-named-struct](closed/arena-sparse-retired-range-named-struct.md) | murk-arena | `(u16, u32, u32)` tuple replaced with named `RetiredRange` struct | (this session) |
| 77 | [arena-descriptor-clone-per-tick](closed/arena-descriptor-clone-per-tick.md) | murk-arena | `FieldMeta.name` changed to `Arc<str>`; `write()` no longer clones entire meta | (this session) |
| 83 | [obs-per-agent-scratch-allocation](closed/obs-per-agent-scratch-allocation.md) | murk-obs | Pooling scratch pre-allocated and reused across agents; fixed entries computed once and memcpy'd | (this session) |
| 52 | [script-organize-by-priority-basename-collision](closed/script-organize-by-priority-basename-collision.md) | scripts | Original script replaced by `codex_bug_hunt.py` which uses `relative_to()` correctly | (already fixed) |
| 37 | [ffi-accessor-ambiguous-zero-return](closed/ffi-accessor-ambiguous-zero-return.md) | murk-ffi | Added `_get` status-returning accessor variants; old functions documented as ambiguous | (this session) |
| 38 | [ffi-handle-generation-wraparound](closed/ffi-handle-generation-wraparound.md) | murk-ffi | Slots retired (not recycled) when generation wraps to 0; prevents ABA handle resurrection | (this session) |
| 27 | [engine-quickstart-setfield-noop](closed/engine-quickstart-setfield-noop.md) | murk-engine | Added HEAT_SOURCE command-only field; propagator reads it via reads_previous and incorporates as external forcing | (this session) |
| 43 | [example-warmup-ticks-shorten-episode-length](closed/example-warmup-ticks-shorten-episode-length.md) | examples, murk-python | `_episode_start_tick` tracking in base class; `_check_truncated` uses episode-relative ticks | (this session) |
| 20 | [ffi-mutex-poisoning-panic-in-extern-c](closed/ffi-mutex-poisoning-panic-in-extern-c.md) | murk-ffi | `ffi_lock!` macro replaces 43+ `.lock().unwrap()` calls; poisoned mutex returns `InternalError` | (this session) |
| 79 | [ffi-inconsistent-mutex-poisoning](closed/ffi-inconsistent-mutex-poisoning.md) | murk-ffi | All modules now use `ffi_lock!` macro; consistent `InternalError` on poisoned mutex | (this session) |
| 82 | [ffi-obsplan-lock-ordering](closed/ffi-obsplan-lock-ordering.md) | murk-ffi | `ObsPlanState` wrapped in `Arc<Mutex<>>`; global table locks never held during execution | (this session) |
| 81 | [ffi-obs-conversion-duplicated](closed/ffi-obs-conversion-duplicated.md) | murk-ffi | Shared `convert_obs_entry` extracted; `batched.rs` imports from `obs.rs` | (this session) |
| 78 | [ffi-config-not-consumed-on-null](closed/ffi-config-not-consumed-on-null.md) | murk-ffi | Config consumed before null check; ownership contract honoured on all paths | (this session) |
| 80 | [ffi-usize-in-repr-c-struct](closed/ffi-usize-in-repr-c-struct.md) | murk-ffi | `usize` → `u64` in `MurkStepMetrics`/`MurkStepContext`; compile-time size assertions added | (this session) |
| 66 | [ffi-callback-propagator-missing-sync](closed/ffi-callback-propagator-missing-sync.md) | murk-ffi | Deliberate `!Sync` documented; `Mutex<LockstepWorld>` identified as load-bearing invariant | (this session) |
| 63 | [python-missing-type-stubs-library-propagators](closed/python-missing-type-stubs-library-propagators.md) | murk-python | `.pyi` stubs added for all 9 library propagator classes | (this session) |
| 40 | [python-vecenv-false-sb3-compatibility-claim](closed/python-vecenv-false-sb3-compatibility-claim.md) | murk-python | Docstring corrected: follows Gymnasium conventions, not SB3 VecEnv | (this session) |
| 42 | [python-error-hints-reference-unexposed-config](closed/python-error-hints-reference-unexposed-config.md) | murk-python | Error hints for codes -4, -6, -11, -14 rewritten to reference available Python actions | (this session) |
| 87 | [python-batched-vecenv-missing-spaces](closed/python-batched-vecenv-missing-spaces.md) | murk-python | `observation_space`/`action_space` added as constructor params with auto-derived default | (this session) |
| 64 | [bench-missing-black-box](closed/bench-missing-black-box.md) | murk-bench | `black_box` added to all `step_sync` results; arena benchmarks use incrementing TickId | (this session) |
| 51 | [bench-space-ops-degenerate-q-distribution](closed/bench-space-ops-degenerate-q-distribution.md) | murk-bench | LCG q multiplier changed to coprime-to-20 value; all 20 q values now exercised | (this session) |
| 62 | [python-trampoline-panic-across-ffi](closed/python-trampoline-panic-across-ffi.md) | murk-python | `catch_unwind` added to `python_trampoline`; panic no longer crosses extern "C" | (this session) |
| 23 | [python-propagator-trampoline-leak-on-cstring-error](closed/python-propagator-trampoline-leak-on-cstring-error.md) | murk-python | `CString::new` moved before `Box::into_raw`; cleanup on `add_propagator_handle` failure | (this session) |
| 74 | [python-cstr-from-ptr-potential-ub](closed/python-cstr-from-ptr-potential-ub.md) | murk-python | `CStr::from_ptr` replaced with safe `CStr::from_bytes_until_nul` | (this session) |
| 75 | [python-reset-all-no-seeds-validation](closed/python-reset-all-no-seeds-validation.md) | murk-python | `reset_all()` validates `seeds.len() == num_worlds` before FFI call | (this session) |
| 86 | [python-close-skips-obsplan-destroy](closed/python-close-skips-obsplan-destroy.md) | murk-python | `close()` destroys ObsPlan before World; `BatchedVecEnv` guards double-close | (this session) |
| 41 | [python-command-docstring-expiry-default-mismatch](closed/python-command-docstring-expiry-default-mismatch.md) | murk-python | Docstring updated: default is u64::MAX (never), 0 = expires immediately | (this session) |
| 60 | [propagators-resolve-axis-duplicated](closed/propagators-resolve-axis-duplicated.md) | murk-propagators | `resolve_axis`/`neighbours_flat` extracted to `grid_helpers.rs`; 5 files de-duplicated | (this session) |
| 30 | [propagator-agent-movement-tick0-actions](closed/propagator-agent-movement-tick0-actions.md) | murk-propagators | Early return after tick-0 initialization prevents action processing on init tick | (this session) |
| 94 | [propagators-performance-hotspots](closed/propagators-performance-hotspots.md) | murk-propagators | BFS containers pre-allocated; agent lookup via HashMap; Box-Muller deferred (replay compat) | (this session) |
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
| murk-engine | 1 | 5 | 0 | 1 | 7 |
| murk-arena | 1 | 1 | 0 | 0 | 2 |
| murk-ffi | 0 | 1 | 2 | 0 | 3 |
| murk-python | 0 | 0 | 0 | 0 | 0 |
| murk-propagator | 0 | 0 | 1 | 0 | 1 |
| murk-propagators | 0 | 2 | 1 | 1 | 4 |
| murk-obs | 0 | 2 | 1 | 0 | 3 |
| murk-replay | 0 | 0 | 0 | 0 | 0 |
| murk-space | 0 | 0 | 0 | 0 | 0 |
| murk-core | 0 | 0 | 0 | 0 | 0 |
| murk-bench | 0 | 0 | 1 | 0 | 1 |
| murk (umbrella) | 0 | 0 | 0 | 0 | 0 |
| examples | 0 | 0 | 0 | 0 | 0 |
| scripts | 0 | 0 | 0 | 0 | 0 |
| workspace (cross-crate) | 0 | 0 | 0 | 0 | 0 |
| **Total** | **2** | **11** | **6** | **2** | **21** |

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
