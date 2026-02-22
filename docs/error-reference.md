# Murk Error Code Reference

Complete reference of all error types in the Murk simulation framework, organized by subsystem.

**How to read this document:** Each error type has a quick-reference table for scanning, followed by detailed explanations of cause and remediation. HLD codes reference the High-Level Design document section 9.7 where applicable.

---

## Table of Contents

- [StepError (murk-core)](#steperror)
- [PropagatorError (murk-core)](#propagatorerror)
- [IngressError (murk-core)](#ingresserror)
- [ObsError (murk-core)](#obserror)
- [ConfigError (murk-engine)](#configerror)
- [PipelineError (murk-propagator)](#pipelineerror)
- [ArenaError (murk-arena)](#arenaerror)
- [SpaceError (murk-space)](#spaceerror)
- [ReplayError (murk-replay)](#replayerror)
- [SubmitError (murk-engine)](#submiterror)
- [BatchError (murk-engine)](#batcherror)
- [Panicked (FFI status)](#panicked)

---

## StepError

**Crate:** `murk-core` | **File:** `crates/murk-core/src/error.rs`

Errors returned by the tick engine during `step()`. Corresponds to the TickEngine and Pipeline subsystem codes in HLD section 9.7.

### Quick reference

| Variant | HLD Code | Description |
|---------|----------|-------------|
| `PropagatorFailed { name, reason }` | `MURK_ERROR_PROPAGATOR_FAILED` | A propagator returned an error during execution |
| `AllocationFailed` | `MURK_ERROR_ALLOCATION_FAILED` | Arena out-of-memory during generation staging |
| `TickRollback` | `MURK_ERROR_TICK_ROLLBACK` | Current tick rolled back due to propagator failure |
| `TickDisabled` | `MURK_ERROR_TICK_DISABLED` | Ticking disabled after consecutive rollbacks (Decision J) |
| `DtOutOfRange` | `MURK_ERROR_DT_OUT_OF_RANGE` | Requested dt exceeds a propagator's `max_dt` constraint |
| `ShuttingDown` | `MURK_ERROR_SHUTTING_DOWN` | World is in the shutdown state machine (Decision E) |

### Details

**`PropagatorFailed { name: String, reason: PropagatorError }`**

A propagator returned an error during execution. The `name` field identifies the failing propagator and `reason` contains the underlying `PropagatorError`.

Remediation:
1. Inspect the wrapped [`PropagatorError`](#propagatorerror) for details.
2. Check propagator inputs (field values, dt) for validity.
3. The tick engine will roll back the tick automatically.

**`AllocationFailed`**

Arena allocation failed due to out-of-memory during generation staging.

Remediation:
1. Reduce field count or cell count.
2. Increase arena segment pool capacity.
3. Check for epoch reclamation stalls preventing segment reuse (see [`ObsError::WorkerStalled`](#obserror)).

**`TickRollback`**

The current tick was rolled back due to a propagator failure. All staged writes are discarded and the world state reverts to the previous generation.

Remediation:
1. Transient: retry on the next tick.
2. Persistent: investigate the failing propagator via `PropagatorFailed`.
3. Note: commands submitted during a rolled-back tick are dropped.

**`TickDisabled`**

Ticking has been disabled after consecutive rollbacks (Decision J). The engine enters a fail-stop state to prevent cascading failures.

Remediation:
1. The simulation must be reset or reconstructed.
2. Investigate the root cause of repeated propagator failures before restarting.

**`DtOutOfRange`**

The requested dt exceeds a propagator's `max_dt` constraint (CFL condition or similar stability limit).

Remediation:
1. Reduce the configured dt to be at or below the tightest `max_dt` across all propagators.
2. Check [`PipelineError::DtTooLarge`](#pipelineerror) for which propagator constrains it.

**`ShuttingDown`**

The world is in the shutdown state machine (Decision E). No further ticks will be executed.

Remediation:
1. Expected during graceful shutdown. Do not retry; the world is terminating.

---

## PropagatorError

**Crate:** `murk-core` | **File:** `crates/murk-core/src/error.rs`

Errors from individual propagator execution. Returned by `Propagator::step()` and wrapped in [`StepError::PropagatorFailed`](#steperror) by the tick engine.

### Quick reference

| Variant | HLD Code | Description |
|---------|----------|-------------|
| `ExecutionFailed { reason }` | `MURK_ERROR_PROPAGATOR_FAILED` | Propagator's step function failed |
| `NanDetected { field_id, cell_index }` | -- | NaN detected in propagator output |
| `ConstraintViolation { constraint }` | -- | User-defined constraint violated |

### Details

**`ExecutionFailed { reason: String }`**

The propagator's step function failed. The `reason` field contains a human-readable description.

Remediation:
1. Inspect the reason string.
2. Common causes: invalid field state, numerical instability, domain-specific constraint violations.

**`NanDetected { field_id: FieldId, cell_index: Option<usize> }`**

NaN detected in propagator output during sentinel checking. `field_id` identifies the affected field; `cell_index` pinpoints the first NaN cell if known.

Remediation:
1. Reduce dt to improve numerical stability.
2. Add clamping or bounds to the propagator logic.
3. Check for division-by-zero in the propagator.

**`ConstraintViolation { constraint: String }`**

A user-defined constraint was violated during propagator execution.

Remediation:
1. Review the constraint definition and the field state that triggered it.
2. May indicate an out-of-bounds physical quantity or a domain invariant violation.

---

## IngressError

**Crate:** `murk-core` | **File:** `crates/murk-core/src/error.rs`

Errors from the ingress (command submission) pipeline. Used in `Receipt::reason_code` to explain why a command was rejected.

### Quick reference

| Variant | HLD Code | Description |
|---------|----------|-------------|
| `QueueFull` | `MURK_ERROR_QUEUE_FULL` | Command queue at capacity |
| `Stale` | `MURK_ERROR_STALE` | Command's `basis_tick_id` too old |
| `TickRollback` | `MURK_ERROR_TICK_ROLLBACK` | Tick rolled back; commands dropped |
| `TickDisabled` | `MURK_ERROR_TICK_DISABLED` | Ticking disabled after consecutive rollbacks |
| `ShuttingDown` | `MURK_ERROR_SHUTTING_DOWN` | World is shutting down |

### Details

**`QueueFull`**

The command queue is at capacity. The ingress pipeline cannot buffer any more commands until the tick engine drains the queue.

Remediation:
1. Reduce command submission rate.
2. Increase `max_ingress_queue` in `WorldConfig`.
3. In RL training, this may indicate the agent is submitting faster than the tick rate.

**`Stale`**

The command's `basis_tick_id` is too old relative to the current tick. The adaptive backoff mechanism rejected it due to excessive skew.

Remediation:
1. Resubmit the command with a fresh basis tick.
2. If occurring frequently, the agent's observation-to-action latency is too high relative to the tick rate.
3. Adjust backoff parameters in `BackoffConfig` to control the tolerance.

**`TickRollback`**

The tick was rolled back; commands submitted during that tick were dropped.

Remediation:
1. Resubmit the command on the next tick.
2. This is a transient condition.

Note: This shares the same HLD code as [`StepError::TickRollback`](#steperror) since both originate from the same tick rollback event.

**`TickDisabled`**

Ticking is disabled after consecutive rollbacks. No commands will be accepted until the world is reset.

Remediation:
1. Reset the simulation.
2. Investigate the root cause of repeated tick rollbacks (see [`StepError::TickDisabled`](#steperror)).

**`ShuttingDown`**

The world is shutting down. No further commands are accepted.

Remediation:
1. Expected during graceful shutdown. Do not retry.

---

## ObsError

**Crate:** `murk-core` | **File:** `crates/murk-core/src/error.rs`

Errors from the observation (egress) pipeline. Covers ObsPlan compilation, execution, and snapshot access failures.

### Quick reference

| Variant | HLD Code | Description |
|---------|----------|-------------|
| `PlanInvalidated { reason }` | `MURK_ERROR_PLAN_INVALIDATED` | ObsPlan generation does not match current snapshot |
| `TimeoutWaitingForTick` | `MURK_ERROR_TIMEOUT_WAITING_FOR_TICK` | Exact-tick egress request timed out (RealtimeAsync only) |
| `NotAvailable` | `MURK_ERROR_NOT_AVAILABLE` | Requested tick evicted from snapshot ring buffer |
| `InvalidComposition { reason }` | `MURK_ERROR_INVALID_COMPOSITION` | ObsPlan `valid_ratio` below 0.35 threshold |
| `ExecutionFailed { reason }` | `MURK_ERROR_EXECUTION_FAILED` | ObsPlan execution failed mid-fill |
| `InvalidObsSpec { reason }` | `MURK_ERROR_INVALID_OBSSPEC` | Malformed ObsSpec at compilation time |
| `WorkerStalled` | `MURK_ERROR_WORKER_STALLED` | Egress worker exceeded `max_epoch_hold` budget |

### Details

**`PlanInvalidated { reason: String }`**

The ObsPlan's generation does not match the current snapshot. This occurs when the world topology or field layout has changed since the plan was compiled.

Remediation:
1. Recompile the ObsPlan via `ObsPlan::compile()` against the current snapshot.
2. Plans are invalidated by world resets or structural changes.

**`TimeoutWaitingForTick`**

An exact-tick egress request timed out. Only occurs in RealtimeAsync mode when waiting for a specific tick that has not yet been produced.

Remediation:
1. Increase the timeout budget.
2. Check if the tick thread is stalled or running slower than expected.
3. This error does not occur in Lockstep mode.

**`NotAvailable`**

The requested tick has been evicted from the snapshot ring buffer. The ring only retains the most recent `ring_buffer_size` snapshots.

Remediation:
1. Increase `ring_buffer_size` in `WorldConfig`.
2. Alternatively, consume observations more promptly so they are not evicted before access.

**`InvalidComposition { reason: String }`**

The ObsPlan's `valid_ratio` is below the 0.35 threshold. Too many entries in the observation spec reference invalid or out-of-bounds regions.

Remediation:
1. Review the `ObsSpec` entries.
2. Ensure field IDs and region specifications are valid for the current world configuration.
3. The 0.35 threshold means at least 35% of entries must be valid.

**`ExecutionFailed { reason: String }`**

ObsPlan execution failed mid-fill. An error occurred while extracting field data into the output buffer.

Remediation:
1. Inspect the reason string.
2. Common causes: snapshot was reclaimed during execution, arena error, or malformed plan.
3. If caused by reclamation, see [`WorkerStalled`](#obserror) and epoch hold settings.

**`InvalidObsSpec { reason: String }`**

Malformed ObsSpec detected at compilation time. The observation specification contains structural errors.

Remediation:
1. Review the ObsSpec structure: check field IDs, region definitions, transforms, and dtypes.
2. Fix the spec before recompiling.

**`WorkerStalled`**

An egress worker exceeded the `max_epoch_hold` budget (default 100ms). The epoch reclamation system forcibly unpinned the worker to prevent blocking arena garbage collection.

Remediation:
1. Reduce observation complexity.
2. Or increase `max_epoch_hold_ms` in `AsyncConfig`.
3. A stalled worker prevents epoch advancement, which blocks arena segment reclamation (see [`ArenaError::CapacityExceeded`](#arenaerror)).

---

## ConfigError

**Crate:** `murk-engine` | **File:** `crates/murk-engine/src/config.rs`

Errors detected during `WorldConfig::validate()` at startup time. These are structural invariant violations that prevent world construction.

### Quick reference

| Variant | HLD Code | Description |
|---------|----------|-------------|
| `Pipeline(PipelineError)` | -- | Propagator pipeline validation failed |
| `Arena(ArenaError)` | -- | Arena configuration is invalid |
| `EmptySpace` | -- | Space has zero cells |
| `NoFields` | -- | No fields registered |
| `RingBufferTooSmall { configured }` | -- | `ring_buffer_size` below minimum of 2 |
| `IngressQueueZero` | -- | `max_ingress_queue` is zero |
| `InvalidTickRate { value }` | -- | `tick_rate_hz` is NaN, infinite, zero, or negative |

### Details

**`Pipeline(PipelineError)`**

Propagator pipeline validation failed. Wraps a [`PipelineError`](#pipelineerror).

Remediation:
1. Inspect the inner `PipelineError` for the specific pipeline issue.

**`Arena(ArenaError)`**

Arena configuration is invalid. Wraps an [`ArenaError`](#arenaerror).

Remediation:
1. Inspect the inner `ArenaError` for the specific arena issue.

**`EmptySpace`**

The configured `Space` has zero cells. A simulation requires at least one spatial cell.

Remediation:
1. Provide a space with at least one cell.
2. Check the space constructor arguments.

**`NoFields`**

No fields are registered in the configuration. A simulation requires at least one `FieldDef`.

Remediation:
1. Add at least one field definition to `WorldConfig::fields`.

**`RingBufferTooSmall { configured: usize }`**

The `ring_buffer_size` is below the minimum of 2. The snapshot ring requires at least 2 slots for double-buffering.

Remediation:
1. Set `ring_buffer_size` to 2 or greater. Default is 8.

**`IngressQueueZero`**

The `max_ingress_queue` capacity is zero. The ingress pipeline requires at least one slot.

Remediation:
1. Set `max_ingress_queue` to 1 or greater. Default is 1024.

**`InvalidTickRate { value: f64 }`**

`tick_rate_hz` is NaN, infinite, zero, or negative. Must be a finite positive number.

Remediation:
1. Provide a valid positive finite `tick_rate_hz` value.
2. Or set it to `None` for no rate limiting.

---

## PipelineError

**Crate:** `murk-propagator` | **File:** `crates/murk-propagator/src/pipeline.rs`

Errors from pipeline validation at startup. These are checked once by `validate_pipeline()` and prevent world construction if any are detected.

### Quick reference

| Variant | HLD Code | Description |
|---------|----------|-------------|
| `EmptyPipeline` | -- | No propagators registered |
| `WriteConflict(Vec<WriteConflict>)` | -- | Two or more propagators write the same field |
| `UndefinedField { propagator, field_id }` | -- | Propagator references an undefined field |
| `DtTooLarge { configured_dt, max_supported, constraining_propagator }` | -- | Configured dt exceeds a propagator's `max_dt` |
| `InvalidDt { value }` | -- | Configured dt is NaN, infinity, zero, or negative |

### Details

**`EmptyPipeline`**

No propagators are registered in the pipeline. At least one propagator is required.

Remediation:
1. Add at least one propagator to `WorldConfig::propagators`.

**`WriteConflict(Vec<WriteConflict>)`**

Two or more propagators write the same field. Each `WriteConflict` contains `field_id`, `first_writer`, and `second_writer`.

Remediation:
1. Ensure each `FieldId` is written by at most one propagator.
2. Restructure propagators so that field ownership is exclusive.

The `WriteConflict` struct:

| Field | Type | Description |
|-------|------|-------------|
| `field_id` | `FieldId` | The contested field |
| `first_writer` | `String` | Name of the first writer (earlier in pipeline order) |
| `second_writer` | `String` | Name of the second writer (later in pipeline order) |

**`UndefinedField { propagator: String, field_id: FieldId }`**

A propagator references (reads, reads_previous, or writes) a field that is not defined in the world's field list.

Remediation:
1. Register the missing `FieldId` in `WorldConfig::fields`.
2. Or update the propagator to reference only defined fields.

**`DtTooLarge { configured_dt: f64, max_supported: f64, constraining_propagator: String }`**

The configured dt exceeds a propagator's `max_dt` constraint. The `constraining_propagator` field identifies which propagator has the tightest limit.

Remediation:
1. Reduce `WorldConfig::dt` to at or below `max_supported`.
2. The tightest `max_dt` across all propagators determines the upper bound.

Note: At runtime, this condition surfaces as [`StepError::DtOutOfRange`](#steperror).

**`InvalidDt { value: f64 }`**

The configured dt is not a valid timestep: NaN, infinity, zero, or negative.

Remediation:
1. Provide a finite positive dt value in `WorldConfig::dt`.

---

## ArenaError

**Crate:** `murk-arena` | **File:** `crates/murk-arena/src/error.rs`

Errors from arena operations. The arena manages generational allocation of field data for the snapshot ring buffer.

### Quick reference

| Variant | HLD Code | Description |
|---------|----------|-------------|
| `CapacityExceeded { requested, capacity }` | -- | Segment pool full, cannot allocate |
| `StaleHandle { handle_generation, oldest_live }` | -- | FieldHandle from a reclaimed generation |
| `UnknownField { field }` | -- | Unregistered FieldId referenced |
| `NotWritable { field }` | -- | Field mutability does not permit writes |
| `InvalidConfig { reason }` | -- | Arena configuration is invalid |

### Details

**`CapacityExceeded { requested: usize, capacity: usize }`**

The segment pool is full and cannot allocate the requested number of bytes. All segments are in use by live generations.

Remediation:
1. Increase arena capacity.
2. Ensure epoch reclamation is running -- workers must unpin epochs so old generations can be freed (see [`ObsError::WorkerStalled`](#obserror)).
3. Reduce `ring_buffer_size` to decrease the number of live generations.

**`StaleHandle { handle_generation: u32, oldest_live: u32 }`**

A `FieldHandle` from a generation that has already been reclaimed was used. The handle's generation predates the oldest live generation.

Remediation:
1. This indicates a use-after-free bug in handle management.
2. Ensure handles are not cached across generation boundaries.
3. Check that observation plans are recompiled after resets (see [`ObsError::PlanInvalidated`](#obserror)).

**`UnknownField { field: FieldId }`**

A `FieldId` that is not registered in the arena was referenced.

Remediation:
1. Ensure the field is registered in `WorldConfig::fields`.
2. Check that the `FieldId` index matches the field definition order.

**`NotWritable { field: FieldId }`**

Attempted to write a field whose `FieldMutability` does not permit writes in the current context (e.g., writing a `Static` field after initialization).

Remediation:
1. Check the field's `FieldMutability` setting.
2. `Static` fields can only be set during initialization.
3. Use `PerTick` or `PerCommand` mutability for fields that change during simulation.

**`InvalidConfig { reason: String }`**

The arena configuration is invalid.

Remediation:
1. Inspect the reason string for details.
2. Typically indicates misconfigured segment sizes or field layouts.
3. This is wrapped by [`ConfigError::Arena`](#configerror) at startup.

---

## SpaceError

**Crate:** `murk-space` | **File:** `crates/murk-space/src/error.rs`

Errors from space construction or spatial queries.

### Quick reference

| Variant | HLD Code | Description |
|---------|----------|-------------|
| `CoordOutOfBounds { coord, bounds }` | -- | Coordinate outside space bounds |
| `InvalidRegion { reason }` | -- | Region spec invalid for this topology |
| `EmptySpace` | -- | Space constructed with zero cells |
| `DimensionTooLarge { name, value, max }` | -- | Dimension exceeds representable range |
| `InvalidComposition { reason }` | -- | Space composition is invalid |

### Details

**`CoordOutOfBounds { coord: Coord, bounds: String }`**

A coordinate is outside the bounds of the space. The `bounds` string describes the valid coordinate range.

Remediation:
1. Validate coordinates before passing them to space methods.
2. Clamp or reject out-of-bounds coordinates in command processing.

**`InvalidRegion { reason: String }`**

A region specification is invalid for this space topology.

Remediation:
1. Review the `RegionSpec` being compiled.
2. Ensure region parameters (center, radius, etc.) are compatible with the space's dimensionality and bounds.

**`EmptySpace`**

Attempted to construct a space with zero cells. All space types require at least one cell.

Remediation:
1. Provide a positive cell count to the space constructor (e.g., `Line1D::new(n, ...)` with `n >= 1`).

Note: This is also caught at the engine level by [`ConfigError::EmptySpace`](#configerror).

**`DimensionTooLarge { name: &'static str, value: u32, max: u32 }`**

A dimension exceeds the representable coordinate range. The `name` field indicates which dimension (e.g., "len", "rows", "cols").

Remediation:
1. Reduce the dimension to at or below `max`.
2. The limit exists because coordinates are stored as `i32` and the space must be indexable.

**`InvalidComposition { reason: String }`**

A space composition is invalid (e.g., empty component list, cell count overflow in product spaces).

Remediation:
1. Review the composition parameters.
2. For product spaces, ensure components are non-empty and the total cell count fits in `usize`.

---

## ReplayError

**Crate:** `murk-replay` | **File:** `crates/murk-replay/src/error.rs`

Errors during replay recording, playback, or determinism comparison.

### Quick reference

| Variant | HLD Code | Description |
|---------|----------|-------------|
| `Io(io::Error)` | -- | I/O error during read or write |
| `InvalidMagic` | -- | File missing `b"MURK"` magic bytes |
| `UnsupportedVersion { found }` | -- | Format version not supported by this build |
| `MalformedFrame { detail }` | -- | Frame could not be decoded |
| `UnknownPayloadType { tag }` | -- | Command payload type tag not recognized |
| `ConfigMismatch { recorded, current }` | -- | Configuration hash mismatch |
| `SnapshotMismatch { tick_id, recorded, replayed }` | -- | Determinism violation at specified tick |

### Details

**`Io(io::Error)`**

An I/O error occurred during read or write. Wraps a `std::io::Error`.

Remediation:
1. Check file permissions, disk space, and path validity.
2. Inspect the inner `io::Error` for the specific OS-level failure.

**`InvalidMagic`**

The file does not start with the expected `b"MURK"` magic bytes. The file is not a valid Murk replay.

Remediation:
1. Verify the file path points to an actual Murk replay file.
2. The file may be corrupt or a different format.

**`UnsupportedVersion { found: u8 }`**

The format version in the file is not supported by this build. The current build supports version 2.

Remediation:
1. Upgrade or downgrade the Murk library to match the replay file's format version.
2. Or re-record the replay with the current version.

**`MalformedFrame { detail: String }`**

A frame could not be decoded due to truncated or corrupt data. This includes truncated frame headers (partial tick_id), invalid presence flags, truncated payloads, and invalid UTF-8 strings.

Remediation:
1. The replay file is corrupt or was truncated (e.g., process crash during recording).
2. Re-record the replay.
3. If the truncation is at the end, preceding frames may still be valid.

**`UnknownPayloadType { tag: u8 }`**

A command payload type tag is not recognized. The `tag` value does not correspond to any known `CommandPayload` variant.

Remediation:
1. The replay was recorded with a newer version of Murk that has additional command types.
2. Upgrade the Murk library to match.

**`ConfigMismatch { recorded: u64, current: u64 }`**

The replay was recorded with a different configuration hash. The `recorded` hash (from the file header) does not match the `current` hash (computed from the live configuration).

Remediation:
1. Reconstruct the world with the same configuration used during recording: same fields, propagators, dt, seed, space, and ring buffer size.

**`SnapshotMismatch { tick_id: u64, recorded: u64, replayed: u64 }`**

A snapshot hash does not match between the recorded and replayed state at the specified `tick_id`. This indicates a determinism violation.

Remediation:
1. Check `BuildMetadata` for toolchain/target differences between recording and replay.
2. Common causes: floating-point non-determinism across toolchains/platforms, uninitialized memory, non-deterministic iteration order, or external state dependency.

---

## SubmitError

**Crate:** `murk-engine` | **File:** `crates/murk-engine/src/realtime.rs`

Errors from submitting commands to the tick thread in `RealtimeAsyncWorld`.

### Quick reference

| Variant | HLD Code | Description |
|---------|----------|-------------|
| `Shutdown` | -- | Tick thread has shut down |
| `ChannelFull` | -- | Command channel is full (back-pressure) |

### Details

**`Shutdown`**

The tick thread has shut down. The command channel is disconnected.

Remediation:
1. The world has been shut down or dropped. Do not retry.
2. Create a new world or call `reset()` if the world supports it.

**`ChannelFull`**

The command channel is full (back-pressure). The bounded channel (capacity 64) cannot accept more batches until the tick thread drains it.

Remediation:
1. Reduce command submission rate.
2. Wait for the tick thread to process pending batches before submitting more.
3. This indicates the submitter is outpacing the tick rate.

---

## BatchError

**Crate:** `murk-engine` | **File:** `crates/murk-engine/src/batched.rs`

Errors from the batched simulation engine (`BatchedEngine`). Each variant annotates the failure with context about which world failed.

### Quick reference

| Variant | Description |
|---------|-------------|
| `Step { world_index, error }` | A world's `step_sync()` failed |
| `Observe(ObsError)` | Observation extraction failed |
| `Config(ConfigError)` | World creation failed |
| `InvalidIndex { world_index, num_worlds }` | World index out of bounds |
| `NoObsPlan` | Observation method called without an ObsSpec |
| `InvalidArgument { reason }` | Method argument failed validation |

### Details

**`Step { world_index, error }`**

A world's `step_sync()` failed during `step_and_observe()` or `step_all()`. The `world_index` identifies which world and `error` contains the underlying `TickError`.

Remediation:
1. Inspect the wrapped `TickError` (see [`StepError`](#steperror)).
2. Check propagator inputs for the failing world.

**`Observe(ObsError)`**

Observation extraction failed during `observe_all()` or `step_and_observe()`.

Remediation:
1. Check `ObsSpec` / `ObsEntry` configuration.
2. Ensure field names and region specs are valid (see [`ObsError`](#obserror)).

**`Config(ConfigError)`**

World creation failed during `BatchedEngine::new()` or `reset_world()`.

Remediation:
1. Inspect the wrapped [`ConfigError`](#configerror).
2. Check that all configs have matching space topologies and field definitions.

**`InvalidIndex { world_index, num_worlds }`**

The requested world index is out of bounds.

Remediation:
1. Use `world_index < num_worlds`.
2. Call `num_worlds()` to check the batch size.

**`NoObsPlan`**

An observation method was called but no `ObsSpec` was provided at construction.

Remediation:
1. Pass `obs_entries` when creating `BatchedWorld` / `BatchedEngine`.

**`InvalidArgument { reason }`**

A method argument failed validation (e.g., wrong number of command lists, buffer size mismatch).

Remediation:
1. Read the `reason` message for specifics.
2. Common causes: `commands.len() != num_worlds`, output buffer too small.

---

## Panicked

**Layer:** `murk-ffi` / `murk-python` | **Status code:** `-128`

FFI boundary panic status returned when Rust catches a panic inside an exported `extern "C"` function via `ffi_guard!`.

### Quick reference

| Code | Description |
|------|-------------|
| `-128` | Rust panic caught at FFI boundary |

### Details

`Panicked` means an internal Rust panic occurred while executing an API call. The panic is caught and converted into a status code instead of unwinding across the C boundary.

Remediation:
1. Treat this as a bug in murk or a custom propagator.
2. Retrieve panic text via `murk_last_panic_message` (or Python exception text) and include it in bug reports.
3. Recreate the affected world/batch handle if subsequent calls report internal errors.
