//! C-compatible step metrics.
//!
//! Per-propagator timings are snapshotted into a thread-local buffer during
//! `murk_lockstep_step` (while the world lock is held). This ensures that
//! subsequent calls to `murk_step_metrics_propagator` return data from the
//! same tick, even if another thread steps the same world concurrently.

use std::cell::RefCell;
use std::ffi::c_char;

use crate::status::MurkStatus;

thread_local! {
    /// Per-propagator timings snapshotted during the most recent
    /// `murk_lockstep_step` on this thread.
    static LAST_PROPAGATOR_US: RefCell<Vec<(String, u64)>> = const { RefCell::new(Vec::new()) };
}

/// Snapshot propagator timings from a step result into the thread-local
/// buffer. Called by `murk_lockstep_step` while the world lock is held.
pub(crate) fn snapshot_propagator_timings(us: &[(String, u64)]) {
    LAST_PROPAGATOR_US.with(|cell| {
        let mut buf = cell.borrow_mut();
        buf.clear();
        buf.extend(us.iter().cloned());
    });
}

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
    /// Number of sparse segment ranges available for reuse.
    pub sparse_retired_ranges: u32,
    /// Number of sparse segment ranges pending promotion (freed this tick).
    pub sparse_pending_retired: u32,
    /// Number of sparse alloc() calls that reused a retired range this tick.
    pub sparse_reuse_hits: u32,
    /// Number of sparse alloc() calls that fell through to bump allocation this tick.
    pub sparse_reuse_misses: u32,
    /// Cumulative number of ingress rejections due to full queue.
    pub queue_full_rejections: u64,
    /// Cumulative number of ingress rejections due to tick-disabled state.
    pub tick_disabled_rejections: u64,
    /// Cumulative number of rollback events.
    pub rollback_events: u64,
    /// Cumulative number of transitions into tick-disabled state.
    pub tick_disabled_transitions: u64,
    /// Cumulative number of worker stall force-unpin events.
    pub worker_stall_events: u64,
    /// Cumulative number of ring "not available" events.
    pub ring_not_available_events: u64,
    /// Cumulative number of snapshot evictions due to ring overwrite.
    pub ring_eviction_events: u64,
    /// Cumulative number of stale/not-yet-written position reads.
    pub ring_stale_read_events: u64,
    /// Cumulative number of reader retries caused by overwrite skew.
    pub ring_skew_retry_events: u64,
}

// Compile-time layout assertions for ABI stability.
// 4×u64 + 5×u32 + 4 bytes padding + 9×u64 = 128 bytes, align 8.
const _: () = assert!(std::mem::size_of::<MurkStepMetrics>() == 128);
const _: () = assert!(std::mem::align_of::<MurkStepMetrics>() == 8);

impl MurkStepMetrics {
    pub(crate) fn from_rust(m: &murk_engine::StepMetrics) -> Self {
        Self {
            total_us: m.total_us,
            command_processing_us: m.command_processing_us,
            snapshot_publish_us: m.snapshot_publish_us,
            memory_bytes: m.memory_bytes as u64,
            n_propagators: m.propagator_us.len() as u32,
            sparse_retired_ranges: m.sparse_retired_ranges,
            sparse_pending_retired: m.sparse_pending_retired,
            sparse_reuse_hits: m.sparse_reuse_hits,
            sparse_reuse_misses: m.sparse_reuse_misses,
            queue_full_rejections: m.queue_full_rejections,
            tick_disabled_rejections: m.tick_disabled_rejections,
            rollback_events: m.rollback_events,
            tick_disabled_transitions: m.tick_disabled_transitions,
            worker_stall_events: m.worker_stall_events,
            ring_not_available_events: m.ring_not_available_events,
            ring_eviction_events: m.ring_eviction_events,
            ring_stale_read_events: m.ring_stale_read_events,
            ring_skew_retry_events: m.ring_skew_retry_events,
        }
    }
}

/// Query per-propagator timing from the most recent step on this thread.
///
/// Reads from the thread-local snapshot populated by `murk_lockstep_step`,
/// ensuring consistency with the aggregate `MurkStepMetrics` from the same
/// call. `world_handle` is accepted for API compatibility but unused — the
/// data comes from the thread-local buffer, not a world lock re-acquisition.
///
/// `index` is 0-based. Writes the propagator name into `name_buf` (up to
/// `name_cap` bytes including null terminator) and its execution time
/// into `us_out`.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_step_metrics_propagator(
    _world_handle: u64,
    index: u32,
    name_buf: *mut c_char,
    name_cap: usize,
    us_out: *mut u64,
) -> i32 {
    ffi_guard!({
        if us_out.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }

        LAST_PROPAGATOR_US.with(|cell| {
            let data = cell.borrow();
            let idx = index as usize;
            if idx >= data.len() {
                return MurkStatus::InvalidArgument as i32;
            }

            let (ref name, us) = data[idx];

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
        })
    })
}

/// Retrieve latest metrics for a world.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_step_metrics(world_handle: u64, out: *mut MurkStepMetrics) -> i32 {
    ffi_guard!({
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_rust_converts_sparse_metrics() {
        let rust_metrics = murk_engine::StepMetrics {
            total_us: 500,
            command_processing_us: 100,
            propagator_us: vec![("heat".to_string(), 200)],
            snapshot_publish_us: 50,
            memory_bytes: 8192,
            sparse_retired_ranges: 7,
            sparse_pending_retired: 2,
            sparse_reuse_hits: 5,
            sparse_reuse_misses: 3,
            queue_full_rejections: 11,
            tick_disabled_rejections: 4,
            rollback_events: 2,
            tick_disabled_transitions: 1,
            worker_stall_events: 3,
            ring_not_available_events: 7,
            ring_eviction_events: 9,
            ring_stale_read_events: 4,
            ring_skew_retry_events: 2,
        };
        let ffi = MurkStepMetrics::from_rust(&rust_metrics);
        assert_eq!(ffi.sparse_retired_ranges, 7);
        assert_eq!(ffi.sparse_pending_retired, 2);
        assert_eq!(ffi.sparse_reuse_hits, 5);
        assert_eq!(ffi.sparse_reuse_misses, 3);
        assert_eq!(ffi.queue_full_rejections, 11);
        assert_eq!(ffi.tick_disabled_rejections, 4);
        assert_eq!(ffi.rollback_events, 2);
        assert_eq!(ffi.tick_disabled_transitions, 1);
        assert_eq!(ffi.worker_stall_events, 3);
        assert_eq!(ffi.ring_not_available_events, 7);
        assert_eq!(ffi.ring_eviction_events, 9);
        assert_eq!(ffi.ring_stale_read_events, 4);
        assert_eq!(ffi.ring_skew_retry_events, 2);
    }

    #[test]
    fn default_sparse_fields_are_zero() {
        let m = MurkStepMetrics::default();
        assert_eq!(m.sparse_retired_ranges, 0);
        assert_eq!(m.sparse_pending_retired, 0);
        assert_eq!(m.sparse_reuse_hits, 0);
        assert_eq!(m.sparse_reuse_misses, 0);
        assert_eq!(m.queue_full_rejections, 0);
        assert_eq!(m.tick_disabled_rejections, 0);
        assert_eq!(m.rollback_events, 0);
        assert_eq!(m.tick_disabled_transitions, 0);
        assert_eq!(m.worker_stall_events, 0);
        assert_eq!(m.ring_not_available_events, 0);
        assert_eq!(m.ring_eviction_events, 0);
        assert_eq!(m.ring_stale_read_events, 0);
        assert_eq!(m.ring_skew_retry_events, 0);
    }
}
