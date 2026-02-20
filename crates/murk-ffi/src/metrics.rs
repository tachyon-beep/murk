//! C-compatible step metrics.

use std::ffi::c_char;

use crate::status::MurkStatus;

/// C-compatible step metrics returned from `murk_lockstep_step`.
#[repr(C)]
#[derive(Clone, Debug, Default)]
pub struct MurkStepMetrics {
    /// Wall-clock time for the entire tick, in microseconds.
    pub total_us: u64,
    /// Time spent processing the ingress command queue, in microseconds.
    pub command_processing_us: u64,
    /// Time spent publishing the snapshot, in microseconds.
    pub snapshot_publish_us: u64,
    /// Memory usage of the arena after the tick, in bytes.
    /// Fixed-width `u64` for ABI portability (not `usize`).
    pub memory_bytes: u64,
    /// Number of propagators executed.
    pub n_propagators: u32,
}

// Compile-time layout assertions for ABI stability.
// 3×u64 + 1×u64 + 1×u32 + 4 bytes padding = 40 bytes, align 8.
const _: () = assert!(std::mem::size_of::<MurkStepMetrics>() == 40);
const _: () = assert!(std::mem::align_of::<MurkStepMetrics>() == 8);

impl MurkStepMetrics {
    pub(crate) fn from_rust(m: &murk_engine::StepMetrics) -> Self {
        Self {
            total_us: m.total_us,
            command_processing_us: m.command_processing_us,
            snapshot_publish_us: m.snapshot_publish_us,
            memory_bytes: m.memory_bytes as u64,
            n_propagators: m.propagator_us.len() as u32,
        }
    }
}

/// Query per-propagator timing from the most recent step.
///
/// `index` is 0-based. Writes the propagator name into `name_buf` (up to
/// `name_cap` bytes including null terminator) and its execution time
/// into `us_out`.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_step_metrics_propagator(
    world_handle: u64,
    index: u32,
    name_buf: *mut c_char,
    name_cap: usize,
    us_out: *mut u64,
) -> i32 {
    use crate::world::worlds;

    if us_out.is_null() {
        return MurkStatus::InvalidArgument as i32;
    }

    let world_arc = {
        let table = ffi_lock!(worlds());
        match table.get(world_handle).cloned() {
            Some(arc) => arc,
            None => return MurkStatus::InvalidHandle as i32,
        }
    };
    let world = ffi_lock!(world_arc);

    let metrics = world.last_metrics();
    let idx = index as usize;
    if idx >= metrics.propagator_us.len() {
        return MurkStatus::InvalidArgument as i32;
    }

    let (ref name, us) = metrics.propagator_us[idx];

    // SAFETY: us_out is valid per caller contract.
    unsafe { *us_out = us };

    // Write name if buffer provided.
    if !name_buf.is_null() && name_cap > 0 {
        let bytes = name.as_bytes();
        let copy_len = bytes.len().min(name_cap - 1);
        // SAFETY: name_buf points to name_cap valid bytes.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), name_buf as *mut u8, copy_len);
            *name_buf.add(copy_len) = 0; // null-terminate
        }
    }

    MurkStatus::Ok as i32
}

/// Retrieve latest metrics for a world.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_step_metrics(world_handle: u64, out: *mut MurkStepMetrics) -> i32 {
    use crate::world::worlds;

    if out.is_null() {
        return MurkStatus::InvalidArgument as i32;
    }

    let world_arc = {
        let table = ffi_lock!(worlds());
        match table.get(world_handle).cloned() {
            Some(arc) => arc,
            None => return MurkStatus::InvalidHandle as i32,
        }
    };
    let world = ffi_lock!(world_arc);

    let metrics = MurkStepMetrics::from_rust(world.last_metrics());
    // SAFETY: out is valid per caller contract.
    unsafe { *out = metrics };
    MurkStatus::Ok as i32
}
