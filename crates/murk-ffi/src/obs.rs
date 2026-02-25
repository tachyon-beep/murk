//! Observation plan FFI: compile, execute, and destroy observation plans.
//!
//! An [`ObsPlanCache`] is compiled from an [`ObsSpec`] against a world's space,
//! then executed to fill caller-allocated observation buffers.

use std::sync::{Arc, Mutex};

use murk_core::id::FieldId;
use murk_core::Coord;
use murk_obs::cache::ObsPlanCache;
use murk_obs::spec::{
    ObsDtype, ObsEntry, ObsRegion, ObsSpec, ObsTransform, PoolConfig, PoolKernel,
};
use murk_space::RegionSpec;
use smallvec::SmallVec;

use crate::handle::HandleTable;
use crate::status::MurkStatus;
use crate::world::worlds;

// Compile-time layout assertions for ABI stability.
const _: () = assert!(std::mem::align_of::<MurkObsEntry>() == 4);
const _: () = assert!(std::mem::align_of::<MurkObsResult>() == 8);
const _: () = assert!(std::mem::size_of::<MurkObsResult>() == 16);

type ObsPlanArc = Arc<Mutex<ObsPlanState>>;

static OBS_PLANS: Mutex<HandleTable<ObsPlanArc>> = Mutex::new(HandleTable::new());

/// Hard limits for FFI agent-batch execution to avoid pathological allocations.
///
/// These are API-facing guardrails, not core engine limits.
const MAX_EXECUTE_AGENTS: usize = 1_000_000;
const MAX_EXECUTE_AGENT_DIMS: usize = 16;

/// Clone the Arc for a plan handle, briefly locking the global table.
///
/// Returns `None` if the handle is invalid or the mutex is poisoned.
/// On poisoning, stores a diagnostic in [`LAST_PANIC`] so the caller
/// can retrieve context via `murk_last_panic_message`.
fn get_obs_plan(handle: u64) -> Option<ObsPlanArc> {
    match OBS_PLANS.lock() {
        Ok(table) => table.get(handle).cloned(),
        Err(_) => {
            crate::LAST_PANIC.with(|cell| {
                *cell.borrow_mut() =
                    "OBS_PLANS mutex poisoned: a prior panic corrupted shared state".into();
            });
            None
        }
    }
}

/// Convert a C `MurkObsEntry` to a Rust `ObsEntry`.
/// Returns `None` on invalid parameters.
///
/// Shared by both `obs.rs` (single-world obs plans) and `batched.rs`
/// (batched engine obs specs) to avoid divergence.
pub(crate) fn convert_obs_entry(e: &MurkObsEntry) -> Option<ObsEntry> {
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

struct ObsPlanState {
    cache: ObsPlanCache,
}

/// C-compatible observation entry for plan compilation.
///
/// Region type values:
/// - 0: All (whole grid)
/// - 5: AgentDisk (radius in `region_params[0]`)
/// - 6: AgentRect (half-extents in `region_params[0..n_region_params]`)
///
/// Pool kernel values:
/// - 0: None (no pooling)
/// - 1: Mean
/// - 2: Max
/// - 3: Min
/// - 4: Sum
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MurkObsEntry {
    /// Field ID to observe.
    pub field_id: u32,
    /// Region type: 0=All, 5=AgentDisk, 6=AgentRect.
    pub region_type: i32,
    /// Transform type: 0 = Identity, 1 = Normalize.
    pub transform_type: i32,
    /// Lower bound for Normalize transform.
    pub normalize_min: f32,
    /// Upper bound for Normalize transform.
    pub normalize_max: f32,
    /// Output data type: 0 = F32.
    pub dtype: i32,
    /// Region parameters (interpretation depends on region_type).
    pub region_params: [i32; 8],
    /// Number of valid entries in `region_params`.
    pub n_region_params: i32,
    /// Pooling kernel: 0=None, 1=Mean, 2=Max, 3=Min, 4=Sum.
    pub pool_kernel: i32,
    /// Pooling window size (ignored if pool_kernel == 0).
    pub pool_kernel_size: i32,
    /// Pooling stride (ignored if pool_kernel == 0).
    pub pool_stride: i32,
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
    ffi_guard!({
        if plan_out.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }
        if n_entries == 0 || entries.is_null() {
            return MurkStatus::InvalidObsSpec as i32;
        }

        // SAFETY: entries points to n_entries valid MurkObsEntry structs.
        let entry_slice = unsafe { std::slice::from_raw_parts(entries, n_entries) };

        // Convert C entries to Rust ObsSpec using shared conversion function.
        let mut obs_entries = Vec::with_capacity(n_entries);
        for e in entry_slice {
            match convert_obs_entry(e) {
                Some(entry) => obs_entries.push(entry),
                None => return MurkStatus::InvalidArgument as i32,
            }
        }
        let spec = ObsSpec {
            entries: obs_entries,
        };

        // Get the space from the world to trigger initial compilation.
        let world_arc = {
            let w_table = ffi_lock!(worlds());
            match w_table.get(world_handle).cloned() {
                Some(arc) => arc,
                None => return MurkStatus::InvalidHandle as i32,
            }
        };
        let world = ffi_lock!(world_arc);

        let mut cache = ObsPlanCache::new(spec);
        // Compile eagerly so we detect errors now rather than at execute time.
        if let Err(e) = cache.get_or_compile(world.space()) {
            return MurkStatus::from(&e) as i32;
        }
        drop(world);

        let state = Arc::new(Mutex::new(ObsPlanState { cache }));
        let handle = ffi_lock!(OBS_PLANS).insert(state);
        unsafe { *plan_out = handle };
        MurkStatus::Ok as i32
    })
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
    ffi_guard!({
        if output.is_null() || mask.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }

        // Acquire per-plan Arc briefly, then drop global table lock.
        let plan_arc = match get_obs_plan(plan_handle) {
            Some(arc) => arc,
            None => return MurkStatus::InvalidHandle as i32,
        };
        let mut plan_state = ffi_lock!(plan_arc);

        // Check buffer sizes. None means plan not compiled.
        let expected_out = match plan_state.cache.output_len() {
            Some(v) => v,
            None => return MurkStatus::InvalidObsSpec as i32,
        };
        let expected_mask = match plan_state.cache.mask_len() {
            Some(v) => v,
            None => return MurkStatus::InvalidObsSpec as i32,
        };
        if output_len < expected_out {
            return MurkStatus::BufferTooSmall as i32;
        }
        if mask_len < expected_mask {
            return MurkStatus::BufferTooSmall as i32;
        }

        // SAFETY: output/mask point to output_len/mask_len valid elements.
        let out_slice = unsafe { std::slice::from_raw_parts_mut(output, output_len) };
        let mask_slice = unsafe { std::slice::from_raw_parts_mut(mask, mask_len) };

        // Acquire per-world Arc briefly, then drop global table lock.
        // Lock ordering: no global table locks are held at this point.
        let world_arc = {
            let w_table = ffi_lock!(worlds());
            match w_table.get(world_handle).cloned() {
                Some(arc) => arc,
                None => return MurkStatus::InvalidHandle as i32,
            }
        };
        let world = ffi_lock!(world_arc);

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
    })
}

/// Execute an observation plan for N agents, filling caller-allocated buffers.
///
/// `agent_centers` is a flat array of `n_agents * ndim` i32 values.
/// `output` must have at least `n_agents * murk_obsplan_output_len()` elements.
/// `mask` must have at least `n_agents * murk_obsplan_mask_len()` bytes.
/// `results_out` may be null; if non-null, must point to `n_agents` results.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_obsplan_execute_agents(
    world_handle: u64,
    plan_handle: u64,
    agent_centers: *const i32,
    ndim: i32,
    n_agents: i32,
    output: *mut f32,
    output_len: usize,
    mask: *mut u8,
    mask_len: usize,
    results_out: *mut MurkObsResult,
) -> i32 {
    ffi_guard!({
        if output.is_null() || mask.is_null() || agent_centers.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }
        if n_agents <= 0 || ndim <= 0 {
            return MurkStatus::InvalidArgument as i32;
        }
        let n = n_agents as usize;
        let dim = ndim as usize;
        if n > MAX_EXECUTE_AGENTS || dim > MAX_EXECUTE_AGENT_DIMS {
            return MurkStatus::InvalidArgument as i32;
        }

        let centers_len = match n.checked_mul(dim) {
            Some(v) => v,
            None => return MurkStatus::InvalidArgument as i32,
        };

        // Acquire per-plan Arc briefly, then drop global table lock.
        let plan_arc = match get_obs_plan(plan_handle) {
            Some(arc) => arc,
            None => return MurkStatus::InvalidHandle as i32,
        };
        let mut plan_state = ffi_lock!(plan_arc);

        // Validate caller buffers before doing heavier work.
        let expected_out = match plan_state
            .cache
            .output_len()
            .and_then(|per_agent| per_agent.checked_mul(n))
        {
            Some(v) => v,
            None => return MurkStatus::InvalidArgument as i32,
        };
        let expected_mask = match plan_state
            .cache
            .mask_len()
            .and_then(|per_agent| per_agent.checked_mul(n))
        {
            Some(v) => v,
            None => return MurkStatus::InvalidArgument as i32,
        };
        if output_len < expected_out || mask_len < expected_mask {
            return MurkStatus::BufferTooSmall as i32;
        }

        // SAFETY: agent_centers points to `centers_len` valid i32 values.
        let centers_flat = unsafe { std::slice::from_raw_parts(agent_centers, centers_len) };
        let centers: Vec<Coord> = centers_flat
            .chunks_exact(dim)
            .map(|chunk| chunk.iter().copied().collect())
            .collect();

        // SAFETY: output/mask point to output_len/mask_len valid elements.
        let out_slice = unsafe { std::slice::from_raw_parts_mut(output, output_len) };
        let mask_slice = unsafe { std::slice::from_raw_parts_mut(mask, mask_len) };

        // Acquire per-world Arc briefly, then drop global table lock.
        // Lock ordering: no global table locks are held at this point.
        let world_arc = {
            let w_table = ffi_lock!(worlds());
            match w_table.get(world_handle).cloned() {
                Some(arc) => arc,
                None => return MurkStatus::InvalidHandle as i32,
            }
        };
        let world = ffi_lock!(world_arc);
        let snap = world.snapshot();

        match plan_state.cache.execute_agents(
            world.space(),
            &snap,
            &centers,
            None,
            out_slice,
            mask_slice,
        ) {
            Ok(metas) => {
                if !results_out.is_null() {
                    for (i, meta) in metas.iter().enumerate() {
                        unsafe {
                            *results_out.add(i) = MurkObsResult {
                                tick_id: meta.tick_id.0,
                                age_ticks: meta.age_ticks,
                            };
                        }
                    }
                }
                MurkStatus::Ok as i32
            }
            Err(e) => MurkStatus::from(&e) as i32,
        }
    })
}

/// Destroy an observation plan.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_obsplan_destroy(plan_handle: u64) -> i32 {
    ffi_guard!({
        match ffi_lock!(OBS_PLANS).remove(plan_handle) {
            Some(_) => MurkStatus::Ok as i32,
            None => MurkStatus::InvalidHandle as i32,
        }
    })
}

/// Query the output length (in f32 elements) of a compiled plan.
///
/// Returns -1 if the handle is invalid or mutex is poisoned.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_obsplan_output_len(plan_handle: u64) -> i64 {
    ffi_guard_or!(-1, {
        get_obs_plan(plan_handle)
            .and_then(|arc| {
                arc.lock()
                    .ok()
                    .and_then(|s| s.cache.output_len().map(|l| l as i64))
            })
            .unwrap_or(-1)
    })
}

/// Query the mask length (in bytes) of a compiled plan.
///
/// Returns -1 if the handle is invalid or mutex is poisoned.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_obsplan_mask_len(plan_handle: u64) -> i64 {
    ffi_guard_or!(-1, {
        get_obs_plan(plan_handle)
            .and_then(|arc| {
                arc.lock()
                    .ok()
                    .and_then(|s| s.cache.mask_len().map(|l| l as i64))
            })
            .unwrap_or(-1)
    })
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
        unsafe extern "C" fn const_step(_ud: *mut c_void, ctx: *const MurkStepContext) -> i32 {
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
            step_fn: Some(const_step),
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
            region_params: [0; 8],
            n_region_params: 0,
            pool_kernel: 0,
            pool_kernel_size: 0,
            pool_stride: 0,
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
            region_params: [0; 8],
            n_region_params: 0,
            pool_kernel: 0,
            pool_kernel_size: 0,
            pool_stride: 0,
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
            region_params: [0; 8],
            n_region_params: 0,
            pool_kernel: 0,
            pool_kernel_size: 0,
            pool_stride: 0,
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
            region_params: [0; 8],
            n_region_params: 0,
            pool_kernel: 0,
            pool_kernel_size: 0,
            pool_stride: 0,
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
        let status = murk_obsplan_compile(world_h, std::ptr::null(), 0, &mut plan_h);
        assert_eq!(status, MurkStatus::InvalidObsSpec as i32);

        crate::world::murk_lockstep_destroy(world_h);
    }

    #[test]
    fn execute_agents_rejects_excessive_agent_count() {
        let mut output = [0.0f32; 1];
        let mut mask = [0u8; 1];
        let centers = [0i32; 1];

        let status = murk_obsplan_execute_agents(
            0,
            0,
            centers.as_ptr(),
            1,
            i32::MAX,
            output.as_mut_ptr(),
            output.len(),
            mask.as_mut_ptr(),
            mask.len(),
            std::ptr::null_mut(),
        );
        assert_eq!(status, MurkStatus::InvalidArgument as i32);
    }

    #[test]
    fn execute_agents_rejects_excessive_dimensions() {
        let mut output = [0.0f32; 1];
        let mut mask = [0u8; 1];
        let centers = [0i32; 1];

        let status = murk_obsplan_execute_agents(
            0,
            0,
            centers.as_ptr(),
            17,
            1,
            output.as_mut_ptr(),
            output.len(),
            mask.as_mut_ptr(),
            mask.len(),
            std::ptr::null_mut(),
        );
        assert_eq!(status, MurkStatus::InvalidArgument as i32);
    }

    #[test]
    fn execute_agents_rejects_legacy_overflow_shape_inputs() {
        let mut output = [0.0f32; 1];
        let mut mask = [0u8; 1];
        let centers = [0i32; 1];

        // These dimensions would overflow legacy unchecked arithmetic
        // (`n_agents * ndim`) and must deterministically reject.
        let status = murk_obsplan_execute_agents(
            0,
            0,
            centers.as_ptr(),
            i32::MAX,
            i32::MAX,
            output.as_mut_ptr(),
            output.len(),
            mask.as_mut_ptr(),
            mask.len(),
            std::ptr::null_mut(),
        );
        assert_eq!(status, MurkStatus::InvalidArgument as i32);
    }

    #[test]
    fn execute_agents_rejects_too_small_output_for_multiple_agents() {
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
            region_params: [0; 8],
            n_region_params: 0,
            pool_kernel: 0,
            pool_kernel_size: 0,
            pool_stride: 0,
        };
        let mut plan_h: u64 = 0;
        assert_eq!(
            murk_obsplan_compile(world_h, &entry, 1, &mut plan_h),
            MurkStatus::Ok as i32
        );

        let centers = [0i32, 1i32];
        let mut output = vec![0.0f32; 17];
        let mut mask = vec![0u8; 18];
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

        murk_obsplan_destroy(plan_h);
        crate::world::murk_lockstep_destroy(world_h);
    }

    #[test]
    fn execute_agents_rejects_too_small_mask_for_multiple_agents() {
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
            region_params: [0; 8],
            n_region_params: 0,
            pool_kernel: 0,
            pool_kernel_size: 0,
            pool_stride: 0,
        };
        let mut plan_h: u64 = 0;
        assert_eq!(
            murk_obsplan_compile(world_h, &entry, 1, &mut plan_h),
            MurkStatus::Ok as i32
        );

        let centers = [0i32, 1i32];
        let mut output = vec![0.0f32; 18];
        let mut mask = vec![0u8; 17];
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

        murk_obsplan_destroy(plan_h);
        crate::world::murk_lockstep_destroy(world_h);
    }
}
