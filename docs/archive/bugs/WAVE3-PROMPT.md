# Wave 3 Bug Fix Prompt

> Copy-paste this to kick off Wave 3 fixes. Same approach as Waves 1-2:
> Opus subagents, one file per agent, systematic-debugging skill.

## Prerequisites

Waves 1+2 must be committed. Run `cargo test` to confirm 699+ passing, 0 failures.

## Agent Mapping (8 agents, 9 bugs)

| Agent | File | Bug(s) | Summary |
|-------|------|--------|---------|
| 1 | `crates/murk-engine/src/config.rs` | #9 | BackoffConfig validation |
| 2 | `crates/murk-arena/src/config.rs` | #12 | segment_size validation |
| 3 | `crates/murk-arena/src/static_arena.rs` | #14 | Duplicate FieldId rejection |
| 4 | `crates/murk-engine/src/ring.rs` | #6 | SnapshotRing::latest() fallback scan |
| 5 | `crates/murk-engine/src/tick_thread.rs` | #4 + #7 | Wire backoff output + interruptible sleep |
| 6 | `crates/murk-arena/src/read.rs` | #13 | Placeholder handle guard |
| 7 | `crates/murk-python/src/propagator.rs` | #23 | TrampolineData leak on error |
| 8 | `crates/murk-space/src/product.rs` | #24 | Weighted metric arity check |

## Prompt

---

Please fix Wave 3 bugs (robustness). Use the same approach as Waves 1+2: parallel Opus subagents, one file per agent, systematic-debugging skill. Here are the 8 agents to dispatch:

### Agent 1: `crates/murk-engine/src/config.rs` — Bug #9 (engine-backoff-config-not-validated)

`WorldConfig::validate()` does not validate `BackoffConfig` invariants:
- `initial_max_skew > max_skew_cap` is allowed but causes runtime reset to exceed cap
- Non-finite `backoff_factor` is allowed
- `rejection_rate_threshold` outside [0.0, 1.0] is allowed

**Fix:** Add backoff validation checks to `WorldConfig::validate()`. Reject:
- `initial_max_skew > max_skew_cap`
- Non-finite or non-positive `backoff_factor`
- `rejection_rate_threshold` outside [0.0, 1.0]
- `decay_rate == 0`

Return `ConfigError` with descriptive messages.

### Agent 2: `crates/murk-arena/src/config.rs` — Bug #12 (arena-missing-segment-size-validation)

`ArenaConfig` documents that `segment_size` must be a power of two and >= 1024, but no validation enforces this. `PingPongArena::new()` only validates `max_segments >= 3`.

**Fix:** Add `ArenaConfig::validate(&self) -> Result<(), ArenaError>` enforcing:
- `segment_size >= 1024`
- `segment_size.is_power_of_two()`
- `max_generation_age >= 1`

Call it at the start of `PingPongArena::new()` — but since pingpong.rs is a different file, just add the validate method here. Add a comment noting that `PingPongArena::new()` should call it. Also consider making config fields private with a builder if feasible within this file.

### Agent 3: `crates/murk-arena/src/static_arena.rs` — Bug #14 (arena-static-arena-duplicate-field-ids)

`StaticArena::new` silently accepts duplicate `FieldId` entries, over-allocating backing storage and routing reads/writes to the last duplicate's offset, leaving earlier allocations orphaned.

**Fix:** Validate uniqueness at construction. Before the allocation loop, check for duplicate FieldIds. Either:
- Return `Result<Self, ArenaError>` and reject duplicates, OR
- Panic with a clear message (construction-time invariant violation)

Also: only advance `cursor` when inserting a truly new key.

### Agent 4: `crates/murk-engine/src/ring.rs` — Bug #6 (engine-ring-latest-spurious-none)

`SnapshotRing::latest()` returns `None` after exhausting `capacity` retry attempts under overwrite races, violating its documented guarantee to return a snapshot whenever the ring is non-empty.

**Fix:** After retry exhaustion, scan ALL slots and return the highest valid `(tag, snapshot)` as a fallback. Only return `None` for the truly empty ring (`write_pos == 0`).

### Agent 5: `crates/murk-engine/src/tick_thread.rs` — Bugs #4 + #7 (combined)

**Bug #4 (adaptive backoff output unused):** `self.backoff.record_tick(had_rejection)` is called but its return value (`effective_max_skew`) is discarded. `check_stalled_workers()` always uses the fixed `self.max_epoch_hold_ns` threshold. Wire the backoff output into stall detection.

**Bug #7 (realtime shutdown blocks on slow tick):** `std::thread::sleep` in the tick loop is uninterruptible. With low `tick_rate_hz`, shutdown blocks for the full tick budget. Replace with shutdown-aware sleep (sleep in short chunks while polling `shutdown_flag`, or use a condvar/channel wait with timeout).

These are both in tick_thread.rs and should be fixed together.

### Agent 6: `crates/murk-arena/src/read.rs` — Bug #13 (arena-placeholder-pertick-handles-in-snapshot)

`FieldDescriptor::from_field_defs` initializes PerTick fields with placeholder handles (`segment_index: 0, offset: 0, len: total_len`). These are live in `published_descriptor` immediately after `PingPongArena::new()`, so `snapshot()` before any `begin_tick()`/`publish()` reads from unallocated regions.

**Fix:** In `Snapshot::resolve_field` and `OwnedSnapshot::resolve_field`, add a validation check: verify that the handle represents a real allocation by checking `offset + len <= segment.cursor` (or a similar bounds check). Return `None` for handles pointing at unallocated regions. This converts the stale-zero read into an explicit "not available" result.

### Agent 7: `crates/murk-python/src/propagator.rs` — Bug #23 (python-propagator-trampoline-leak-on-cstring-error)

`PropagatorDef::register` calls `Box::into_raw(data)` at line ~89, converting ownership to a raw pointer. If `CString::new()` at line ~101 fails (name with interior NUL), or `config.add_propagator_handle()` at line ~141 fails, the raw pointer is never reclaimed.

**Fix:** Keep `TrampolineData` as `Box<TrampolineData>` during setup. Only call `Box::into_raw` after all fallible operations succeed. Alternatively, add explicit `Box::from_raw` cleanup in each early-return path after `Box::into_raw`.

### Agent 8: `crates/murk-space/src/product.rs` — Bug #24 (space-product-weighted-metric-truncation)

`ProductSpace::metric_distance` with `ProductMetric::Weighted` uses `Iterator::zip` which silently drops trailing component distances when `weights.len() != components.len()`.

**Fix:** Add `assert_eq!(weights.len(), self.components.len(), "Weighted metric requires exactly one weight per component")` before the zip. This is a construction-time invariant that should be caught early. Alternatively, add validation when `ProductMetric::Weighted` is passed to `metric_distance`.

This relates to CR-1 (ProductSpace semantics) from the architectural review.

---

## After All Agents Complete

1. `cargo check` — expect zero warnings
2. `cargo test` — expect 700+ passing, 0 failures
3. Fix any seam issues (signature changes breaking callers in other files)
4. Commit with message: `fix: wave 3 robustness fixes (9 bugs)`
