//! PyStepMetrics: Python wrapper for step performance metrics.

use std::ffi::CStr;

use pyo3::prelude::*;
use pyo3::types::PyDict;

use murk_ffi::{murk_step_metrics_propagator, MurkStepMetrics};

/// Per-step performance metrics.
#[pyclass]
pub(crate) struct StepMetrics {
    pub(crate) total_us: u64,
    pub(crate) command_processing_us: u64,
    pub(crate) snapshot_publish_us: u64,
    pub(crate) memory_bytes: usize,
    pub(crate) propagator_us: Vec<(String, u64)>,
    pub(crate) sparse_retired_ranges: u32,
    pub(crate) sparse_pending_retired: u32,
    pub(crate) sparse_reuse_hits: u32,
    pub(crate) sparse_reuse_misses: u32,
    pub(crate) queue_full_rejections: u64,
    pub(crate) tick_disabled_rejections: u64,
    pub(crate) rollback_events: u64,
    pub(crate) tick_disabled_transitions: u64,
    pub(crate) worker_stall_events: u64,
    pub(crate) ring_not_available_events: u64,
}

#[pymethods]
impl StepMetrics {
    /// Wall-clock time for the entire tick, in microseconds.
    #[getter]
    fn total_us(&self) -> u64 {
        self.total_us
    }

    /// Time spent processing the ingress command queue, in microseconds.
    #[getter]
    fn command_processing_us(&self) -> u64 {
        self.command_processing_us
    }

    /// Time spent publishing the snapshot, in microseconds.
    #[getter]
    fn snapshot_publish_us(&self) -> u64 {
        self.snapshot_publish_us
    }

    /// Memory usage of the arena after the tick, in bytes.
    #[getter]
    fn memory_bytes(&self) -> usize {
        self.memory_bytes
    }

    /// Per-propagator timing: list of (name, microseconds) tuples.
    #[getter]
    fn propagator_us(&self) -> Vec<(String, u64)> {
        self.propagator_us.clone()
    }

    /// Number of sparse segment ranges available for reuse.
    #[getter]
    fn sparse_retired_ranges(&self) -> u32 {
        self.sparse_retired_ranges
    }

    /// Number of sparse segment ranges pending promotion (freed this tick).
    #[getter]
    fn sparse_pending_retired(&self) -> u32 {
        self.sparse_pending_retired
    }

    /// Number of sparse alloc() calls that reused a retired range this tick.
    #[getter]
    fn sparse_reuse_hits(&self) -> u32 {
        self.sparse_reuse_hits
    }

    /// Number of sparse alloc() calls that fell through to bump allocation this tick.
    #[getter]
    fn sparse_reuse_misses(&self) -> u32 {
        self.sparse_reuse_misses
    }

    /// Cumulative number of ingress rejections due to full queue.
    #[getter]
    fn queue_full_rejections(&self) -> u64 {
        self.queue_full_rejections
    }

    /// Cumulative number of ingress rejections due to tick-disabled state.
    #[getter]
    fn tick_disabled_rejections(&self) -> u64 {
        self.tick_disabled_rejections
    }

    /// Cumulative number of rollback events.
    #[getter]
    fn rollback_events(&self) -> u64 {
        self.rollback_events
    }

    /// Cumulative number of transitions into tick-disabled state.
    #[getter]
    fn tick_disabled_transitions(&self) -> u64 {
        self.tick_disabled_transitions
    }

    /// Cumulative number of worker stall force-unpin events.
    #[getter]
    fn worker_stall_events(&self) -> u64 {
        self.worker_stall_events
    }

    /// Cumulative number of ring "not available" events.
    #[getter]
    fn ring_not_available_events(&self) -> u64 {
        self.ring_not_available_events
    }

    /// Convert to a plain Python dict.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let d = PyDict::new(py);
        d.set_item("total_us", self.total_us)?;
        d.set_item("command_processing_us", self.command_processing_us)?;
        d.set_item("snapshot_publish_us", self.snapshot_publish_us)?;
        d.set_item("memory_bytes", self.memory_bytes)?;
        d.set_item("propagator_us", &self.propagator_us)?;
        d.set_item("sparse_retired_ranges", self.sparse_retired_ranges)?;
        d.set_item("sparse_pending_retired", self.sparse_pending_retired)?;
        d.set_item("sparse_reuse_hits", self.sparse_reuse_hits)?;
        d.set_item("sparse_reuse_misses", self.sparse_reuse_misses)?;
        d.set_item("queue_full_rejections", self.queue_full_rejections)?;
        d.set_item("tick_disabled_rejections", self.tick_disabled_rejections)?;
        d.set_item("rollback_events", self.rollback_events)?;
        d.set_item("tick_disabled_transitions", self.tick_disabled_transitions)?;
        d.set_item("worker_stall_events", self.worker_stall_events)?;
        d.set_item("ring_not_available_events", self.ring_not_available_events)?;
        Ok(d)
    }

    fn __repr__(&self) -> String {
        format!(
            "StepMetrics(total={}us, mem={}B, propagators={}, sparse_retired={}, sparse_pending={}, reuse_hits={}, reuse_misses={}, queue_full={}, tick_disabled_rejections={}, rollbacks={}, tick_disabled_transitions={}, worker_stalls={}, ring_not_available={})",
            self.total_us,
            self.memory_bytes,
            self.propagator_us.len(),
            self.sparse_retired_ranges,
            self.sparse_pending_retired,
            self.sparse_reuse_hits,
            self.sparse_reuse_misses,
            self.queue_full_rejections,
            self.tick_disabled_rejections,
            self.rollback_events,
            self.tick_disabled_transitions,
            self.worker_stall_events,
            self.ring_not_available_events,
        )
    }
}

impl StepMetrics {
    /// Build from FFI MurkStepMetrics + per-propagator queries.
    pub(crate) fn from_ffi(m: &MurkStepMetrics, world_handle: u64) -> Self {
        let mut propagator_us = Vec::with_capacity(m.n_propagators as usize);
        for i in 0..m.n_propagators {
            let mut name_buf = [0u8; 256];
            let mut us: u64 = 0;
            let rc = murk_step_metrics_propagator(
                world_handle,
                i,
                name_buf.as_mut_ptr() as *mut std::ffi::c_char,
                name_buf.len(),
                &mut us,
            );
            if rc == 0 {
                let name = CStr::from_bytes_until_nul(&name_buf)
                    .map(|c| c.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| "<unknown>".to_string());
                propagator_us.push((name, us));
            }
        }
        StepMetrics {
            total_us: m.total_us,
            command_processing_us: m.command_processing_us,
            snapshot_publish_us: m.snapshot_publish_us,
            memory_bytes: m.memory_bytes as usize,
            propagator_us,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_has_sparse_fields() {
        let m = StepMetrics {
            total_us: 0,
            command_processing_us: 0,
            snapshot_publish_us: 0,
            memory_bytes: 0,
            propagator_us: vec![],
            sparse_retired_ranges: 5,
            sparse_pending_retired: 3,
            sparse_reuse_hits: 10,
            sparse_reuse_misses: 4,
            queue_full_rejections: 11,
            tick_disabled_rejections: 4,
            rollback_events: 2,
            tick_disabled_transitions: 1,
            worker_stall_events: 3,
            ring_not_available_events: 7,
        };
        assert_eq!(m.sparse_retired_ranges, 5);
        assert_eq!(m.sparse_pending_retired, 3);
        assert_eq!(m.sparse_reuse_hits, 10);
        assert_eq!(m.sparse_reuse_misses, 4);
        assert_eq!(m.queue_full_rejections, 11);
        assert_eq!(m.tick_disabled_rejections, 4);
        assert_eq!(m.rollback_events, 2);
        assert_eq!(m.tick_disabled_transitions, 1);
        assert_eq!(m.worker_stall_events, 3);
        assert_eq!(m.ring_not_available_events, 7);
    }
}
