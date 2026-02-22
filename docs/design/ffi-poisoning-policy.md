# FFI Mutex Poisoning Policy

## Status

Accepted (v0.1.x hardening)

## Context

`murk-ffi` catches panics at the FFI boundary (`ffi_guard!`) and returns `MurkStatus::Panicked`.
If a panic occurs while holding a Rust mutex, that mutex becomes poisoned. Subsequent lock attempts
currently return `MurkStatus::InternalError` via `ffi_lock!`.

Without an explicit policy, callers cannot tell whether they should retry, rebuild a world, or terminate.

## Decision

Use a **world-fatal** policy:

- A panic while operating on a specific world/plan/config handle is treated as fatal for that handle.
- Other independent handles may continue to work.
- The API returns `MurkStatus::InternalError` for poisoned-handle follow-up calls.
- Callers are expected to destroy and recreate the affected handle.

## Rationale

- Safer than attempting in-place recovery from potentially inconsistent state.
- Less disruptive than process-fatal behavior for multi-world consumers.
- Aligns with existing behavior in `ffi_lock!` and current tests.

## Operational Guidance

- On `MurkStatus::Panicked`, immediately read `murk_last_panic_message`.
- Mark the associated handle unhealthy.
- Destroy and recreate that handle before resuming normal operation.
- If `InternalError` spreads beyond one handle, escalate to process restart.

## Implementation Notes

- Current code path: `ffi_lock!` in `crates/murk-ffi/src/lib.rs` maps poisoned mutexes to `InternalError`.
- Existing regression coverage:
  - `world::tests::panicking_propagator_returns_panicked_and_poisoned_world_is_reported`
  - `world::tests::ffi_guard_end_to_end_with_world_lifecycle`

## Consequences

- Predictable failure semantics for FFI consumers.
- Recovery cost is handle recreation, not transparent retry.
- Future enhancement option: explicit health query APIs per handle.
