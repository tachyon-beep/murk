# Static Analysis Triage Summary: murk-obs, murk-replay, murk-core

**Date:** 2026-02-17
**Triaged by:** static-analysis-triage (Opus 4.6)
**Branch:** feat/release-0.1.7
**Reports reviewed:** 22

---

## Overview

| Metric | Count |
|--------|-------|
| Reports reviewed | 22 |
| Skipped (no bug / trivial) | 12 |
| Non-trivial reports investigated | 10 |
| CONFIRMED bugs (tickets written) | 9 |
| FALSE_POSITIVE | 0 |
| DESIGN_AS_INTENDED | 1 |
| ALREADY_FIXED | 0 |

---

## Confirmed Bugs by Severity

### High (2)

| Ticket | Crate | File | Summary |
|--------|-------|------|---------|
| [obs-plan-fast-path-unchecked-index-panic](obs-plan-fast-path-unchecked-index-panic.md) | murk-obs | plan.rs:970,1018 | Fast-path gather uses unchecked indexing; panics on short field buffers instead of returning error |
| [obs-pool-nan-produces-infinity](obs-pool-nan-produces-infinity.md) | murk-obs | pool.rs:50-84 | Max/Min pooling emits -inf/+inf with valid mask when all window cells are NaN |

### Medium (3)

| Ticket | Crate | File | Summary |
|--------|-------|------|---------|
| [obs-flatbuf-silent-u16-truncation](obs-flatbuf-silent-u16-truncation.md) | murk-obs | flatbuf.rs:66,81,114-115 | Serialize silently truncates entry count via `as u16`; deserialize ignores trailing bytes |
| [replay-codec-unbounded-alloc-from-wire](replay-codec-unbounded-alloc-from-wire.md) | murk-replay | codec.rs:114-126,257-258 | Decode allocates up to 4GB from untrusted u32 lengths (DoS vector) |
| [replay-hash-empty-snapshot-returns-nonzero](replay-hash-empty-snapshot-returns-nonzero.md) | murk-replay | hash.rs:45-60 | Doc says "returns 0 for no readable fields" but code returns FNV_OFFSET |
| [obs-geometry-is-interior-missing-dim-check](obs-geometry-is-interior-missing-dim-check.md) | murk-obs | geometry.rs:124-135 | `is_interior` missing `center.len() == ndim` check; false positive for wrong-dimensional input |

### Low (3)

| Ticket | Crate | File | Summary |
|--------|-------|------|---------|
| [core-command-ordering-doc-missing-source-seq](core-command-ordering-doc-missing-source-seq.md) | murk-core | command.rs:7-10 | Doc omits `source_seq` from command ordering description |
| [obs-metadata-doc-says-six-fields](obs-metadata-doc-says-six-fields.md) | murk-obs | metadata.rs:8 | Doc says "six fields" but struct has five |
| [replay-compare-sentinel-zero-divergence](replay-compare-sentinel-zero-divergence.md) | murk-replay | compare.rs:83-100 | Length/presence mismatches reported with hardcoded 0.0 values, misleading diagnostics |

---

## DESIGN_AS_INTENDED (1)

| Report | Crate | File | Rationale |
|--------|-------|------|-----------|
| id.rs u64 wraparound | murk-core | id.rs:64 | `SpaceInstanceId::next()` wraps at u64::MAX. At 1 billion IDs/second, this takes ~584 years. Practically impossible; adding overflow detection would add unnecessary overhead to a hot path. |

---

## Skipped Reports (12 -- no bug found)

| File | Crate | Reason |
|------|-------|--------|
| error.rs | murk-core | No executable logic with defects |
| field.rs | murk-core | Bitset operations correct; property-tested |
| lib.rs | murk-core | Module declarations only |
| traits.rs | murk-core | Trait definitions only |
| cache.rs | murk-obs | Fingerprint/recompile logic correct |
| lib.rs | murk-obs | Module declarations only |
| spec.rs | murk-obs | Type declarations only |
| error.rs | murk-replay | Error enum well-formed |
| lib.rs | murk-replay | Module declarations and constants only |
| reader.rs | murk-replay | EOF/error handling correct |
| types.rs | murk-replay | Data types only |
| writer.rs | murk-replay | Write counter logic correct |

---

## Replay Determinism Impact Assessment

Two bugs have potential replay-determinism implications:

1. **replay-hash-empty-snapshot-returns-nonzero** (Medium): The doc/code mismatch does NOT affect replay correctness since both recording and replay compute the same FNV_OFFSET for empty snapshots. However, any code that special-cases hash==0 to mean "empty" would be broken. Fix by updating the doc, not the code (changing hash behavior would break existing recordings).

2. **replay-compare-sentinel-zero-divergence** (Low): Only affects diagnostic reporting quality, not divergence detection. The hash-based fast path correctly detects all mismatches; only the per-field detail report uses misleading sentinel values.

Neither bug causes silent replay divergence. The replay codec itself is sound -- its only issue is the unbounded allocation DoS vector.

---

## Recommended Fix Priority

1. **obs-pool-nan-produces-infinity** -- Highest practical impact. NaN values from boundary padding can silently inject infinity into training tensors. Fix is surgical (add `val.is_nan()` check in the Max/Min branches).

2. **obs-plan-fast-path-unchecked-index-panic** -- Any panic in the observation pipeline crashes the training process. Add a pre-flight length check on field buffers.

3. **obs-flatbuf-silent-u16-truncation** -- Silent data loss on serialize. Make serialize fallible with `try_from` checks.

4. **replay-hash-empty-snapshot-returns-nonzero** -- Update doc to match code. Do NOT change the hash value (would break replay compatibility).

5. **replay-codec-unbounded-alloc-from-wire** -- Add decode limits. Lower urgency since replay files are self-generated.

6. **obs-geometry-is-interior-missing-dim-check** -- Add dimension guard. Lower urgency since internal callers pass correct dimensions.

7. **Doc-only fixes** (command ordering, metadata field count, compare sentinel) -- batch into a single PR.
