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

/// Clone the Arc for a world handle, briefly locking the global table.
fn get_world(handle: u64) -> Option<WorldArc> {
    WORLDS.lock().unwrap().get(handle).cloned()
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
    if world_out.is_null() {
        return MurkStatus::InvalidArgument as i32;
    }

    // Remove config from table (consumes it).
    let builder = match configs().lock().unwrap().remove(config_handle) {
        Some(b) => b,
        None => return MurkStatus::InvalidHandle as i32,
    };

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

    let handle = WORLDS.lock().unwrap().insert(Arc::new(Mutex::new(world)));
    // SAFETY: world_out is valid per caller contract.
    unsafe { *world_out = handle };
    MurkStatus::Ok as i32
}

/// Destroy a lockstep world, releasing all resources.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_lockstep_destroy(world_handle: u64) -> i32 {
    match WORLDS.lock().unwrap().remove(world_handle) {
        Some(_) => MurkStatus::Ok as i32,
        None => MurkStatus::InvalidHandle as i32,
    }
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
    let mut world = world_arc.lock().unwrap();

    match world.step_sync(rust_cmds) {
        Ok(result) => {
            // Write receipts.
            write_receipts(&result.receipts, receipts_out, receipts_cap, n_receipts_out);

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
}

/// Reset the world to tick 0 with a new seed.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_lockstep_reset(world_handle: u64, seed: u64) -> i32 {
    let world_arc = match get_world(world_handle) {
        Some(arc) => arc,
        None => return MurkStatus::InvalidHandle as i32,
    };
    let mut world = world_arc.lock().unwrap();

    match world.reset(seed) {
        Ok(_) => MurkStatus::Ok as i32,
        Err(e) => MurkStatus::from(&e) as i32,
    }
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
    if buf.is_null() {
        return MurkStatus::InvalidArgument as i32;
    }

    let world_arc = match get_world(world_handle) {
        Some(arc) => arc,
        None => return MurkStatus::InvalidHandle as i32,
    };
    let world = world_arc.lock().unwrap();

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
}

/// Current tick ID for a world (0 after construction or reset).
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_current_tick(world_handle: u64) -> u64 {
    match get_world(world_handle) {
        Some(arc) => arc.lock().unwrap().current_tick().0,
        None => 0,
    }
}

/// Whether ticking is disabled due to consecutive rollbacks.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_is_tick_disabled(world_handle: u64) -> u8 {
    match get_world(world_handle) {
        Some(arc) => u8::from(arc.lock().unwrap().is_tick_disabled()),
        None => 0,
    }
}

/// Number of consecutive rollbacks since the last successful tick.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_consecutive_rollbacks(world_handle: u64) -> u32 {
    match get_world(world_handle) {
        Some(arc) => arc.lock().unwrap().consecutive_rollback_count(),
        None => 0,
    }
}

/// The world's current seed.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_seed(world_handle: u64) -> u64 {
    match get_world(world_handle) {
        Some(arc) => arc.lock().unwrap().seed(),
        None => 0,
    }
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
        let mut world = world_arc.lock().unwrap();

        match world.step_sync(rust_cmds) {
            Ok(result) => {
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
            *n_out = receipts.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::MurkCommandType;
    use crate::config::{murk_config_add_field, murk_config_create, murk_config_set_dt, murk_config_set_seed, murk_config_set_space};
    use crate::metrics::murk_step_metrics;
    use crate::propagator::{
        murk_propagator_create, MurkPropagatorDef, MurkStepContext, MurkWriteDecl,
    };
    use crate::types::*;
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

    /// Helper: build a world with a constant propagator writing 7.0 to field 0.
    fn create_test_world() -> u64 {
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
        let prop_name = CString::new("const7").unwrap();
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
            step_fn: Some(const_step_fn),
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
            command_type: MurkCommandType::SetParameter as i32,
            expires_after_tick: 100,
            source_id: 0,
            source_seq: 0,
            priority_class: 1,
            field_id: 0,
            param_key: 0,
            float_value: 0.0,
            double_value: 1.0,
            coord: [0; 4],
            coord_ndim: 0,
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
        // Config was not consumed because we returned early.
        // Clean it up manually.
        crate::config::murk_config_destroy(cfg_h);
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

        // Test murk_step_metrics too.
        let mut metrics2 = MurkStepMetrics::default();
        let status = murk_step_metrics(world_h, &mut metrics2);
        assert_eq!(status, MurkStatus::Ok as i32);
        assert!(metrics2.n_propagators >= 1);

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
}
