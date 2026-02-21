//! Per-tick performance metrics for the simulation engine.
//!
//! [`StepMetrics`] captures timing and memory data for a single tick,
//! enabling telemetry, profiling, and adaptive backoff decisions.

/// Timing and memory metrics collected during a single tick.
///
/// All durations are in microseconds. The engine populates these fields
/// after each `step()` call; consumers (telemetry, backoff logic) read
/// them from the most recent tick.
#[derive(Clone, Debug, Default)]
pub struct StepMetrics {
    /// Wall-clock time for the entire tick, in microseconds.
    pub total_us: u64,
    /// Time spent processing the ingress command queue, in microseconds.
    pub command_processing_us: u64,
    /// Per-propagator execution times: `(name, microseconds)`.
    pub propagator_us: Vec<(String, u64)>,
    /// Time spent publishing the snapshot to the ring buffer, in microseconds.
    pub snapshot_publish_us: u64,
    /// Memory usage of the arena after the tick, in bytes.
    pub memory_bytes: usize,
    /// Number of sparse segment ranges available for reuse.
    pub sparse_retired_ranges: u32,
    /// Number of sparse segment ranges pending promotion (freed this tick).
    pub sparse_pending_retired: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_metrics_are_zero() {
        let m = StepMetrics::default();
        assert_eq!(m.total_us, 0);
        assert_eq!(m.command_processing_us, 0);
        assert!(m.propagator_us.is_empty());
        assert_eq!(m.snapshot_publish_us, 0);
        assert_eq!(m.memory_bytes, 0);
        assert_eq!(m.sparse_retired_ranges, 0);
        assert_eq!(m.sparse_pending_retired, 0);
    }

    #[test]
    fn metrics_fields_accessible() {
        let m = StepMetrics {
            total_us: 100,
            command_processing_us: 20,
            propagator_us: vec![("diffusion".to_string(), 50), ("decay".to_string(), 30)],
            snapshot_publish_us: 10,
            memory_bytes: 4096,
            sparse_retired_ranges: 3,
            sparse_pending_retired: 1,
        };
        assert_eq!(m.total_us, 100);
        assert_eq!(m.command_processing_us, 20);
        assert_eq!(m.propagator_us.len(), 2);
        assert_eq!(m.propagator_us[0].0, "diffusion");
        assert_eq!(m.propagator_us[0].1, 50);
        assert_eq!(m.snapshot_publish_us, 10);
        assert_eq!(m.memory_bytes, 4096);
        assert_eq!(m.sparse_retired_ranges, 3);
        assert_eq!(m.sparse_pending_retired, 1);
    }
}
