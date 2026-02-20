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

    /// Convert to a plain Python dict.
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let d = PyDict::new(py);
        d.set_item("total_us", self.total_us)?;
        d.set_item("command_processing_us", self.command_processing_us)?;
        d.set_item("snapshot_publish_us", self.snapshot_publish_us)?;
        d.set_item("memory_bytes", self.memory_bytes)?;
        d.set_item("propagator_us", &self.propagator_us)?;
        Ok(d)
    }

    fn __repr__(&self) -> String {
        format!(
            "StepMetrics(total={}us, mem={}B, propagators={})",
            self.total_us,
            self.memory_bytes,
            self.propagator_us.len()
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
            memory_bytes: m.memory_bytes,
            propagator_us,
        }
    }
}
