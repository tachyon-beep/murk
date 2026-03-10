# murk-space Static Analysis Triage Summary

**Date:** 2026-02-17
**Triaged by:** static-analysis-triage
**Branch:** feat/release-0.1.7
**Crate:** murk-space

## Overview

14 static analysis reports were triaged for the murk-space crate. 4 reports
identified no issues (trivial/no-change), 6 reports were classified as
design-as-intended, and 4 bugs were confirmed.

## Reports Triaged

| Source File | Report Severity | Verdict | Ticket |
|---|---|---|---|
| compliance.rs | major | **CONFIRMED** | [space-compliance-ordering-membership.md](space-compliance-ordering-membership.md) |
| edge.rs | trivial | SKIPPED | -- |
| error.rs | trivial | SKIPPED | -- |
| fcc12.rs | major | **CONFIRMED** | [space-fcc12-parity-overflow.md](space-fcc12-parity-overflow.md) |
| grid2d.rs | major | DESIGN_AS_INTENDED | -- |
| hex2d.rs | major | **CONFIRMED** | [space-hex2d-disk-overflow.md](space-hex2d-disk-overflow.md) |
| lib.rs | trivial | SKIPPED | -- |
| line1d.rs | major | DESIGN_AS_INTENDED | -- |
| product.rs | major | **CONFIRMED** | [space-product-weighted-metric-truncation.md](space-product-weighted-metric-truncation.md) |
| region.rs | trivial | SKIPPED | -- |
| ring1d.rs | major | DESIGN_AS_INTENDED | -- |
| space.rs | major | DESIGN_AS_INTENDED | -- |
| square4.rs | major | DESIGN_AS_INTENDED | -- |
| square8.rs | major | DESIGN_AS_INTENDED | -- |

## Confirmed Bugs (4)

### 1. ProductSpace weighted metric silently truncates on arity mismatch (HIGH)

**File:** `crates/murk-space/src/product.rs:141-143`
**Ticket:** [space-product-weighted-metric-truncation.md](space-product-weighted-metric-truncation.md)

`ProductSpace::metric_distance` with `ProductMetric::Weighted` uses `zip` to
pair component distances with weights. When the weight vector is shorter than
the number of components, trailing distances are silently dropped, producing
incorrect results. This is a public API correctness issue.

**Priority:** Fix before 0.1.7 release. Relates to CR-1 (ProductSpace semantics).

### 2. Fcc12 parity check overflows i32 for extreme dimensions (MEDIUM)

**File:** `crates/murk-space/src/fcc12.rs:169, 305, 350, 381, 419`
**Ticket:** [space-fcc12-parity-overflow.md](space-fcc12-parity-overflow.md)

`(x + y + z) % 2` overflow when coordinate sums exceed `i32::MAX`. Constructor
permits dimensions up to `i32::MAX`, enabling this overflow path. The fix is
trivial: use `(x ^ y ^ z) & 1` for parity instead of addition.

**Priority:** Low urgency (practically unreachable), but cheap to fix.

### 3. Hex2D compile_hex_disk overflows i64 for extreme radius (MEDIUM)

**File:** `crates/murk-space/src/hex2d.rs:156-157, 173`
**Ticket:** [space-hex2d-disk-overflow.md](space-hex2d-disk-overflow.md)

`side * side` (bounding area) overflows `i64` when effective radius reaches
`i32::MAX`. The radius clamp is insufficient because `(2*i32::MAX+1)^2 > i64::MAX`.

**Priority:** Low urgency (practically unreachable), but should be hardened.

### 4. Compliance test does not verify cell membership (MEDIUM)

**File:** `crates/murk-space/src/compliance.rs:76-91, 106-117`
**Ticket:** [space-compliance-ordering-membership.md](space-compliance-ordering-membership.md)

`assert_canonical_ordering_complete` checks cardinality and uniqueness but not
that the coordinates are valid cells. A broken implementation could return
wrong-but-unique coordinates and pass. Test harness gap only -- all existing
backends are correct.

**Priority:** Nice-to-have hardening. No production impact.

## Design-as-Intended Dismissals (6)

### Unchecked coordinate indexing in neighbours/distance (grid2d, line1d, ring1d, square4, square8)

Five reports flagged that `neighbours()` and `distance()` index into `Coord`
without validating dimensionality or bounds. After reviewing ALL Space
implementations, this is a consistent and intentional design pattern:

- **Every backend** (Line1D, Ring1D, Square4, Square8, Hex2D, Fcc12, ProductSpace)
  does unchecked indexing in `neighbours()` and `distance()`.
- These are hot-path trait methods called by the engine on coordinates from
  `canonical_ordering()` or `compile_region()`, which are always valid.
- Validation exists in `compile_region` paths (via `check_1d_bounds`,
  `check_2d_bounds`, `check_bounds`), which is the intended entry point for
  user-supplied coordinates.
- The `Space` trait contract implicitly requires valid coordinates for
  `neighbours()` and `distance()`. Adding validation would impose per-call
  overhead on the most performance-critical code paths.

The `grid2d::axis_distance` wrap-distance underflow (`len - diff` where
`diff > len`) is also only reachable with invalid out-of-bounds coordinates.
For valid in-bounds coordinates, `diff <= len - 1` is guaranteed.

### map_coord_to_tensor_index parallel vector assumption (space.rs)

The default implementation assumes `plan.coords.len() == plan.tensor_indices.len()`.
This invariant is maintained by all `compile_region` implementations. The public
fields on `RegionPlan` are for read access by propagators, not for manual
construction. This is not a bug.

## Skipped Reports (4)

- **edge.rs:** Enum definition only, no executable logic.
- **error.rs:** Error type definitions only, no fallible logic.
- **lib.rs:** Module declarations and re-exports only.
- **region.rs:** Data type definitions with trivially correct helpers.

## Aggregate Statistics

- **Total reports:** 14
- **Confirmed bugs:** 4 (1 High, 3 Medium)
- **Design-as-intended:** 6
- **Skipped (trivial):** 4
- **False positive rate:** 71% (10/14 reports were not actionable bugs)
