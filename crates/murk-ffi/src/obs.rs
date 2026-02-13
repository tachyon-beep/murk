//! Observation plan FFI: compile, execute, and destroy observation plans.
//!
//! An [`ObsPlanCache`] is compiled from an [`ObsSpec`] against a world's space,
//! then executed to fill caller-allocated observation buffers.

use std::sync::Mutex;

use murk_core::id::FieldId;
use murk_obs::cache::ObsPlanCache;
use murk_obs::spec::{ObsDtype, ObsEntry, ObsSpec, ObsTransform};
use murk_space::RegionSpec;

use crate::handle::HandleTable;
use crate::status::MurkStatus;
use crate::world::worlds;

static OBS_PLANS: Mutex<HandleTable<ObsPlanState>> = Mutex::new(HandleTable::new());

struct ObsPlanState {
    cache: ObsPlanCache,
}

/// C-compatible observation entry for plan compilation.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MurkObsEntry {
    /// Field ID to observe.
    pub field_id: u32,
    /// Region type: 0 = All (only option for v1).
    pub region_type: i32,
    /// Transform type: 0 = Identity, 1 = Normalize.
    pub transform_type: i32,
    /// Lower bound for Normalize transform.
    pub normalize_min: f32,
    /// Upper bound for Normalize transform.
    pub normalize_max: f32,
    /// Output data type: 0 = F32.
    pub dtype: i32,
}

/// Result metadata from observation plan execution.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct MurkObsResult {
    /// Tick at which the observed snapshot was produced.
    pub tick_id: u64,
    /// Age of the snapshot relative to the current engine tick.
    pub age_ticks: u64,
}

/// Compile an observation plan against a world's space.
///
/// Takes a world handle (for space access) and an array of observation entries.
/// Returns a plan handle via `plan_out`.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_obsplan_compile(
    world_handle: u64,
    entries: *const MurkObsEntry,
    n_entries: usize,
    plan_out: *mut u64,
) -> i32 {
    if plan_out.is_null() {
        return MurkStatus::InvalidArgument as i32;
    }
    if n_entries == 0 || entries.is_null() {
        return MurkStatus::InvalidObsSpec as i32;
    }

    // SAFETY: entries points to n_entries valid MurkObsEntry structs.
    let entry_slice = unsafe { std::slice::from_raw_parts(entries, n_entries) };

    // Convert C entries to Rust ObsSpec.
    let mut obs_entries = Vec::with_capacity(n_entries);
    for e in entry_slice {
        let region = match e.region_type {
            0 => RegionSpec::All,
            _ => return MurkStatus::InvalidArgument as i32,
        };
        let transform = match e.transform_type {
            0 => ObsTransform::Identity,
            1 => ObsTransform::Normalize {
                min: e.normalize_min as f64,
                max: e.normalize_max as f64,
            },
            _ => return MurkStatus::InvalidArgument as i32,
        };
        let dtype = match e.dtype {
            0 => ObsDtype::F32,
            _ => return MurkStatus::InvalidArgument as i32,
        };
        obs_entries.push(ObsEntry {
            field_id: FieldId(e.field_id),
            region,
            transform,
            dtype,
        });
    }
    let spec = ObsSpec {
        entries: obs_entries,
    };

    // Get the space from the world to trigger initial compilation.
    let w_table = worlds().lock().unwrap();
    let world = match w_table.get(world_handle) {
        Some(w) => w,
        None => return MurkStatus::InvalidHandle as i32,
    };

    let mut cache = ObsPlanCache::new(spec);
    // Compile eagerly so we detect errors now rather than at execute time.
    if let Err(e) = cache.get_or_compile(world.space()) {
        return MurkStatus::from(&e) as i32;
    }
    drop(w_table);

    let state = ObsPlanState { cache };
    let handle = OBS_PLANS.lock().unwrap().insert(state);
    unsafe { *plan_out = handle };
    MurkStatus::Ok as i32
}

/// Execute an observation plan, filling caller-allocated output and mask buffers.
///
/// `output` must have at least `murk_obsplan_output_len()` elements.
/// `mask` must have at least `murk_obsplan_mask_len()` bytes.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_obsplan_execute(
    world_handle: u64,
    plan_handle: u64,
    output: *mut f32,
    output_len: usize,
    mask: *mut u8,
    mask_len: usize,
    result_out: *mut MurkObsResult,
) -> i32 {
    if output.is_null() || mask.is_null() {
        return MurkStatus::InvalidArgument as i32;
    }

    let mut plans = OBS_PLANS.lock().unwrap();
    let plan_state = match plans.get_mut(plan_handle) {
        Some(s) => s,
        None => return MurkStatus::InvalidHandle as i32,
    };

    // Check buffer sizes.
    let expected_out = plan_state.cache.output_len().unwrap_or(0);
    let expected_mask = plan_state.cache.mask_len().unwrap_or(0);
    if output_len < expected_out {
        return MurkStatus::BufferTooSmall as i32;
    }
    if mask_len < expected_mask {
        return MurkStatus::BufferTooSmall as i32;
    }

    // SAFETY: output/mask point to output_len/mask_len valid elements.
    let out_slice = unsafe { std::slice::from_raw_parts_mut(output, output_len) };
    let mask_slice = unsafe { std::slice::from_raw_parts_mut(mask, mask_len) };

    let w_table = worlds().lock().unwrap();
    let world = match w_table.get(world_handle) {
        Some(w) => w,
        None => return MurkStatus::InvalidHandle as i32,
    };

    let snap = world.snapshot();

    match plan_state
        .cache
        .execute(world.space(), &snap, None, out_slice, mask_slice)
    {
        Ok(meta) => {
            if !result_out.is_null() {
                unsafe {
                    *result_out = MurkObsResult {
                        tick_id: meta.tick_id.0,
                        age_ticks: meta.age_ticks,
                    };
                }
            }
            MurkStatus::Ok as i32
        }
        Err(e) => MurkStatus::from(&e) as i32,
    }
}

/// Destroy an observation plan.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_obsplan_destroy(plan_handle: u64) -> i32 {
    match OBS_PLANS.lock().unwrap().remove(plan_handle) {
        Some(_) => MurkStatus::Ok as i32,
        None => MurkStatus::InvalidHandle as i32,
    }
}

/// Query the output length (in f32 elements) of a compiled plan.
///
/// Returns -1 if the handle is invalid.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_obsplan_output_len(plan_handle: u64) -> i64 {
    let plans = OBS_PLANS.lock().unwrap();
    match plans.get(plan_handle) {
        Some(s) => s.cache.output_len().map_or(-1, |l| l as i64),
        None => -1,
    }
}

/// Query the mask length (in bytes) of a compiled plan.
///
/// Returns -1 if the handle is invalid.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_obsplan_mask_len(plan_handle: u64) -> i64 {
    let plans = OBS_PLANS.lock().unwrap();
    match plans.get(plan_handle) {
        Some(s) => s.cache.mask_len().map_or(-1, |l| l as i64),
        None => -1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Re-use world creation from world.rs tests.
    fn create_test_world() -> u64 {
        use crate::config::*;
        use crate::propagator::*;
        use crate::types::*;
        use crate::world::murk_lockstep_create;
        use std::ffi::{c_void, CString};

        #[allow(unsafe_code)]
        unsafe extern "C" fn const_step(
            _ud: *mut c_void,
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
            for (i, v) in slice.iter_mut().enumerate() {
                *v = (i + 1) as f32;
            }
            0
        }

        let mut cfg_h: u64 = 0;
        murk_config_create(&mut cfg_h);

        // Square4 3x3 => 9 cells, for obs testing.
        let params = [3.0f64, 3.0, 0.0];
        murk_config_set_space(cfg_h, MurkSpaceType::Square4 as i32, params.as_ptr(), 3);

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

        let prop_name = CString::new("seq").unwrap();
        let writes = [MurkWriteDecl {
            field_id: 0,
            mode: MurkWriteMode::Full,
        }];
        let def = MurkPropagatorDef {
            name: prop_name.as_ptr(),
            reads: std::ptr::null(),
            n_reads: 0,
            reads_previous: std::ptr::null(),
            n_reads_previous: 0,
            writes: writes.as_ptr(),
            n_writes: 1,
            step_fn: const_step,
            user_data: std::ptr::null_mut(),
            scratch_bytes: 0,
        };
        let mut prop_h: u64 = 0;
        murk_propagator_create(&def, &mut prop_h);
        murk_config_add_propagator(cfg_h, prop_h);

        let mut world_h: u64 = 0;
        murk_lockstep_create(cfg_h, &mut world_h);
        world_h
    }

    #[test]
    fn compile_execute_matches_rust_side() {
        let world_h = create_test_world();

        // Step once to populate data.
        crate::world::murk_lockstep_step(
            world_h,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );

        // Compile obs plan.
        let entry = MurkObsEntry {
            field_id: 0,
            region_type: 0,
            transform_type: 0,
            normalize_min: 0.0,
            normalize_max: 0.0,
            dtype: 0,
        };
        let mut plan_h: u64 = 0;
        let status = murk_obsplan_compile(world_h, &entry, 1, &mut plan_h);
        assert_eq!(status, MurkStatus::Ok as i32);

        // Check lengths.
        assert_eq!(murk_obsplan_output_len(plan_h), 9);
        assert_eq!(murk_obsplan_mask_len(plan_h), 9);

        // Execute.
        let mut output = [0.0f32; 9];
        let mut mask = [0u8; 9];
        let mut result = MurkObsResult::default();
        let status = murk_obsplan_execute(
            world_h,
            plan_h,
            output.as_mut_ptr(),
            9,
            mask.as_mut_ptr(),
            9,
            &mut result,
        );
        assert_eq!(status, MurkStatus::Ok as i32);
        assert_eq!(result.tick_id, 1);
        assert_eq!(result.age_ticks, 0);
        // Values should be 1..=9 from the sequential propagator.
        let expected: Vec<f32> = (1..=9).map(|x| x as f32).collect();
        assert_eq!(&output[..], &expected[..]);
        assert!(mask.iter().all(|&m| m == 1));

        murk_obsplan_destroy(plan_h);
        crate::world::murk_lockstep_destroy(world_h);
    }

    #[test]
    fn compile_destroy_execute_returns_invalid_handle() {
        let world_h = create_test_world();

        crate::world::murk_lockstep_step(
            world_h,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );

        let entry = MurkObsEntry {
            field_id: 0,
            region_type: 0,
            transform_type: 0,
            normalize_min: 0.0,
            normalize_max: 0.0,
            dtype: 0,
        };
        let mut plan_h: u64 = 0;
        murk_obsplan_compile(world_h, &entry, 1, &mut plan_h);
        murk_obsplan_destroy(plan_h);

        let mut output = [0.0f32; 9];
        let mut mask = [0u8; 9];
        let status = murk_obsplan_execute(
            world_h,
            plan_h,
            output.as_mut_ptr(),
            9,
            mask.as_mut_ptr(),
            9,
            std::ptr::null_mut(),
        );
        assert_eq!(status, MurkStatus::InvalidHandle as i32);

        crate::world::murk_lockstep_destroy(world_h);
    }

    #[test]
    fn execute_buffer_too_small() {
        let world_h = create_test_world();

        crate::world::murk_lockstep_step(
            world_h,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );

        let entry = MurkObsEntry {
            field_id: 0,
            region_type: 0,
            transform_type: 0,
            normalize_min: 0.0,
            normalize_max: 0.0,
            dtype: 0,
        };
        let mut plan_h: u64 = 0;
        murk_obsplan_compile(world_h, &entry, 1, &mut plan_h);

        let mut output = [0.0f32; 4]; // too small
        let mut mask = [0u8; 9];
        let status = murk_obsplan_execute(
            world_h,
            plan_h,
            output.as_mut_ptr(),
            4,
            mask.as_mut_ptr(),
            9,
            std::ptr::null_mut(),
        );
        assert_eq!(status, MurkStatus::BufferTooSmall as i32);

        murk_obsplan_destroy(plan_h);
        crate::world::murk_lockstep_destroy(world_h);
    }

    #[test]
    fn output_len_mask_len_correct() {
        let world_h = create_test_world();

        crate::world::murk_lockstep_step(
            world_h,
            std::ptr::null(),
            0,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );

        let entry = MurkObsEntry {
            field_id: 0,
            region_type: 0,
            transform_type: 0,
            normalize_min: 0.0,
            normalize_max: 0.0,
            dtype: 0,
        };
        let mut plan_h: u64 = 0;
        murk_obsplan_compile(world_h, &entry, 1, &mut plan_h);

        assert_eq!(murk_obsplan_output_len(plan_h), 9);
        assert_eq!(murk_obsplan_mask_len(plan_h), 9);

        // Invalid handle returns -1.
        assert_eq!(murk_obsplan_output_len(0xDEAD), -1);
        assert_eq!(murk_obsplan_mask_len(0xDEAD), -1);

        murk_obsplan_destroy(plan_h);
        crate::world::murk_lockstep_destroy(world_h);
    }

    #[test]
    fn invalid_entries_return_invalid_obsspec() {
        let world_h = create_test_world();

        let mut plan_h: u64 = 0;
        // Empty entries.
        let status =
            murk_obsplan_compile(world_h, std::ptr::null(), 0, &mut plan_h);
        assert_eq!(status, MurkStatus::InvalidObsSpec as i32);

        crate::world::murk_lockstep_destroy(world_h);
    }
}
