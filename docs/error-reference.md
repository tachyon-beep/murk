# Murk Error Code Reference

Complete reference of all error types in the Murk simulation framework, organized by subsystem.

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

---

## StepError

**Crate:** `murk-core` | **File:** `crates/murk-core/src/error.rs`

Errors returned by the tick engine during `step()`. Corresponds to the TickEngine and Pipeline subsystem codes in HLD section 9.7.

| Variant | HLD Code | Cause | Remediation |
|---------|----------|-------|-------------|
| `PropagatorFailed { name: String, reason: PropagatorError }` | `MURK_ERROR_PROPAGATOR_FAILED` | A propagator returned an error during execution. The `name` field identifies the failing propagator and `reason` contains the underlying `PropagatorError`. | Inspect the wrapped `PropagatorError` for details. Check propagator inputs (field values, dt) for validity. The tick engine will roll back the tick. |
| `AllocationFailed` | `MURK_ERROR_ALLOCATION_FAILED` | Arena allocation failed due to out-of-memory during generation staging. | Reduce field count or cell count. Increase arena segment pool capacity. Check for epoch reclamation stalls preventing segment reuse. |
| `TickRollback` | `MURK_ERROR_TICK_ROLLBACK` | The current tick was rolled back due to a propagator failure. All staged writes are discarded and the world state reverts to the previous generation. | Transient: retry on the next tick. Persistent: investigate the failing propagator. Commands submitted during a rolled-back tick are dropped. |
| `TickDisabled` | `MURK_ERROR_TICK_DISABLED` | Ticking has been disabled after consecutive rollbacks (Decision J). The engine enters a fail-stop state to prevent cascading failures. | The simulation must be reset or reconstructed. Investigate the root cause of repeated propagator failures before restarting. |
| `DtOutOfRange` | `MURK_ERROR_DT_OUT_OF_RANGE` | The requested dt exceeds a propagator's `max_dt` constraint (CFL condition or similar stability limit). | Reduce the configured dt to be at or below the tightest `max_dt` across all propagators. Check `PipelineError::DtTooLarge` for which propagator constrains it. |
| `ShuttingDown` | `MURK_ERROR_SHUTTING_DOWN` | The world is in the shutdown state machine (Decision E). No further ticks will be executed. | Expected during graceful shutdown. Do not retry; the world is terminating. |

---

## PropagatorError

**Crate:** `murk-core` | **File:** `crates/murk-core/src/error.rs`

Errors from individual propagator execution. Returned by `Propagator::step()` and wrapped in `StepError::PropagatorFailed` by the tick engine.

| Variant | HLD Code | Cause | Remediation |
|---------|----------|-------|-------------|
| `ExecutionFailed { reason: String }` | `MURK_ERROR_PROPAGATOR_FAILED` | The propagator's step function failed. The `reason` field contains a human-readable description of the failure. | Inspect the reason string. Common causes: invalid field state, numerical instability, domain-specific constraint violations. |
| `NanDetected { field_id: FieldId, cell_index: Option<usize> }` | -- | NaN detected in propagator output during sentinel checking. `field_id` identifies the affected field; `cell_index` pinpoints the first NaN cell if known. | Indicates numerical instability. Reduce dt, add clamping or bounds to the propagator, or check for division-by-zero in the propagator logic. |
| `ConstraintViolation { constraint: String }` | -- | A user-defined constraint was violated during propagator execution. | Review the constraint definition and the field state that triggered it. May indicate an out-of-bounds physical quantity or a domain invariant violation. |

---

## IngressError

**Crate:** `murk-core` | **File:** `crates/murk-core/src/error.rs`

Errors from the ingress (command submission) pipeline. Used in `Receipt::reason_code` to explain why a command was rejected.

| Variant | HLD Code | Cause | Remediation |
|---------|----------|-------|-------------|
| `QueueFull` | `MURK_ERROR_QUEUE_FULL` | The command queue is at capacity. The ingress pipeline cannot buffer any more commands until the tick engine drains the queue. | Reduce command submission rate. Increase `max_ingress_queue` in `WorldConfig`. In RL training, this may indicate the agent is submitting faster than the tick rate. |
| `Stale` | `MURK_ERROR_STALE` | The command's `basis_tick_id` is too old relative to the current tick. The adaptive backoff mechanism rejected it due to excessive skew. | Resubmit the command with a fresh basis tick. If occurring frequently, the agent's observation-to-action latency is too high relative to the tick rate. Backoff parameters in `BackoffConfig` control the tolerance. |
| `TickRollback` | `MURK_ERROR_TICK_ROLLBACK` | The tick was rolled back; commands submitted during that tick were dropped. | Resubmit the command on the next tick. This is a transient condition. |
| `TickDisabled` | `MURK_ERROR_TICK_DISABLED` | Ticking is disabled after consecutive rollbacks. No commands will be accepted until the world is reset. | Reset the simulation. Investigate the root cause of repeated tick rollbacks. |
| `ShuttingDown` | `MURK_ERROR_SHUTTING_DOWN` | The world is shutting down. No further commands are accepted. | Expected during graceful shutdown. Do not retry. |

---

## ObsError

**Crate:** `murk-core` | **File:** `crates/murk-core/src/error.rs`

Errors from the observation (egress) pipeline. Covers ObsPlan compilation, execution, and snapshot access failures.

| Variant | HLD Code | Cause | Remediation |
|---------|----------|-------|-------------|
| `PlanInvalidated { reason: String }` | `MURK_ERROR_PLAN_INVALIDATED` | The ObsPlan's generation does not match the current snapshot. This occurs when the world topology or field layout has changed since the plan was compiled. | Recompile the ObsPlan via `ObsPlan::compile()` against the current snapshot. Plans are invalidated by world resets or structural changes. |
| `TimeoutWaitingForTick` | `MURK_ERROR_TIMEOUT_WAITING_FOR_TICK` | An exact-tick egress request timed out. Only occurs in RealtimeAsync mode when waiting for a specific tick that has not yet been produced. | Increase the timeout budget. Check if the tick thread is stalled or running slower than expected. This error does not occur in Lockstep mode. |
| `NotAvailable` | `MURK_ERROR_NOT_AVAILABLE` | The requested tick has been evicted from the snapshot ring buffer. The ring only retains the most recent `ring_buffer_size` snapshots. | Increase `ring_buffer_size` in `WorldConfig`. Alternatively, consume observations more promptly so they are not evicted before access. |
| `InvalidComposition { reason: String }` | `MURK_ERROR_INVALID_COMPOSITION` | The ObsPlan's `valid_ratio` is below the 0.35 threshold. Too many entries in the observation spec reference invalid or out-of-bounds regions. | Review the `ObsSpec` entries. Ensure field IDs and region specifications are valid for the current world configuration. The 0.35 threshold means at least 35% of entries must be valid. |
| `ExecutionFailed { reason: String }` | `MURK_ERROR_EXECUTION_FAILED` | ObsPlan execution failed mid-fill. An error occurred while extracting field data into the output buffer. | Inspect the reason string. Common causes: snapshot was reclaimed during execution, arena error, or malformed plan. |
| `InvalidObsSpec { reason: String }` | `MURK_ERROR_INVALID_OBSSPEC` | Malformed ObsSpec detected at compilation time. The observation specification contains structural errors. | Review the ObsSpec structure: check field IDs, region definitions, transforms, and dtypes. Fix the spec before recompiling. |
| `WorkerStalled` | `MURK_ERROR_WORKER_STALLED` | An egress worker exceeded the `max_epoch_hold` budget (default 100ms). The epoch reclamation system forcibly unpinned the worker to prevent blocking arena garbage collection. | Reduce observation complexity or increase `max_epoch_hold_ms` in `AsyncConfig`. A stalled worker prevents epoch advancement, which blocks arena segment reclamation. |

---

## ConfigError

**Crate:** `murk-engine` | **File:** `crates/murk-engine/src/config.rs`

Errors detected during `WorldConfig::validate()` at startup time. These are structural invariant violations that prevent world construction.

| Variant | HLD Code | Cause | Remediation |
|---------|----------|-------|-------------|
| `Pipeline(PipelineError)` | -- | Propagator pipeline validation failed. Wraps a `PipelineError` (see below). | Inspect the inner `PipelineError` for the specific pipeline issue. |
| `Arena(ArenaError)` | -- | Arena configuration is invalid. Wraps an `ArenaError` (see below). | Inspect the inner `ArenaError` for the specific arena issue. |
| `EmptySpace` | -- | The configured `Space` has zero cells. A simulation requires at least one spatial cell. | Provide a space with at least one cell. Check the space constructor arguments. |
| `NoFields` | -- | No fields are registered in the configuration. A simulation requires at least one `FieldDef`. | Add at least one field definition to `WorldConfig::fields`. |
| `RingBufferTooSmall { configured: usize }` | -- | The `ring_buffer_size` is below the minimum of 2. The snapshot ring requires at least 2 slots for double-buffering. | Set `ring_buffer_size` to 2 or greater. Default is 8. |
| `IngressQueueZero` | -- | The `max_ingress_queue` capacity is zero. The ingress pipeline requires at least one slot. | Set `max_ingress_queue` to 1 or greater. Default is 1024. |
| `InvalidTickRate { value: f64 }` | -- | `tick_rate_hz` is NaN, infinite, zero, or negative. Must be a finite positive number. | Provide a valid positive finite `tick_rate_hz` value, or set it to `None` for no rate limiting. |

---

## PipelineError

**Crate:** `murk-propagator` | **File:** `crates/murk-propagator/src/pipeline.rs`

Errors from pipeline validation at startup. These are checked once by `validate_pipeline()` and prevent world construction if any are detected.

| Variant | HLD Code | Cause | Remediation |
|---------|----------|-------|-------------|
| `EmptyPipeline` | -- | No propagators are registered in the pipeline. At least one propagator is required. | Add at least one propagator to `WorldConfig::propagators`. |
| `WriteConflict(Vec<WriteConflict>)` | -- | Two or more propagators write the same field. Each `WriteConflict` contains `field_id`, `first_writer`, and `second_writer`. | Ensure each `FieldId` is written by at most one propagator. Restructure propagators so that field ownership is exclusive. |
| `UndefinedField { propagator: String, field_id: FieldId }` | -- | A propagator references (reads, reads_previous, or writes) a field that is not defined in the world's field list. | Register the missing `FieldId` in `WorldConfig::fields`, or update the propagator to reference only defined fields. |
| `DtTooLarge { configured_dt: f64, max_supported: f64, constraining_propagator: String }` | -- | The configured dt exceeds a propagator's `max_dt` constraint. The `constraining_propagator` field identifies which propagator has the tightest limit. | Reduce `WorldConfig::dt` to at or below `max_supported`. The tightest `max_dt` across all propagators determines the upper bound. |
| `InvalidDt { value: f64 }` | -- | The configured dt is not a valid timestep: NaN, infinity, zero, or negative. | Provide a finite positive dt value in `WorldConfig::dt`. |

The `WriteConflict` struct contains:

| Field | Type | Description |
|-------|------|-------------|
| `field_id` | `FieldId` | The contested field |
| `first_writer` | `String` | Name of the first writer (earlier in pipeline order) |
| `second_writer` | `String` | Name of the second writer (later in pipeline order) |

---

## ArenaError

**Crate:** `murk-arena` | **File:** `crates/murk-arena/src/error.rs`

Errors from arena operations. The arena manages generational allocation of field data for the snapshot ring buffer.

| Variant | HLD Code | Cause | Remediation |
|---------|----------|-------|-------------|
| `CapacityExceeded { requested: usize, capacity: usize }` | -- | The segment pool is full and cannot allocate the requested number of bytes. All segments are in use by live generations. | Increase arena capacity. Ensure epoch reclamation is running (workers must unpin epochs so old generations can be freed). Reduce `ring_buffer_size` to decrease the number of live generations. |
| `StaleHandle { handle_generation: u32, oldest_live: u32 }` | -- | A `FieldHandle` from a generation that has already been reclaimed was used. The handle's generation predates the oldest live generation. | This indicates a use-after-free bug in handle management. Ensure handles are not cached across generation boundaries. Check that observation plans are recompiled after resets. |
| `UnknownField { field: FieldId }` | -- | A `FieldId` that is not registered in the arena was referenced. | Ensure the field is registered in `WorldConfig::fields`. Check that the `FieldId` index matches the field definition order. |
| `NotWritable { field: FieldId }` | -- | Attempted to write a field whose `FieldMutability` does not permit writes in the current context (e.g., writing a `Static` field after initialization). | Check the field's `FieldMutability` setting. `Static` fields can only be set during initialization. Use `PerTick` or `PerCommand` mutability for fields that change during simulation. |
| `InvalidConfig { reason: String }` | -- | The arena configuration is invalid. | Inspect the reason string for details. Typically indicates misconfigured segment sizes or field layouts. |

---

## SpaceError

**Crate:** `murk-space` | **File:** `crates/murk-space/src/error.rs`

Errors from space construction or spatial queries.

| Variant | HLD Code | Cause | Remediation |
|---------|----------|-------|-------------|
| `CoordOutOfBounds { coord: Coord, bounds: String }` | -- | A coordinate is outside the bounds of the space. The `bounds` string describes the valid coordinate range. | Validate coordinates before passing them to space methods. Clamp or reject out-of-bounds coordinates in command processing. |
| `InvalidRegion { reason: String }` | -- | A region specification is invalid for this space topology. | Review the `RegionSpec` being compiled. Ensure region parameters (center, radius, etc.) are compatible with the space's dimensionality and bounds. |
| `EmptySpace` | -- | Attempted to construct a space with zero cells. All space types require at least one cell. | Provide a positive cell count to the space constructor (e.g., `Line1D::new(n, ...)` with `n >= 1`). |
| `DimensionTooLarge { name: &'static str, value: u32, max: u32 }` | -- | A dimension exceeds the representable coordinate range. The `name` field indicates which dimension (e.g., "len", "rows", "cols"). | Reduce the dimension to at or below `max`. The limit exists because coordinates are stored as `i32` and the space must be indexable. |
| `InvalidComposition { reason: String }` | -- | A space composition is invalid (e.g., empty component list, cell count overflow in product spaces). | Review the composition parameters. For product spaces, ensure components are non-empty and the total cell count fits in `usize`. |

---

## ReplayError

**Crate:** `murk-replay` | **File:** `crates/murk-replay/src/error.rs`

Errors during replay recording, playback, or determinism comparison.

| Variant | HLD Code | Cause | Remediation |
|---------|----------|-------|-------------|
| `Io(io::Error)` | -- | An I/O error occurred during read or write. Wraps a `std::io::Error`. | Check file permissions, disk space, and path validity. Inspect the inner `io::Error` for the specific OS-level failure. |
| `InvalidMagic` | -- | The file does not start with the expected `b"MURK"` magic bytes. The file is not a valid Murk replay. | Verify the file path points to an actual Murk replay file. The file may be corrupt or a different format. |
| `UnsupportedVersion { found: u8 }` | -- | The format version in the file is not supported by this build. The current build supports version 2. | Upgrade or downgrade the Murk library to match the replay file's format version. Re-record the replay with the current version. |
| `MalformedFrame { detail: String }` | -- | A frame could not be decoded due to truncated or corrupt data. Includes truncated frame headers (partial tick_id), invalid presence flags, truncated payloads, and invalid UTF-8 strings. | The replay file is corrupt or was truncated (e.g., process crash during recording). Re-record the replay. If the truncation is at the end, preceding frames may still be valid. |
| `UnknownPayloadType { tag: u8 }` | -- | A command payload type tag is not recognized. The `tag` value does not correspond to any known `CommandPayload` variant. | The replay was recorded with a newer version of Murk that has additional command types. Upgrade the Murk library. |
| `ConfigMismatch { recorded: u64, current: u64 }` | -- | The replay was recorded with a different configuration hash. The `recorded` hash (from the file header) does not match the `current` hash (computed from the live configuration). | Reconstruct the world with the same configuration used during recording: same fields, propagators, dt, seed, space, and ring buffer size. |
| `SnapshotMismatch { tick_id: u64, recorded: u64, replayed: u64 }` | -- | A snapshot hash does not match between the recorded and replayed state at the specified `tick_id`. This indicates a determinism violation. | The simulation is not deterministic under replay. Common causes: floating-point non-determinism across toolchains/platforms, uninitialized memory, non-deterministic iteration order, or external state dependency. Check the `BuildMetadata` for toolchain/target differences. |

---

## SubmitError

**Crate:** `murk-engine` | **File:** `crates/murk-engine/src/realtime.rs`

Errors from submitting commands to the tick thread in `RealtimeAsyncWorld`.

| Variant | HLD Code | Cause | Remediation |
|---------|----------|-------|-------------|
| `Shutdown` | -- | The tick thread has shut down. The command channel is disconnected. | The world has been shut down or dropped. Do not retry. Create a new world or call `reset()` if the world supports it. |
| `ChannelFull` | -- | The command channel is full (back-pressure). The bounded channel (capacity 64) cannot accept more batches until the tick thread drains it. | Reduce command submission rate. Wait for the tick thread to process pending batches before submitting more. This indicates the submitter is outpacing the tick rate. |

---

## BatchError

**Crate:** `murk-engine` | **File:** `crates/murk-engine/src/batched.rs`

Errors from the batched simulation engine (`BatchedEngine`). Each variant annotates the failure with context about which world failed.

| Variant | Cause | Remediation |
|---------|-------|-------------|
| `Step { world_index, error }` | A world's `step_sync()` failed during `step_and_observe()` or `step_all()`. The `world_index` identifies which world and `error` contains the underlying `TickError`. | Inspect the wrapped `TickError`. Check propagator inputs for the failing world. |
| `Observe(ObsError)` | Observation extraction failed during `observe_all()` or `step_and_observe()`. | Check `ObsSpec` / `ObsEntry` configuration. Ensure field names and region specs are valid. |
| `Config(ConfigError)` | World creation failed during `BatchedEngine::new()` or `reset_world()`. | Inspect the wrapped `ConfigError`. Check that all configs have matching space topologies and field definitions. |
| `InvalidIndex { world_index, num_worlds }` | The requested world index is out of bounds. | Use `world_index < num_worlds`. Call `num_worlds()` to check the batch size. |
| `NoObsPlan` | An observation method was called but no `ObsSpec` was provided at construction. | Pass `obs_entries` when creating `BatchedWorld` / `BatchedEngine`. |
| `InvalidArgument { reason }` | A method argument failed validation (e.g., wrong number of command lists, buffer size mismatch). | Read the `reason` message for specifics. Common causes: `commands.len() != num_worlds`, output buffer too small. |
