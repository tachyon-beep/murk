//! World lifecycle FFI: create, step, reset, destroy, snapshot read, step_vec.
//!
//! Uses per-world `Arc<Mutex<LockstepWorld>>` so the global `WORLDS` table
//! lock is only held briefly (for handle lookup). Different worlds can be
//! stepped concurrently — essential when Python propagators re-acquire the
//! GIL during `step_sync`, preventing GIL/WORLDS deadlocks.

use std::sync::{Arc, Mutex};

use murk_core::id::FieldId;
use murk_core::traits::SnapshotAccess;
use murk_engine::config::{BackoffConfig, WorldConfig};
use murk_engine::LockstepWorld;

use crate::command::{convert_command, convert_receipt, MurkCommand, MurkReceipt};
use crate::config::configs;
use crate::handle::HandleTable;
use crate::metrics::MurkStepMetrics;
use crate::status::MurkStatus;

type WorldArc = Arc<Mutex<LockstepWorld>>;

static WORLDS: Mutex<HandleTable<WorldArc>> = Mutex::new(HandleTable::new());

/// Lightweight world readiness snapshot for non-blocking preflight checks.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MurkWorldPreflight {
    /// Number of command batches currently waiting in ingress.
    pub ingress_queue_depth: u32,
    /// Maximum number of command batches ingress can hold.
    pub ingress_queue_capacity: u32,
    /// Current world tick.
    pub current_tick: u64,
    /// Whether ticking is disabled due to consecutive rollbacks.
    pub tick_disabled: u8,
    /// Number of consecutive rollback ticks.
    pub consecutive_rollbacks: u32,
}

/// Clone the Arc for a world handle, briefly locking the global table.
///
/// Returns `None` if the handle is invalid or the mutex is poisoned.
fn get_world(handle: u64) -> Option<WorldArc> {
    WORLDS.lock().ok()?.get(handle).cloned()
}

pub(crate) fn worlds() -> &'static Mutex<HandleTable<WorldArc>> {
    &WORLDS
}

/// Create a lockstep world from a config handle. Consumes the config.
///
/// On success, writes the world handle to `world_out` and returns `MURK_OK`.
/// On failure, the config is still consumed (destroyed).
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_lockstep_create(config_handle: u64, world_out: *mut u64) -> i32 {
    ffi_guard!({
        // Remove config from table FIRST (consumes it unconditionally).
        // This ensures the "config is always consumed" contract holds even
        // if we return early due to null world_out or validation errors.
        let builder = match ffi_lock!(configs()).remove(config_handle) {
            Some(b) => b,
            None => return MurkStatus::InvalidHandle as i32,
        };

        if world_out.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }

        // Validate: space and fields must be set.
        let space = match builder.space {
            Some(s) => s,
            None => return MurkStatus::ConfigError as i32,
        };
        if builder.fields.is_empty() {
            return MurkStatus::ConfigError as i32;
        }
        if builder.propagators.is_empty() {
            return MurkStatus::ConfigError as i32;
        }

        let config = WorldConfig {
            space,
            fields: builder.fields,
            propagators: builder.propagators,
            dt: builder.dt,
            seed: builder.seed,
            ring_buffer_size: builder.ring_buffer_size,
            max_ingress_queue: builder.max_ingress_queue,
            tick_rate_hz: None,
            backoff: BackoffConfig::default(),
        };

        let world = match LockstepWorld::new(config) {
            Ok(w) => w,
            Err(e) => return MurkStatus::from(&e) as i32,
        };

        let handle = ffi_lock!(WORLDS).insert(Arc::new(Mutex::new(world)));
        // SAFETY: world_out is valid per caller contract.
        unsafe { *world_out = handle };
        MurkStatus::Ok as i32
    })
}

/// Destroy a lockstep world, releasing all resources.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_lockstep_destroy(world_handle: u64) -> i32 {
    ffi_guard!({
        match ffi_lock!(WORLDS).remove(world_handle) {
            Some(_) => MurkStatus::Ok as i32,
            None => MurkStatus::InvalidHandle as i32,
        }
    })
}

/// Execute one tick: submit commands, run pipeline, return receipts + metrics.
///
/// `cmds` is an array of `n_cmds` commands (may be null if `n_cmds == 0`).
/// `receipts_out` is a caller-allocated buffer of at least `receipts_cap` entries.
/// `n_receipts_out` receives the actual number of receipts written.
/// `metrics_out` may be null to skip metrics collection.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_lockstep_step(
    world_handle: u64,
    cmds: *const MurkCommand,
    n_cmds: usize,
    receipts_out: *mut MurkReceipt,
    receipts_cap: usize,
    n_receipts_out: *mut usize,
    metrics_out: *mut MurkStepMetrics,
) -> i32 {
    ffi_guard!({
        // Convert C commands to Rust commands.
        let mut rust_cmds = Vec::with_capacity(n_cmds);
        if n_cmds > 0 {
            if cmds.is_null() {
                return MurkStatus::InvalidArgument as i32;
            }
            // SAFETY: cmds points to n_cmds valid MurkCommand structs.
            let cmd_slice = unsafe { std::slice::from_raw_parts(cmds, n_cmds) };
            for (i, cmd) in cmd_slice.iter().enumerate() {
                match convert_command(cmd, i) {
                    Ok(c) => rust_cmds.push(c),
                    Err(status) => return status as i32,
                }
            }
        }

        let world_arc = match get_world(world_handle) {
            Some(arc) => arc,
            None => return MurkStatus::InvalidHandle as i32,
        };
        // Per-world lock: only this world is locked, not the global table.
        let mut world = ffi_lock!(world_arc);

        match world.step_sync(rust_cmds) {
            Ok(result) => {
                // Write receipts.
                write_receipts(&result.receipts, receipts_out, receipts_cap, n_receipts_out);

                // Snapshot propagator timings into thread-local while the
                // world lock is still held, so murk_step_metrics_propagator
                // returns data from the same tick as the aggregate metrics.
                crate::metrics::snapshot_propagator_timings(&result.metrics.propagator_us);

                // Write metrics.
                if !metrics_out.is_null() {
                    let m = MurkStepMetrics::from_rust(&result.metrics);
                    // SAFETY: metrics_out is valid per caller contract.
                    unsafe { *metrics_out = m };
                }

                MurkStatus::Ok as i32
            }
            Err(tick_error) => {
                // Write receipts even on error (rollback receipts).
                write_receipts(
                    &tick_error.receipts,
                    receipts_out,
                    receipts_cap,
                    n_receipts_out,
                );

                MurkStatus::from(&tick_error) as i32
            }
        }
    })
}

/// Reset the world to tick 0 with a new seed.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_lockstep_reset(world_handle: u64, seed: u64) -> i32 {
    ffi_guard!({
        let world_arc = match get_world(world_handle) {
            Some(arc) => arc,
            None => return MurkStatus::InvalidHandle as i32,
        };
        let mut world = ffi_lock!(world_arc);

        match world.reset(seed) {
            Ok(_) => MurkStatus::Ok as i32,
            Err(e) => MurkStatus::from(&e) as i32,
        }
    })
}

/// Read a field from the current snapshot into a caller-allocated buffer.
///
/// Returns `MURK_OK` on success, `MURK_ERROR_BUFFER_TOO_SMALL` if `buf_len`
/// is less than the field's element count, `MURK_ERROR_INVALID_ARGUMENT` if
/// the field ID is invalid.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_snapshot_read_field(
    world_handle: u64,
    field_id: u32,
    buf: *mut f32,
    buf_len: usize,
) -> i32 {
    ffi_guard!({
        if buf.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }

        let world_arc = match get_world(world_handle) {
            Some(arc) => arc,
            None => return MurkStatus::InvalidHandle as i32,
        };
        let world = ffi_lock!(world_arc);

        let snap = world.snapshot();
        let data = match snap.read_field(FieldId(field_id)) {
            Some(d) => d,
            None => return MurkStatus::InvalidArgument as i32,
        };

        if buf_len < data.len() {
            return MurkStatus::BufferTooSmall as i32;
        }

        // SAFETY: buf points to buf_len valid f32 values.
        unsafe {
            std::ptr::copy_nonoverlapping(data.as_ptr(), buf, data.len());
        }

        MurkStatus::Ok as i32
    })
}

/// Current tick ID for a world (0 after construction or reset).
///
/// **Ambiguity warning:** returns 0 for both "tick 0" and "invalid handle."
/// Prefer [`murk_current_tick_get`] for unambiguous error detection.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_current_tick(world_handle: u64) -> u64 {
    ffi_guard_or!(0, {
        get_world(world_handle)
            .and_then(|arc| arc.lock().ok().map(|w| w.current_tick().0))
            .unwrap_or(0)
    })
}

/// Current tick ID with explicit error reporting.
///
/// Writes the tick to `*out` and returns `MURK_OK`. Returns
/// `InvalidHandle` or `InternalError` without writing to `out`.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_current_tick_get(world_handle: u64, out: *mut u64) -> i32 {
    ffi_guard!({
        if out.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }
        let world_arc = match get_world(world_handle) {
            Some(arc) => arc,
            None => return MurkStatus::InvalidHandle as i32,
        };
        let world = ffi_lock!(world_arc);
        unsafe { *out = world.current_tick().0 };
        MurkStatus::Ok as i32
    })
}

/// Whether ticking is disabled due to consecutive rollbacks.
///
/// **Ambiguity warning:** returns 0 for both "not disabled" and "invalid handle."
/// Prefer [`murk_is_tick_disabled_get`] for unambiguous error detection.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_is_tick_disabled(world_handle: u64) -> u8 {
    ffi_guard_or!(0, {
        get_world(world_handle)
            .and_then(|arc| arc.lock().ok().map(|w| u8::from(w.is_tick_disabled())))
            .unwrap_or(0)
    })
}

/// Tick-disabled state with explicit error reporting.
///
/// Writes 1 (disabled) or 0 (enabled) to `*out` and returns `MURK_OK`.
/// Returns `InvalidHandle` or `InternalError` without writing to `out`.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_is_tick_disabled_get(world_handle: u64, out: *mut u8) -> i32 {
    ffi_guard!({
        if out.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }
        let world_arc = match get_world(world_handle) {
            Some(arc) => arc,
            None => return MurkStatus::InvalidHandle as i32,
        };
        let world = ffi_lock!(world_arc);
        unsafe { *out = u8::from(world.is_tick_disabled()) };
        MurkStatus::Ok as i32
    })
}

/// Number of consecutive rollbacks since the last successful tick.
///
/// **Ambiguity warning:** returns 0 for both "zero rollbacks" and "invalid handle."
/// Prefer [`murk_consecutive_rollbacks_get`] for unambiguous error detection.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_consecutive_rollbacks(world_handle: u64) -> u32 {
    ffi_guard_or!(0, {
        get_world(world_handle)
            .and_then(|arc| arc.lock().ok().map(|w| w.consecutive_rollback_count()))
            .unwrap_or(0)
    })
}

/// Consecutive rollback count with explicit error reporting.
///
/// Writes the count to `*out` and returns `MURK_OK`. Returns
/// `InvalidHandle` or `InternalError` without writing to `out`.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_consecutive_rollbacks_get(world_handle: u64, out: *mut u32) -> i32 {
    ffi_guard!({
        if out.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }
        let world_arc = match get_world(world_handle) {
            Some(arc) => arc,
            None => return MurkStatus::InvalidHandle as i32,
        };
        let world = ffi_lock!(world_arc);
        unsafe { *out = world.consecutive_rollback_count() };
        MurkStatus::Ok as i32
    })
}

/// Non-blocking world preflight with queue-depth/readiness counters.
///
/// Writes a snapshot into `*out` and returns `MURK_OK`. Returns
/// `InvalidHandle` or `InternalError` without writing to `out`.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_world_preflight_get(world_handle: u64, out: *mut MurkWorldPreflight) -> i32 {
    ffi_guard!({
        if out.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }
        let world_arc = match get_world(world_handle) {
            Some(arc) => arc,
            None => return MurkStatus::InvalidHandle as i32,
        };
        let world = ffi_lock!(world_arc);
        let preflight = MurkWorldPreflight {
            ingress_queue_depth: world.ingress_queue_depth().min(u32::MAX as usize) as u32,
            ingress_queue_capacity: world.ingress_queue_capacity().min(u32::MAX as usize) as u32,
            current_tick: world.current_tick().0,
            tick_disabled: u8::from(world.is_tick_disabled()),
            consecutive_rollbacks: world.consecutive_rollback_count(),
        };
        unsafe { *out = preflight };
        MurkStatus::Ok as i32
    })
}

/// The world's current seed.
///
/// **Ambiguity warning:** returns 0 for both "seed 0" and "invalid handle."
/// Prefer [`murk_seed_get`] for unambiguous error detection.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_seed(world_handle: u64) -> u64 {
    ffi_guard_or!(0, {
        get_world(world_handle)
            .and_then(|arc| arc.lock().ok().map(|w| w.seed()))
            .unwrap_or(0)
    })
}

/// Seed with explicit error reporting.
///
/// Writes the seed to `*out` and returns `MURK_OK`. Returns
/// `InvalidHandle` or `InternalError` without writing to `out`.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_seed_get(world_handle: u64, out: *mut u64) -> i32 {
    ffi_guard!({
        if out.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }
        let world_arc = match get_world(world_handle) {
            Some(arc) => arc,
            None => return MurkStatus::InvalidHandle as i32,
        };
        let world = ffi_lock!(world_arc);
        unsafe { *out = world.seed() };
        MurkStatus::Ok as i32
    })
}

/// Step multiple worlds sequentially. v1: no parallelism.
///
/// If any world fails, returns the first error. All preceding worlds'
/// results are valid (receipts and metrics written).
///
/// `receipts_out` is an array of `n_worlds` pointers to per-world receipt buffers
/// (or null to skip receipts). Each buffer must have capacity for the world's receipts.
/// `metrics_out` is an array of `n_worlds` MurkStepMetrics (or null to skip).
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_lockstep_step_vec(
    world_handles: *const u64,
    cmds_per_world: *const *const MurkCommand,
    n_cmds_per_world: *const usize,
    n_worlds: usize,
    metrics_out: *mut MurkStepMetrics,
) -> i32 {
    ffi_guard!({
        if n_worlds == 0 {
            return MurkStatus::Ok as i32;
        }
        if world_handles.is_null() || cmds_per_world.is_null() || n_cmds_per_world.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }

        // SAFETY: caller guarantees these arrays have n_worlds elements.
        let handles = unsafe { std::slice::from_raw_parts(world_handles, n_worlds) };
        let cmds_ptrs = unsafe { std::slice::from_raw_parts(cmds_per_world, n_worlds) };
        let n_cmds = unsafe { std::slice::from_raw_parts(n_cmds_per_world, n_worlds) };

        for i in 0..n_worlds {
            // Convert commands for this world.
            let mut rust_cmds = Vec::with_capacity(n_cmds[i]);
            if n_cmds[i] > 0 {
                if cmds_ptrs[i].is_null() {
                    return MurkStatus::InvalidArgument as i32;
                }
                let cmd_slice = unsafe { std::slice::from_raw_parts(cmds_ptrs[i], n_cmds[i]) };
                for (j, cmd) in cmd_slice.iter().enumerate() {
                    match convert_command(cmd, j) {
                        Ok(c) => rust_cmds.push(c),
                        Err(status) => return status as i32,
                    }
                }
            }

            let world_arc = match get_world(handles[i]) {
                Some(arc) => arc,
                None => return MurkStatus::InvalidHandle as i32,
            };
            let mut world = ffi_lock!(world_arc);

            match world.step_sync(rust_cmds) {
                Ok(result) => {
                    crate::metrics::snapshot_propagator_timings(&result.metrics.propagator_us);
                    if !metrics_out.is_null() {
                        let m = MurkStepMetrics::from_rust(&result.metrics);
                        unsafe { *metrics_out.add(i) = m };
                    }
                }
                Err(tick_error) => {
                    return MurkStatus::from(&tick_error) as i32;
                }
            }
        }

        MurkStatus::Ok as i32
    })
}

// ── helpers ──────────────────────────────────────────────

#[allow(unsafe_code)]
fn write_receipts(
    receipts: &[murk_core::command::Receipt],
    out: *mut MurkReceipt,
    cap: usize,
    n_out: *mut usize,
) {
    let write_count = receipts.len().min(cap);
    if !out.is_null() && write_count > 0 {
        for (i, receipt) in receipts.iter().enumerate().take(write_count) {
            // SAFETY: out points to cap valid MurkReceipt structs.
            unsafe {
                *out.add(i) = convert_receipt(receipt);
            }
        }
    }
    if !n_out.is_null() {
        // SAFETY: n_out is valid.
        unsafe {
            *n_out = write_count;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::MurkCommandType;
    use crate::config::{
        murk_config_add_field, murk_config_create, murk_config_set_dt, murk_config_set_seed,
        murk_config_set_space,
    };
    use crate::metrics::murk_step_metrics;
    use crate::obs::{
        murk_obsplan_compile, murk_obsplan_destroy, murk_obsplan_execute_agents, MurkObsEntry,
    };
    use crate::propagator::{
        murk_propagator_create, MurkPropagatorDef, MurkStepContext, MurkWriteDecl,
    };
    use crate::types::*;
    use murk_core::{FieldId, FieldSet, PropagatorError};
    use murk_propagator::propagator::WriteMode;
    use murk_propagator::StepContext;
    use std::ffi::{c_void, CString};

    // Test step function: writes constant 7.0 to field 0.
    #[allow(unsafe_code)]
    unsafe extern "C" fn const_step_fn(
        _user_data: *mut c_void,
        ctx: *const MurkStepContext,
    ) -> i32 {
        let ctx = &*ctx;
        let mut ptr: *mut f32 = std::ptr::null_mut();
        let mut len: usize = 0;
        let rc = (ctx.write_fn)(ctx.opaque, 0, &mut ptr, &mut len);
        if rc != 0 {
            return rc;
        }
        let slice = std::slice::from_raw_parts_mut(ptr, len);
        for v in slice {
            *v = 7.0;
        }
        0
    }

    struct PanickingRustPropagator;

    impl murk_propagator::Propagator for PanickingRustPropagator {
        fn name(&self) -> &str {
            "panic_prop"
        }

        fn reads(&self) -> FieldSet {
            FieldSet::empty()
        }

        fn reads_previous(&self) -> FieldSet {
            FieldSet::empty()
        }

        fn writes(&self) -> Vec<(FieldId, WriteMode)> {
            vec![(FieldId(0), WriteMode::Full)]
        }

        fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
            panic!("panicking propagator test");
        }
    }

    /// Helper: build a world with a callback propagator.
    fn create_test_world_with_step(
        propagator_name: &str,
        step_fn: unsafe extern "C" fn(*mut c_void, *const MurkStepContext) -> i32,
    ) -> u64 {
        let mut cfg_h: u64 = 0;
        murk_config_create(&mut cfg_h);

        let params = [10.0f64, 0.0]; // Line1D, len=10, Absorb
        murk_config_set_space(cfg_h, MurkSpaceType::Line1D as i32, params.as_ptr(), 2);

        let name = CString::new("energy").unwrap();
        murk_config_add_field(
            cfg_h,
            name.as_ptr(),
            MurkFieldType::Scalar as i32,
            MurkFieldMutability::PerTick as i32,
            0,
            MurkBoundaryBehavior::Clamp as i32,
        );

        murk_config_set_dt(cfg_h, 0.1);
        murk_config_set_seed(cfg_h, 42);

        // Create propagator.
        let prop_name = CString::new(propagator_name).unwrap();
        let writes = [MurkWriteDecl {
            field_id: 0,
            mode: MurkWriteMode::Full as i32,
        }];
        let def = MurkPropagatorDef {
            name: prop_name.as_ptr(),
            reads: std::ptr::null(),
            n_reads: 0,
            reads_previous: std::ptr::null(),
            n_reads_previous: 0,
            writes: writes.as_ptr(),
            n_writes: 1,
            step_fn: Some(step_fn),
            user_data: std::ptr::null_mut(),
            scratch_bytes: 0,
        };
        let mut prop_h: u64 = 0;
        murk_propagator_create(&def, &mut prop_h);
        crate::config::murk_config_add_propagator(cfg_h, prop_h);

        let mut world_h: u64 = 0;
        let status = murk_lockstep_create(cfg_h, &mut world_h);
        assert_eq!(status, MurkStatus::Ok as i32, "world creation failed");
        world_h
    }

    /// Helper: build a world with a constant propagator writing 7.0 to field 0.
    fn create_test_world() -> u64 {
        create_test_world_with_step("const7", const_step_fn)
    }

    fn create_test_world_with_panicking_rust_propagator() -> u64 {
        let mut cfg_h: u64 = 0;
        murk_config_create(&mut cfg_h);

        let params = [10.0f64, 0.0]; // Line1D, len=10, Absorb
        murk_config_set_space(cfg_h, MurkSpaceType::Line1D as i32, params.as_ptr(), 2);

        let name = CString::new("energy").unwrap();
        murk_config_add_field(
            cfg_h,
            name.as_ptr(),
            MurkFieldType::Scalar as i32,
            MurkFieldMutability::PerTick as i32,
            0,
            MurkBoundaryBehavior::Clamp as i32,
        );

        murk_config_set_dt(cfg_h, 0.1);
        murk_config_set_seed(cfg_h, 42);

        let mut table = crate::config::configs().lock().unwrap();
        let cfg = table
            .get_mut(cfg_h)
            .expect("config handle must be present before world creation");
        cfg.propagators.push(Box::new(PanickingRustPropagator));
        drop(table);

        let mut world_h: u64 = 0;
        let status = murk_lockstep_create(cfg_h, &mut world_h);
        assert_eq!(status, MurkStatus::Ok as i32, "world creation failed");
        world_h
    }

    #[test]
    fn create_step_destroy_lifecycle() {
        let world_h = create_test_world();

        let mut metrics = MurkStepMetrics::default();
        let mut n_receipts: usize = 0;
        let status = murk_lockstep_step(
            world_h,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut n_receipts,
            &mut metrics,
        );
        assert_eq!(status, MurkStatus::Ok as i32);
        assert_eq!(murk_current_tick(world_h), 1);

        assert_eq!(murk_lockstep_destroy(world_h), MurkStatus::Ok as i32);
    }

    #[test]
    fn create_step_read_field_values_correct() {
        let world_h = create_test_world();

        murk_lockstep_step(
            world_h,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );

        let mut buf = [0.0f32; 10];
        let status = murk_snapshot_read_field(world_h, 0, buf.as_mut_ptr(), 10);
        assert_eq!(status, MurkStatus::Ok as i32);
        assert!(buf.iter().all(|&v| v == 7.0));

        murk_lockstep_destroy(world_h);
    }

    #[test]
    fn create_step_receipts_populated() {
        let world_h = create_test_world();

        let cmd = MurkCommand {
            command_type: MurkCommandType::SetField as i32,
            expires_after_tick: 100,
            source_id: 0,
            source_seq: 0,
            priority_class: 1,
            field_id: 0,
            param_key: 0,
            float_value: 1.0,
            double_value: 0.0,
            coord: [0; 4],
            coord_ndim: 1,
        };

        let mut receipts = [MurkReceipt {
            accepted: 0,
            applied_tick_id: 0,
            reason_code: 0,
            command_index: 0,
        }; 4];
        let mut n_receipts: usize = 0;

        murk_lockstep_step(
            world_h,
            &cmd,
            1,
            receipts.as_mut_ptr(),
            4,
            &mut n_receipts,
            std::ptr::null_mut(),
        );
        assert!(n_receipts >= 1);
        assert_eq!(receipts[0].accepted, 1);

        murk_lockstep_destroy(world_h);
    }

    #[test]
    fn create_reset_tick_is_zero() {
        let world_h = create_test_world();

        murk_lockstep_step(
            world_h,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        assert_eq!(murk_current_tick(world_h), 1);

        let status = murk_lockstep_reset(world_h, 99);
        assert_eq!(status, MurkStatus::Ok as i32);
        assert_eq!(murk_current_tick(world_h), 0);
        assert_eq!(murk_seed(world_h), 99);

        murk_lockstep_destroy(world_h);
    }

    #[test]
    fn destroy_then_step_returns_invalid_handle() {
        let world_h = create_test_world();
        murk_lockstep_destroy(world_h);
        let status = murk_lockstep_step(
            world_h,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        assert_eq!(status, MurkStatus::InvalidHandle as i32);
    }

    #[test]
    fn double_destroy_returns_invalid_handle() {
        let world_h = create_test_world();
        assert_eq!(murk_lockstep_destroy(world_h), MurkStatus::Ok as i32);
        assert_eq!(
            murk_lockstep_destroy(world_h),
            MurkStatus::InvalidHandle as i32
        );
    }

    #[test]
    fn null_world_out_returns_invalid_argument() {
        let mut cfg_h: u64 = 0;
        murk_config_create(&mut cfg_h);
        assert_eq!(
            murk_lockstep_create(cfg_h, std::ptr::null_mut()),
            MurkStatus::InvalidArgument as i32
        );
        // Config IS consumed even on null world_out (documented contract).
        // Double-destroy should return InvalidHandle.
        assert_eq!(
            crate::config::murk_config_destroy(cfg_h),
            MurkStatus::InvalidHandle as i32
        );
    }

    #[test]
    fn step_with_no_commands_succeeds() {
        let world_h = create_test_world();
        let mut metrics = MurkStepMetrics::default();
        let status = murk_lockstep_step(
            world_h,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut metrics,
        );
        assert_eq!(status, MurkStatus::Ok as i32);
        assert!(metrics.n_propagators >= 1);
        murk_lockstep_destroy(world_h);
    }

    #[test]
    fn accessors_work() {
        let world_h = create_test_world();
        assert_eq!(murk_current_tick(world_h), 0);
        assert_eq!(murk_is_tick_disabled(world_h), 0);
        assert_eq!(murk_seed(world_h), 42);
        assert_eq!(murk_consecutive_rollbacks(world_h), 0);
        murk_lockstep_destroy(world_h);
    }

    #[test]
    fn metrics_populated_after_step() {
        let world_h = create_test_world();
        let mut metrics = MurkStepMetrics::default();
        murk_lockstep_step(
            world_h,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            &mut metrics,
        );
        assert!(metrics.n_propagators >= 1);
        assert!(metrics.memory_bytes > 0);
        assert_eq!(metrics.worker_stall_events, 0);
        assert_eq!(metrics.ring_not_available_events, 0);

        // Test murk_step_metrics too.
        let mut metrics2 = MurkStepMetrics::default();
        let status = murk_step_metrics(world_h, &mut metrics2);
        assert_eq!(status, MurkStatus::Ok as i32);
        assert!(metrics2.n_propagators >= 1);
        assert_eq!(metrics2.worker_stall_events, 0);
        assert_eq!(metrics2.ring_not_available_events, 0);

        murk_lockstep_destroy(world_h);
    }

    #[test]
    fn read_field_buffer_too_small() {
        let world_h = create_test_world();
        murk_lockstep_step(
            world_h,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );

        let mut buf = [0.0f32; 5]; // too small for 10 cells
        let status = murk_snapshot_read_field(world_h, 0, buf.as_mut_ptr(), 5);
        assert_eq!(status, MurkStatus::BufferTooSmall as i32);

        murk_lockstep_destroy(world_h);
    }

    #[test]
    fn step_vec_multiple_worlds() {
        let w1 = create_test_world();
        let w2 = create_test_world();

        let handles = [w1, w2];
        let cmds: [*const MurkCommand; 2] = [std::ptr::null(), std::ptr::null()];
        let n_cmds = [0usize, 0];
        let mut metrics = [MurkStepMetrics::default(), MurkStepMetrics::default()];

        let status = murk_lockstep_step_vec(
            handles.as_ptr(),
            cmds.as_ptr(),
            n_cmds.as_ptr(),
            2,
            metrics.as_mut_ptr(),
        );
        assert_eq!(status, MurkStatus::Ok as i32);
        assert_eq!(murk_current_tick(w1), 1);
        assert_eq!(murk_current_tick(w2), 1);

        murk_lockstep_destroy(w1);
        murk_lockstep_destroy(w2);
    }

    #[test]
    fn step_vec_zero_worlds() {
        let status = murk_lockstep_step_vec(
            std::ptr::null(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
        );
        assert_eq!(status, MurkStatus::Ok as i32);
    }

    #[test]
    fn step_vec_invalid_handle() {
        let handles = [0xDEADBEEFu64];
        let cmds: [*const MurkCommand; 1] = [std::ptr::null()];
        let n_cmds = [0usize];
        let status = murk_lockstep_step_vec(
            handles.as_ptr(),
            cmds.as_ptr(),
            n_cmds.as_ptr(),
            1,
            std::ptr::null_mut(),
        );
        assert_eq!(status, MurkStatus::InvalidHandle as i32);
    }

    #[test]
    fn write_receipts_reports_written_count_not_total() {
        use murk_core::command::Receipt;
        use murk_core::id::TickId;

        let receipts = vec![
            Receipt {
                accepted: true,
                applied_tick_id: Some(TickId(1)),
                reason_code: None,
                command_index: 0,
            },
            Receipt {
                accepted: true,
                applied_tick_id: Some(TickId(1)),
                reason_code: None,
                command_index: 1,
            },
            Receipt {
                accepted: true,
                applied_tick_id: Some(TickId(1)),
                reason_code: None,
                command_index: 2,
            },
        ];

        // Buffer smaller than receipt count: cap=1 but 3 receipts.
        let mut buf = [MurkReceipt {
            accepted: 0,
            applied_tick_id: 0,
            reason_code: 0,
            command_index: 0,
        }; 1];
        let mut n_out: usize = 0;

        write_receipts(&receipts, buf.as_mut_ptr(), 1, &mut n_out);

        // n_out must report the number WRITTEN (1), not the total (3).
        assert_eq!(
            n_out, 1,
            "write_receipts should report written count (1), not total (3)"
        );
        // The one written receipt should be the first one.
        assert_eq!(buf[0].command_index, 0);
    }

    #[test]
    fn obsplan_execute_agents_buffer_too_small_does_not_poison_world() {
        let world_h = create_test_world();
        // Populate one tick so observation paths have data.
        assert_eq!(
            murk_lockstep_step(
                world_h,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ),
            MurkStatus::Ok as i32
        );

        let entry = MurkObsEntry {
            field_id: 0,
            region_type: 0,
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
        let mut plan_h = 0u64;
        assert_eq!(
            murk_obsplan_compile(world_h, &entry, 1, &mut plan_h),
            MurkStatus::Ok as i32
        );

        let centers = [0i32, 1i32];
        let mut output = vec![0.0f32; 19];
        let mut mask = vec![0u8; 20];
        let status = murk_obsplan_execute_agents(
            world_h,
            plan_h,
            centers.as_ptr(),
            1,
            2,
            output.as_mut_ptr(),
            output.len(),
            mask.as_mut_ptr(),
            mask.len(),
            std::ptr::null_mut(),
        );
        assert_eq!(status, MurkStatus::BufferTooSmall as i32);

        // World remains usable after the rejected FFI call.
        assert_eq!(
            murk_lockstep_step(
                world_h,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ),
            MurkStatus::Ok as i32
        );

        assert_eq!(murk_obsplan_destroy(plan_h), MurkStatus::Ok as i32);
        assert_eq!(murk_lockstep_destroy(world_h), MurkStatus::Ok as i32);
    }

    #[test]
    fn accessor_get_variants_return_values() {
        let world_h = create_test_world();

        let mut tick: u64 = 999;
        assert_eq!(
            murk_current_tick_get(world_h, &mut tick),
            MurkStatus::Ok as i32
        );
        assert_eq!(tick, 0);

        let mut disabled: u8 = 99;
        assert_eq!(
            murk_is_tick_disabled_get(world_h, &mut disabled),
            MurkStatus::Ok as i32
        );
        assert_eq!(disabled, 0);

        let mut rollbacks: u32 = 99;
        assert_eq!(
            murk_consecutive_rollbacks_get(world_h, &mut rollbacks),
            MurkStatus::Ok as i32
        );
        assert_eq!(rollbacks, 0);

        let mut seed: u64 = 0;
        assert_eq!(murk_seed_get(world_h, &mut seed), MurkStatus::Ok as i32);
        assert_eq!(seed, 42);

        let mut preflight = MurkWorldPreflight::default();
        assert_eq!(
            murk_world_preflight_get(world_h, &mut preflight),
            MurkStatus::Ok as i32
        );
        assert_eq!(preflight.current_tick, 0);
        assert_eq!(preflight.tick_disabled, 0);
        assert_eq!(preflight.consecutive_rollbacks, 0);
        assert!(preflight.ingress_queue_capacity > 0);

        murk_lockstep_destroy(world_h);
    }

    #[test]
    fn accessor_get_variants_detect_invalid_handle() {
        let world_h = create_test_world();
        murk_lockstep_destroy(world_h);

        // All _get variants must return InvalidHandle for a destroyed world.
        let mut tick: u64 = 999;
        assert_eq!(
            murk_current_tick_get(world_h, &mut tick),
            MurkStatus::InvalidHandle as i32
        );
        assert_eq!(tick, 999, "out must not be written on error");

        let mut disabled: u8 = 99;
        assert_eq!(
            murk_is_tick_disabled_get(world_h, &mut disabled),
            MurkStatus::InvalidHandle as i32
        );
        assert_eq!(disabled, 99, "out must not be written on error");

        let mut rollbacks: u32 = 99;
        assert_eq!(
            murk_consecutive_rollbacks_get(world_h, &mut rollbacks),
            MurkStatus::InvalidHandle as i32
        );
        assert_eq!(rollbacks, 99, "out must not be written on error");

        let mut seed: u64 = 999;
        assert_eq!(
            murk_seed_get(world_h, &mut seed),
            MurkStatus::InvalidHandle as i32
        );
        assert_eq!(seed, 999, "out must not be written on error");
    }

    #[test]
    fn ffi_guard_catches_panic_and_stores_message() {
        // Clear any previous panic message on this thread.
        crate::LAST_PANIC.with(|cell| cell.borrow_mut().clear());

        // Simulate a panic inside an FFI-guarded function body.
        // This is the same mechanism that protects every extern "C" entry point
        // (murk_lockstep_step, murk_lockstep_create, etc.) — ffi_guard! wraps
        // their entire body in catch_unwind.
        let status = ffi_guard!({
            panic!("deliberate test panic from world integration test");
        });

        // ffi_guard! must convert the panic into MurkStatus::Panicked (-128).
        assert_eq!(
            status,
            MurkStatus::Panicked as i32,
            "ffi_guard! must return Panicked (-128) when the body panics"
        );

        // The panic message must be retrievable via murk_last_panic_message.
        // First, query the length by passing null buf.
        let len = crate::murk_last_panic_message(std::ptr::null_mut(), 0);
        assert!(
            len > 0,
            "murk_last_panic_message must return non-zero length after a panic"
        );

        // Now read the full message into a buffer.
        let mut buf = vec![0u8; (len as usize) + 1];
        let len2 =
            crate::murk_last_panic_message(buf.as_mut_ptr() as *mut std::ffi::c_char, buf.len());
        assert_eq!(len, len2, "length must be consistent between calls");

        let msg = std::str::from_utf8(&buf[..len2 as usize]).unwrap();
        assert!(
            msg.contains("deliberate test panic from world integration test"),
            "panic message must contain the original panic string, got: {msg:?}"
        );
    }

    #[test]
    fn ffi_guard_end_to_end_with_world_lifecycle() {
        // This test proves that ffi_guard! does not interfere with normal
        // world operations: create a world, trigger a guarded panic, verify
        // the panic is caught, then step the world normally to show it is
        // still usable (the panic was contained, not propagated).
        let world_h = create_test_world();

        // Clear previous panic state.
        crate::LAST_PANIC.with(|cell| cell.borrow_mut().clear());

        // A guarded panic in an unrelated call does not corrupt the world.
        let panic_status = ffi_guard!({
            panic!("boom during world test");
        });
        assert_eq!(panic_status, MurkStatus::Panicked as i32);

        // Verify the message is stored.
        let mut buf = [0u8; 128];
        let len =
            crate::murk_last_panic_message(buf.as_mut_ptr() as *mut std::ffi::c_char, buf.len());
        assert!(len > 0);
        let msg = std::str::from_utf8(&buf[..len as usize]).unwrap();
        assert!(msg.contains("boom during world test"));

        // The world must still be fully operational after the caught panic.
        let mut n_receipts: usize = 0;
        let step_status = murk_lockstep_step(
            world_h,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut n_receipts,
            std::ptr::null_mut(),
        );
        assert_eq!(
            step_status,
            MurkStatus::Ok as i32,
            "world must remain usable after a caught panic in an unrelated ffi_guard! call"
        );
        assert_eq!(murk_current_tick(world_h), 1);

        murk_lockstep_destroy(world_h);
    }

    #[test]
    fn panicking_propagator_returns_panicked_and_poisoned_world_is_reported() {
        crate::LAST_PANIC.with(|cell| cell.borrow_mut().clear());
        let world_h = create_test_world_with_panicking_rust_propagator();

        let mut n_receipts: usize = 0;
        let status = murk_lockstep_step(
            world_h,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            &mut n_receipts,
            std::ptr::null_mut(),
        );
        assert_eq!(status, MurkStatus::Panicked as i32);

        let len = crate::murk_last_panic_message(std::ptr::null_mut(), 0);
        assert!(len > 0, "panic message should be captured");
        let mut buf = vec![0u8; (len as usize) + 1];
        let len2 =
            crate::murk_last_panic_message(buf.as_mut_ptr() as *mut std::ffi::c_char, buf.len());
        assert_eq!(len, len2);
        let msg = std::str::from_utf8(&buf[..len as usize]).unwrap();
        assert!(
            msg.contains("panicking propagator test"),
            "panic message should include callback panic text, got: {msg:?}"
        );

        // The handle still exists, but the world mutex is poisoned.
        let mut tick: u64 = 123;
        assert_eq!(
            murk_current_tick_get(world_h, &mut tick),
            MurkStatus::InternalError as i32
        );
        assert_eq!(tick, 123, "out param must not be written on error");
        assert_eq!(
            murk_lockstep_step(
                world_h,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            ),
            MurkStatus::InternalError as i32
        );

        // Destroy still succeeds because removal does not require locking the world mutex.
        assert_eq!(murk_lockstep_destroy(world_h), MurkStatus::Ok as i32);
    }

    #[test]
    fn accessor_get_variants_null_out_returns_invalid_argument() {
        let world_h = create_test_world();

        assert_eq!(
            murk_current_tick_get(world_h, std::ptr::null_mut()),
            MurkStatus::InvalidArgument as i32
        );
        assert_eq!(
            murk_is_tick_disabled_get(world_h, std::ptr::null_mut()),
            MurkStatus::InvalidArgument as i32
        );
        assert_eq!(
            murk_consecutive_rollbacks_get(world_h, std::ptr::null_mut()),
            MurkStatus::InvalidArgument as i32
        );
        assert_eq!(
            murk_seed_get(world_h, std::ptr::null_mut()),
            MurkStatus::InvalidArgument as i32
        );
        assert_eq!(
            murk_world_preflight_get(world_h, std::ptr::null_mut()),
            MurkStatus::InvalidArgument as i32
        );

        murk_lockstep_destroy(world_h);
    }
}
