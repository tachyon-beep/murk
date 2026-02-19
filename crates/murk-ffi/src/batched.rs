//! Batched engine FFI: create, step+observe, reset, destroy.
//!
//! Single handle manages N worlds. The hot path (`murk_batched_step_and_observe`)
//! steps all worlds and extracts observations in one call — the Python layer
//! releases the GIL once, covering the entire batch operation.

use std::sync::Mutex;

use murk_core::command::Command;
use murk_core::id::FieldId;
use murk_engine::batched::BatchedEngine;
use murk_engine::config::{BackoffConfig, WorldConfig};
use murk_obs::spec::{ObsDtype, ObsEntry, ObsRegion, ObsSpec, ObsTransform, PoolConfig, PoolKernel};
use murk_space::RegionSpec;
use smallvec::SmallVec;

use crate::command::{convert_command, MurkCommand};
use crate::config::configs;
use crate::handle::HandleTable;
use crate::obs::MurkObsEntry;
use crate::status::MurkStatus;

static BATCHED: Mutex<HandleTable<BatchedEngine>> = Mutex::new(HandleTable::new());

// ── Helpers ─────────────────────────────────────────────────────

/// Convert a C `MurkObsEntry` to a Rust `ObsEntry`.
/// Returns `None` on invalid parameters.
fn convert_obs_entry(e: &MurkObsEntry) -> Option<ObsEntry> {
    let region = match e.region_type {
        0 => ObsRegion::Fixed(RegionSpec::All),
        5 => {
            if e.n_region_params < 1 {
                return None;
            }
            if e.region_params[0] < 0 {
                return None;
            }
            ObsRegion::AgentDisk {
                radius: e.region_params[0] as u32,
            }
        }
        6 => {
            let n = e.n_region_params as usize;
            if n == 0 || n > 8 {
                return None;
            }
            if e.region_params[..n].iter().any(|&v| v < 0) {
                return None;
            }
            let half_extent: SmallVec<[u32; 4]> =
                e.region_params[..n].iter().map(|&v| v as u32).collect();
            ObsRegion::AgentRect { half_extent }
        }
        _ => return None,
    };

    let transform = match e.transform_type {
        0 => ObsTransform::Identity,
        1 => ObsTransform::Normalize {
            min: e.normalize_min as f64,
            max: e.normalize_max as f64,
        },
        _ => return None,
    };

    let dtype = match e.dtype {
        0 => ObsDtype::F32,
        _ => return None,
    };

    let pool = match e.pool_kernel {
        0 => None,
        k @ 1..=4 => {
            let kernel = match k {
                1 => PoolKernel::Mean,
                2 => PoolKernel::Max,
                3 => PoolKernel::Min,
                4 => PoolKernel::Sum,
                _ => unreachable!(),
            };
            if e.pool_kernel_size <= 0 || e.pool_stride <= 0 {
                return None;
            }
            Some(PoolConfig {
                kernel,
                kernel_size: e.pool_kernel_size as usize,
                stride: e.pool_stride as usize,
            })
        }
        _ => return None,
    };

    Some(ObsEntry {
        field_id: FieldId(e.field_id),
        region,
        pool,
        transform,
        dtype,
    })
}

// ── FFI functions ───────────────────────────────────────────────

/// Create a batched engine from N config handles and an optional obs spec.
///
/// `config_handles`: array of N config handles (all consumed, whether success or error).
/// `obs_entries` + `n_entries`: obs plan entries (0 entries = no obs plan).
/// `handle_out`: receives the batched engine handle.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_batched_create(
    config_handles: *const u64,
    n_worlds: usize,
    obs_entries: *const MurkObsEntry,
    n_entries: usize,
    handle_out: *mut u64,
) -> i32 {
    if handle_out.is_null() {
        return MurkStatus::InvalidArgument as i32;
    }
    if n_worlds == 0 || config_handles.is_null() {
        return MurkStatus::InvalidArgument as i32;
    }

    // SAFETY: config_handles points to n_worlds valid u64 values.
    let handles = unsafe { std::slice::from_raw_parts(config_handles, n_worlds) };

    // Consume ALL config handles unconditionally (even on error).
    // Remove every handle first, then check for missing ones.
    let mut configs_table = match configs().lock() {
        Ok(g) => g,
        Err(_) => return MurkStatus::InternalError as i32,
    };
    let mut builders = Vec::with_capacity(n_worlds);
    let mut any_missing = false;
    for &ch in handles {
        match configs_table.remove(ch) {
            Some(b) => builders.push(b),
            None => any_missing = true,
        }
    }
    drop(configs_table);
    if any_missing {
        return MurkStatus::InvalidHandle as i32;
    }

    // Build WorldConfigs from builders.
    let mut world_configs = Vec::with_capacity(n_worlds);
    for builder in builders {
        let space = match builder.space {
            Some(s) => s,
            None => return MurkStatus::ConfigError as i32,
        };
        if builder.fields.is_empty() || builder.propagators.is_empty() {
            return MurkStatus::ConfigError as i32;
        }
        world_configs.push(WorldConfig {
            space,
            fields: builder.fields,
            propagators: builder.propagators,
            dt: builder.dt,
            seed: builder.seed,
            ring_buffer_size: builder.ring_buffer_size,
            max_ingress_queue: builder.max_ingress_queue,
            tick_rate_hz: None,
            backoff: BackoffConfig::default(),
        });
    }

    // Convert obs entries to ObsSpec (if any).
    let obs_spec = if n_entries > 0 {
        if obs_entries.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }
        let entry_slice = unsafe { std::slice::from_raw_parts(obs_entries, n_entries) };
        let mut rust_entries = Vec::with_capacity(n_entries);
        for e in entry_slice {
            match convert_obs_entry(e) {
                Some(re) => rust_entries.push(re),
                None => return MurkStatus::InvalidArgument as i32,
            }
        }
        Some(ObsSpec {
            entries: rust_entries,
        })
    } else {
        None
    };

    // Create the batched engine.
    let engine = match BatchedEngine::new(world_configs, obs_spec.as_ref()) {
        Ok(e) => e,
        Err(e) => {
            return match &e {
                murk_engine::batched::BatchError::Config(ce) => MurkStatus::from(ce) as i32,
                murk_engine::batched::BatchError::Observe(oe) => MurkStatus::from(oe) as i32,
                _ => MurkStatus::ConfigError as i32,
            };
        }
    };

    let handle = match BATCHED.lock() {
        Ok(mut g) => g.insert(engine),
        Err(_) => return MurkStatus::InternalError as i32,
    };
    unsafe { *handle_out = handle };
    MurkStatus::Ok as i32
}

/// Step all worlds and extract observations in one call.
///
/// `cmds_per_world`: array of N pointers to MurkCommand arrays.
/// `n_cmds_per_world`: array of N counts.
/// `obs_output`: pre-allocated buffer, `N * per_world_obs_output_len` f32s.
/// `obs_mask`: pre-allocated buffer, `N * per_world_obs_mask_len` u8s.
/// `tick_ids_out`: array of N u64s for per-world tick IDs (may be null).
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_batched_step_and_observe(
    handle: u64,
    cmds_per_world: *const *const MurkCommand,
    n_cmds_per_world: *const usize,
    obs_output: *mut f32,
    obs_output_len: usize,
    obs_mask: *mut u8,
    obs_mask_len: usize,
    tick_ids_out: *mut u64,
) -> i32 {
    let mut table = match BATCHED.lock() {
        Ok(g) => g,
        Err(_) => return MurkStatus::InternalError as i32,
    };
    let engine = match table.get_mut(handle) {
        Some(e) => e,
        None => return MurkStatus::InvalidHandle as i32,
    };

    let n = engine.num_worlds();

    // Convert commands.
    let commands = match convert_batch_commands(cmds_per_world, n_cmds_per_world, n) {
        Ok(cmds) => cmds,
        Err(status) => return status as i32,
    };

    // SAFETY: caller guarantees buffers are valid.
    let out_slice = if obs_output.is_null() {
        &mut []
    } else {
        unsafe { std::slice::from_raw_parts_mut(obs_output, obs_output_len) }
    };
    let mask_slice = if obs_mask.is_null() {
        &mut []
    } else {
        unsafe { std::slice::from_raw_parts_mut(obs_mask, obs_mask_len) }
    };

    match engine.step_and_observe(&commands, out_slice, mask_slice) {
        Ok(result) => {
            if !tick_ids_out.is_null() {
                for (i, tid) in result.tick_ids.iter().enumerate() {
                    unsafe { *tick_ids_out.add(i) = tid.0 };
                }
            }
            MurkStatus::Ok as i32
        }
        Err(e) => batch_error_to_status(&e),
    }
}

/// Extract observations from all worlds without stepping.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_batched_observe_all(
    handle: u64,
    obs_output: *mut f32,
    obs_output_len: usize,
    obs_mask: *mut u8,
    obs_mask_len: usize,
) -> i32 {
    let table = match BATCHED.lock() {
        Ok(g) => g,
        Err(_) => return MurkStatus::InternalError as i32,
    };
    let engine = match table.get(handle) {
        Some(e) => e,
        None => return MurkStatus::InvalidHandle as i32,
    };

    if obs_output.is_null() || obs_mask.is_null() {
        return MurkStatus::InvalidArgument as i32;
    }

    let out_slice = unsafe { std::slice::from_raw_parts_mut(obs_output, obs_output_len) };
    let mask_slice = unsafe { std::slice::from_raw_parts_mut(obs_mask, obs_mask_len) };

    match engine.observe_all(out_slice, mask_slice) {
        Ok(_) => MurkStatus::Ok as i32,
        Err(e) => batch_error_to_status(&e),
    }
}

/// Reset one world by index.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_batched_reset_world(handle: u64, world_index: usize, seed: u64) -> i32 {
    let mut table = match BATCHED.lock() {
        Ok(g) => g,
        Err(_) => return MurkStatus::InternalError as i32,
    };
    let engine = match table.get_mut(handle) {
        Some(e) => e,
        None => return MurkStatus::InvalidHandle as i32,
    };

    match engine.reset_world(world_index, seed) {
        Ok(()) => MurkStatus::Ok as i32,
        Err(e) => batch_error_to_status(&e),
    }
}

/// Reset all worlds with per-world seeds.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_batched_reset_all(
    handle: u64,
    seeds: *const u64,
    n_seeds: usize,
) -> i32 {
    let mut table = match BATCHED.lock() {
        Ok(g) => g,
        Err(_) => return MurkStatus::InternalError as i32,
    };
    let engine = match table.get_mut(handle) {
        Some(e) => e,
        None => return MurkStatus::InvalidHandle as i32,
    };

    if seeds.is_null() && n_seeds > 0 {
        return MurkStatus::InvalidArgument as i32;
    }

    let seed_slice = if n_seeds > 0 {
        unsafe { std::slice::from_raw_parts(seeds, n_seeds) }
    } else {
        &[]
    };

    match engine.reset_all(seed_slice) {
        Ok(()) => MurkStatus::Ok as i32,
        Err(e) => batch_error_to_status(&e),
    }
}

/// Destroy a batched engine.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_batched_destroy(handle: u64) -> i32 {
    let mut table = match BATCHED.lock() {
        Ok(g) => g,
        Err(_) => return MurkStatus::InternalError as i32,
    };
    match table.remove(handle) {
        Some(_) => MurkStatus::Ok as i32,
        None => MurkStatus::InvalidHandle as i32,
    }
}

/// Number of worlds in the batch.
///
/// Returns 0 for invalid handles.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_batched_num_worlds(handle: u64) -> usize {
    let table = match BATCHED.lock() {
        Ok(g) => g,
        Err(_) => return 0,
    };
    table.get(handle).map_or(0, |e| e.num_worlds())
}

/// Per-world observation output length (f32 elements).
///
/// Returns 0 for invalid handles or if no obs plan.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_batched_obs_output_len(handle: u64) -> usize {
    let table = match BATCHED.lock() {
        Ok(g) => g,
        Err(_) => return 0,
    };
    table.get(handle).map_or(0, |e| e.obs_output_len())
}

/// Per-world observation mask length (bytes).
///
/// Returns 0 for invalid handles or if no obs plan.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_batched_obs_mask_len(handle: u64) -> usize {
    let table = match BATCHED.lock() {
        Ok(g) => g,
        Err(_) => return 0,
    };
    table.get(handle).map_or(0, |e| e.obs_mask_len())
}

// ── Internal helpers ────────────────────────────────────────────

/// Convert per-world command arrays from C to Rust.
#[allow(unsafe_code)]
fn convert_batch_commands(
    cmds_per_world: *const *const MurkCommand,
    n_cmds_per_world: *const usize,
    n_worlds: usize,
) -> Result<Vec<Vec<Command>>, MurkStatus> {
    if n_worlds == 0 {
        return Ok(vec![]);
    }
    if cmds_per_world.is_null() || n_cmds_per_world.is_null() {
        return Err(MurkStatus::InvalidArgument);
    }

    let cmds_ptrs = unsafe { std::slice::from_raw_parts(cmds_per_world, n_worlds) };
    let n_cmds = unsafe { std::slice::from_raw_parts(n_cmds_per_world, n_worlds) };

    let mut all_commands = Vec::with_capacity(n_worlds);
    for i in 0..n_worlds {
        let mut world_cmds = Vec::with_capacity(n_cmds[i]);
        if n_cmds[i] > 0 {
            if cmds_ptrs[i].is_null() {
                return Err(MurkStatus::InvalidArgument);
            }
            let cmd_slice = unsafe { std::slice::from_raw_parts(cmds_ptrs[i], n_cmds[i]) };
            for (j, cmd) in cmd_slice.iter().enumerate() {
                let c = convert_command(cmd, j)?;
                world_cmds.push(c);
            }
        }
        all_commands.push(world_cmds);
    }
    Ok(all_commands)
}

/// Map a BatchError to the appropriate MurkStatus.
fn batch_error_to_status(e: &murk_engine::batched::BatchError) -> i32 {
    use murk_engine::batched::BatchError;
    match e {
        BatchError::Step { error, .. } => MurkStatus::from(error) as i32,
        BatchError::Observe(oe) => MurkStatus::from(oe) as i32,
        BatchError::Config(ce) => MurkStatus::from(ce) as i32,
        BatchError::InvalidIndex { .. } => MurkStatus::InvalidArgument as i32,
        BatchError::NoObsPlan => MurkStatus::InvalidArgument as i32,
        BatchError::InvalidArgument { .. } => MurkStatus::InvalidArgument as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::*;
    use crate::propagator::*;
    use crate::types::*;
    use std::ffi::{c_void, CString};

    // Propagator: writes constant 7.0 to field 0.
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

    fn create_config_handle() -> u64 {
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
        murk_config_add_propagator(cfg_h, prop_h);

        cfg_h
    }

    #[test]
    fn create_step_observe_destroy_lifecycle() {
        let cfg1 = create_config_handle();
        let cfg2 = create_config_handle();
        let handles = [cfg1, cfg2];

        // Obs entry: all of field 0.
        let obs = [MurkObsEntry {
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
        }];

        let mut batch_h: u64 = 0;
        let status = murk_batched_create(
            handles.as_ptr(),
            2,
            obs.as_ptr(),
            1,
            &mut batch_h,
        );
        assert_eq!(status, MurkStatus::Ok as i32);
        assert_eq!(murk_batched_num_worlds(batch_h), 2);
        assert_eq!(murk_batched_obs_output_len(batch_h), 10); // Line1D(10)
        assert_eq!(murk_batched_obs_mask_len(batch_h), 10);

        // Step + observe.
        let cmds: [*const MurkCommand; 2] = [std::ptr::null(), std::ptr::null()];
        let n_cmds = [0usize, 0];
        let mut output = [0.0f32; 20]; // 2 worlds * 10 cells
        let mut mask = [0u8; 20];
        let mut tick_ids = [0u64; 2];

        let status = murk_batched_step_and_observe(
            batch_h,
            cmds.as_ptr(),
            n_cmds.as_ptr(),
            output.as_mut_ptr(),
            20,
            mask.as_mut_ptr(),
            20,
            tick_ids.as_mut_ptr(),
        );
        assert_eq!(status, MurkStatus::Ok as i32);
        assert_eq!(tick_ids, [1, 1]);
        assert!(output.iter().all(|&v| v == 7.0));
        assert!(mask.iter().all(|&m| m == 1));

        // Destroy.
        assert_eq!(murk_batched_destroy(batch_h), MurkStatus::Ok as i32);
    }

    #[test]
    fn use_after_destroy_returns_invalid_handle() {
        let cfg = create_config_handle();
        let handles = [cfg];
        let mut batch_h: u64 = 0;
        murk_batched_create(handles.as_ptr(), 1, std::ptr::null(), 0, &mut batch_h);
        murk_batched_destroy(batch_h);

        assert_eq!(murk_batched_num_worlds(batch_h), 0);
        assert_eq!(
            murk_batched_reset_world(batch_h, 0, 0),
            MurkStatus::InvalidHandle as i32
        );
    }

    #[test]
    fn reset_and_observe() {
        let cfg1 = create_config_handle();
        let cfg2 = create_config_handle();
        let handles = [cfg1, cfg2];

        let obs = [MurkObsEntry {
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
        }];

        let mut batch_h: u64 = 0;
        murk_batched_create(handles.as_ptr(), 2, obs.as_ptr(), 1, &mut batch_h);

        // Step once.
        let cmds: [*const MurkCommand; 2] = [std::ptr::null(), std::ptr::null()];
        let n_cmds = [0usize, 0];
        let mut output = [0.0f32; 20];
        let mut mask = [0u8; 20];
        murk_batched_step_and_observe(
            batch_h,
            cmds.as_ptr(),
            n_cmds.as_ptr(),
            output.as_mut_ptr(),
            20,
            mask.as_mut_ptr(),
            20,
            std::ptr::null_mut(),
        );

        // Reset world 0.
        let status = murk_batched_reset_world(batch_h, 0, 99);
        assert_eq!(status, MurkStatus::Ok as i32);

        // Observe all (world 0 is reset, world 1 still has data).
        let mut output2 = [0.0f32; 20];
        let mut mask2 = [0u8; 20];
        let status = murk_batched_observe_all(
            batch_h,
            output2.as_mut_ptr(),
            20,
            mask2.as_mut_ptr(),
            20,
        );
        assert_eq!(status, MurkStatus::Ok as i32);

        // World 0 is reset → zeroed fields → obs should be 0.0.
        assert!(output2[..10].iter().all(|&v| v == 0.0));
        // World 1 still has const 7.0.
        assert!(output2[10..].iter().all(|&v| v == 7.0));

        murk_batched_destroy(batch_h);
    }
}

#[cfg(test)]
mod p1_regression_tests {
    use super::*;
    use crate::config::*;
    use crate::propagator::*;
    use crate::types::*;
    use std::ffi::{c_void, CString};

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

    fn create_config_handle() -> u64 {
        let mut cfg_h: u64 = 0;
        murk_config_create(&mut cfg_h);
        let params = [10.0f64, 0.0];
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
        murk_config_add_propagator(cfg_h, prop_h);
        cfg_h
    }

    #[test]
    fn negative_disk_radius_rejected() {
        let cfg = create_config_handle();
        let handles = [cfg];
        let obs = [MurkObsEntry {
            field_id: 0,
            region_type: 5,
            transform_type: 0,
            normalize_min: 0.0,
            normalize_max: 0.0,
            dtype: 0,
            region_params: [-1, 0, 0, 0, 0, 0, 0, 0],
            n_region_params: 1,
            pool_kernel: 0,
            pool_kernel_size: 0,
            pool_stride: 0,
        }];
        let mut batch_h: u64 = 0;
        let status =
            murk_batched_create(handles.as_ptr(), 1, obs.as_ptr(), 1, &mut batch_h);
        assert_eq!(status, MurkStatus::InvalidArgument as i32);
    }

    #[test]
    fn negative_rect_half_extent_rejected() {
        let cfg = create_config_handle();
        let handles = [cfg];
        let obs = [MurkObsEntry {
            field_id: 0,
            region_type: 6,
            transform_type: 0,
            normalize_min: 0.0,
            normalize_max: 0.0,
            dtype: 0,
            region_params: [2, -3, 0, 0, 0, 0, 0, 0],
            n_region_params: 2,
            pool_kernel: 0,
            pool_kernel_size: 0,
            pool_stride: 0,
        }];
        let mut batch_h: u64 = 0;
        let status =
            murk_batched_create(handles.as_ptr(), 1, obs.as_ptr(), 1, &mut batch_h);
        assert_eq!(status, MurkStatus::InvalidArgument as i32);
    }

    #[test]
    fn zero_pool_stride_rejected() {
        let cfg = create_config_handle();
        let handles = [cfg];
        let obs = [MurkObsEntry {
            field_id: 0,
            region_type: 0,
            transform_type: 0,
            normalize_min: 0.0,
            normalize_max: 0.0,
            dtype: 0,
            region_params: [0; 8],
            n_region_params: 0,
            pool_kernel: 1,
            pool_kernel_size: 2,
            pool_stride: 0,
        }];
        let mut batch_h: u64 = 0;
        let status =
            murk_batched_create(handles.as_ptr(), 1, obs.as_ptr(), 1, &mut batch_h);
        assert_eq!(status, MurkStatus::InvalidArgument as i32);
    }
}
