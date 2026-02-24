# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [ ] High | [x] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [x] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`murk_obsplan_compile` returns `InvalidArgument` for malformed `MurkObsEntry` specs instead of `InvalidObsSpec`.

## Steps to Reproduce

1. Create a valid world handle.
2. Call `murk_obsplan_compile` with one entry containing an invalid spec field (example: `region_type = 99`), `n_entries = 1`, and valid `plan_out`.
3. Observe returned status code.

## Expected Behavior

Malformed observation specs at compile time should return `MurkStatus::InvalidObsSpec` (`-12`).

## Actual Behavior

The function returns `MurkStatus::InvalidArgument` (`-18`) for entry-conversion failures.

## Reproduction Rate

Always

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** Any

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```text
N/A - found via static analysis
```

## Minimal Reproducer

```c
// Assume world_handle is a valid world.
MurkObsEntry e = {0};
e.field_id = 0;
e.region_type = 99;   // invalid
e.transform_type = 0;
e.dtype = 0;

uint64_t plan = 0;
int32_t s = murk_obsplan_compile(world_handle, &e, 1, &plan);
// Actual: -18 (InvalidArgument)
// Expected: -12 (InvalidObsSpec)
```

## Additional Context

Root cause is in `crates/murk-ffi/src/obs.rs:202` and `crates/murk-ffi/src/obs.rs:204`, where `convert_obs_entry(...) == None` maps to `InvalidArgument`.  
The same file’s conversion logic is validating ObsSpec structure (`crates/murk-ffi/src/obs.rs:49`, `crates/murk-ffi/src/obs.rs:77`, `crates/murk-ffi/src/obs.rs:91`), which semantically matches `InvalidObsSpec` (`crates/murk-ffi/src/status.rs:41`).  
Suggested fix: return `InvalidObsSpec` for conversion failures in `murk_obsplan_compile`, keeping `InvalidArgument` for pointer/null/range API argument errors.

---

# Bug Report

**Date:** 2026-02-23  
**Reporter:** static-analysis-agent  
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [x] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

`murk_obsplan_execute_agents` is documented as generic plan execution but fails for fixed-only plans (Simple strategy) with `ExecutionFailed`.

## Steps to Reproduce

1. Create a world and compile an obs plan with fixed region(s) only (for example `region_type = 0` / `All`).
2. Allocate correctly sized output and mask for `n_agents * plan_len`.
3. Call `murk_obsplan_execute_agents`.

## Expected Behavior

`murk_obsplan_execute_agents` should execute the compiled plan for each agent as documented, or at minimum reject unsupported plan types with a clear argument/precondition error at the FFI boundary.

## Actual Behavior

Call returns `MurkStatus::ExecutionFailed` (`-11`) because Simple plans are not supported by underlying `execute_agents`, and this precondition is not enforced/documented at the FFI function boundary.

## Reproduction Rate

Always (for fixed-only plans)

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD
- **Python version (if murk-python):** N/A
- **C compiler (if murk-ffi C header/source):** Any

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```text
N/A - found via static analysis
```

## Minimal Reproducer

```rust
// Pseudocode using FFI calls; assume world_h is valid and stepped once.
let entry = MurkObsEntry {
    field_id: 0,
    region_type: 0, // fixed/all -> Simple plan
    transform_type: 0,
    normalize_min: 0.0,
    normalize_max: 0.0,
    dtype: 0,
    region_params: [0; 8],
    n_region_params: 0,
    pool_kernel: 0,
    pool_kernel_size: 0,
    pool_stride: 0,
};
let mut plan_h = 0;
assert_eq!(murk_obsplan_compile(world_h, &entry, 1, &mut plan_h), 0);

let centers = [0i32, 1i32];
let mut out = vec![0.0f32; 2 * murk_obsplan_output_len(plan_h) as usize];
let mut mask = vec![0u8; 2 * murk_obsplan_mask_len(plan_h) as usize];

let s = murk_obsplan_execute_agents(
    world_h,
    plan_h,
    centers.as_ptr(),
    1,
    2,
    out.as_mut_ptr(),
    out.len(),
    mask.as_mut_ptr(),
    mask.len(),
    std::ptr::null_mut(),
);
// Actual: -11 (ExecutionFailed)
```

## Additional Context

FFI docs present this as generic plan execution (`crates/murk-ffi/src/obs.rs:309`, `crates/murk-ffi/src/obs.rs:312`) and unconditionally forward to cache `execute_agents` (`crates/murk-ffi/src/obs.rs:398`).  
Underlying implementation rejects Simple plans (`crates/murk-obs/src/plan.rs:770`, `crates/murk-obs/src/plan.rs:773`).  
Suggested fix options:
1. Add explicit FFI precheck and return `InvalidArgument` with clear precondition, or
2. Support broadcasting fixed-entry plans across agents in the FFI path.