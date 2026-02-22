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
            sparse_reuse_hits: 5,
            sparse_reuse_misses: 2,
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
        assert_eq!(m.total_us, 100);
        assert_eq!(m.command_processing_us, 20);
        assert_eq!(m.propagator_us.len(), 2);
        assert_eq!(m.propagator_us[0].0, "diffusion");
        assert_eq!(m.propagator_us[0].1, 50);
        assert_eq!(m.snapshot_publish_us, 10);
        assert_eq!(m.memory_bytes, 4096);
        assert_eq!(m.sparse_retired_ranges, 3);
        assert_eq!(m.sparse_pending_retired, 1);
        assert_eq!(m.sparse_reuse_hits, 5);
        assert_eq!(m.sparse_reuse_misses, 2);
        assert_eq!(m.queue_full_rejections, 11);
        assert_eq!(m.tick_disabled_rejections, 4);
        assert_eq!(m.rollback_events, 2);
        assert_eq!(m.tick_disabled_transitions, 1);
        assert_eq!(m.worker_stall_events, 3);
        assert_eq!(m.ring_not_available_events, 7);
        assert_eq!(m.ring_eviction_events, 9);
        assert_eq!(m.ring_stale_read_events, 4);
        assert_eq!(m.ring_skew_retry_events, 2);
    }
}
