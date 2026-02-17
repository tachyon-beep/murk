//! Tick loop, command channel draining, stalled-worker detection, and
//! adaptive backoff for RealtimeAsync mode.
//!
//! The tick thread owns [`TickEngine`] exclusively (moved in via
//! `thread::spawn`). No locks on the hot path — commands arrive via
//! a bounded crossbeam channel and replies go back via per-batch
//! oneshot channels.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossbeam_channel::Receiver;

use crate::config::BackoffConfig;
use crate::epoch::{EpochCounter, WorkerEpoch};
use crate::ring::SnapshotRing;
use crate::tick::TickEngine;

use murk_core::command::{Command, Receipt};

/// A batch of commands submitted by a user thread, paired with a reply
/// channel for the resulting receipts.
pub(crate) struct IngressBatch {
    pub commands: Vec<Command>,
    pub reply: crossbeam_channel::Sender<Vec<Receipt>>,
}

/// Adaptive backoff state machine for stalled-worker detection.
///
/// When workers hold epoch pins too long (blocking ring reclamation),
/// the tick thread increases the effective max-skew tolerance. This
/// avoids false positives during legitimate long observations while
/// still detecting true stalls.
pub(crate) struct AdaptiveBackoff {
    effective_max_skew: u64,
    config: BackoffConfig,
    ticks_since_last_rejection: u64,
    rejection_window: RejectionWindow,
}

/// Sliding window for tracking force-unpin rejections.
struct RejectionWindow {
    /// Ring buffer of booleans: true = rejection occurred on this tick.
    window: Vec<bool>,
    /// Current write position in the ring buffer.
    pos: usize,
    /// Number of rejections in the current window.
    count: usize,
}

impl RejectionWindow {
    fn new(size: usize) -> Self {
        Self {
            window: vec![false; size],
            pos: 0,
            count: 0,
        }
    }

    fn push(&mut self, rejected: bool) {
        let old = self.window[self.pos];
        if old {
            self.count -= 1;
        }
        self.window[self.pos] = rejected;
        if rejected {
            self.count += 1;
        }
        self.pos = (self.pos + 1) % self.window.len();
    }

    fn rate(&self) -> f64 {
        self.count as f64 / self.window.len() as f64
    }
}

impl AdaptiveBackoff {
    pub fn new(config: &BackoffConfig) -> Self {
        let window_size = config.decay_rate.max(1) as usize;
        Self {
            effective_max_skew: config.initial_max_skew,
            config: config.clone(),
            ticks_since_last_rejection: 0,
            rejection_window: RejectionWindow::new(window_size),
        }
    }

    /// Record whether a force-unpin (rejection) happened this tick.
    /// Returns the current effective max skew.
    pub fn record_tick(&mut self, had_rejection: bool) -> u64 {
        self.rejection_window.push(had_rejection);

        if had_rejection {
            self.ticks_since_last_rejection = 0;
            // Backoff: increase effective_max_skew.
            let next = (self.effective_max_skew as f64 * self.config.backoff_factor) as u64;
            self.effective_max_skew = next.min(self.config.max_skew_cap);
        } else {
            self.ticks_since_last_rejection += 1;
            // Decay: if no rejections for `decay_rate` ticks, reset.
            if self.ticks_since_last_rejection >= self.config.decay_rate {
                self.effective_max_skew = self.config.initial_max_skew;
            }
        }

        // Proactive backoff: if rejection rate exceeds threshold, bump.
        if self.rejection_window.rate() > self.config.rejection_rate_threshold {
            let next = (self.effective_max_skew as f64 * self.config.backoff_factor) as u64;
            self.effective_max_skew = next.min(self.config.max_skew_cap);
        }

        self.effective_max_skew
    }

    /// Current effective max skew tolerance.
    #[cfg(test)]
    pub fn effective_max_skew(&self) -> u64 {
        self.effective_max_skew
    }
}

/// State held by the tick thread's main loop.
pub(crate) struct TickThreadState {
    engine: TickEngine,
    ring: Arc<SnapshotRing>,
    epoch_counter: Arc<EpochCounter>,
    worker_epochs: Arc<[WorkerEpoch]>,
    cmd_rx: Receiver<IngressBatch>,
    shutdown_flag: Arc<AtomicBool>,
    tick_stopped: Arc<AtomicBool>,
    tick_budget: Duration,
    max_epoch_hold_ns: u64,
    cancel_grace_ns: u64,
    backoff: AdaptiveBackoff,
}

impl TickThreadState {
    /// Create a new tick thread state.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        engine: TickEngine,
        ring: Arc<SnapshotRing>,
        epoch_counter: Arc<EpochCounter>,
        worker_epochs: Arc<[WorkerEpoch]>,
        cmd_rx: Receiver<IngressBatch>,
        shutdown_flag: Arc<AtomicBool>,
        tick_stopped: Arc<AtomicBool>,
        tick_rate_hz: f64,
        max_epoch_hold_ms: u64,
        cancel_grace_ms: u64,
        backoff_config: &BackoffConfig,
    ) -> Self {
        Self {
            engine,
            ring,
            epoch_counter,
            worker_epochs,
            cmd_rx,
            shutdown_flag,
            tick_stopped,
            tick_budget: Duration::from_secs_f64(1.0 / tick_rate_hz),
            max_epoch_hold_ns: max_epoch_hold_ms * 1_000_000,
            cancel_grace_ns: cancel_grace_ms * 1_000_000,
            backoff: AdaptiveBackoff::new(backoff_config),
        }
    }

    /// Main tick loop. Runs until `shutdown_flag` is set.
    ///
    /// Consumes self and returns the `TickEngine` so that the caller
    /// can recover it for `reset()` via `JoinHandle<TickEngine>`.
    pub fn run(mut self) -> TickEngine {
        loop {
            if self.shutdown_flag.load(Ordering::Acquire) {
                break;
            }

            // If tick is disabled (consecutive rollbacks), idle until shutdown.
            if self.engine.is_tick_disabled() {
                self.idle_until_shutdown();
                break;
            }

            let tick_start = Instant::now();

            // 1. Drain command channel.
            self.drain_command_channel();

            // 2. Execute tick.
            match self.engine.execute_tick() {
                Ok(result) => {
                    // 3. Push snapshot to ring.
                    let snap = self.engine.owned_snapshot();
                    self.ring.push(snap);

                    // 4. Advance global epoch.
                    self.epoch_counter.advance();

                    // 5. Check for stalled workers.
                    let had_rejection = self.check_stalled_workers();
                    self.backoff.record_tick(had_rejection);

                    drop(result);
                }
                Err(_tick_err) => {
                    // Tick failed (propagator error). The engine tracks consecutive
                    // rollbacks internally. If tick becomes disabled, next iteration
                    // enters idle_until_shutdown.
                }
            }

            // 6. Sleep for remaining budget.
            let elapsed = tick_start.elapsed();
            if let Some(remaining) = self.tick_budget.checked_sub(elapsed) {
                std::thread::sleep(remaining);
            }
        }

        // Signal that the tick thread has stopped.
        self.tick_stopped.store(true, Ordering::Release);
        self.engine
    }

    /// Drain all pending command batches from the channel.
    fn drain_command_channel(&mut self) {
        while let Ok(batch) = self.cmd_rx.try_recv() {
            let receipts = self.engine.submit_commands(batch.commands);
            // Best-effort reply — caller may have dropped their receiver.
            let _ = batch.reply.send(receipts);
        }
    }

    /// Check for stalled workers and force-unpin them.
    /// Returns `true` if any worker was force-unpinned.
    fn check_stalled_workers(&self) -> bool {
        let now_ns = crate::epoch::monotonic_nanos();
        let mut had_rejection = false;

        for worker in self.worker_epochs.iter() {
            // Use pin_snapshot() for a consistent (epoch, pin_start_ns) read.
            // This avoids a TOCTOU race where a concurrent unpin/repin could
            // produce a mismatched pair and trigger false-positive stall detection.
            let pin_start = match worker.pin_snapshot() {
                Some((_epoch, start_ns)) => start_ns,
                None => continue, // not pinned
            };
            let hold_ns = now_ns.saturating_sub(pin_start);

            if hold_ns > self.max_epoch_hold_ns {
                // First: request cooperative cancellation.
                worker.request_cancel();

                // If already past the grace period, force unpin.
                if hold_ns > self.max_epoch_hold_ns + self.cancel_grace_ns {
                    worker.unpin();
                    worker.clear_cancel();
                    had_rejection = true;
                }
            }
        }

        had_rejection
    }

    /// Spin on the command channel and shutdown flag when tick is disabled.
    fn idle_until_shutdown(&mut self) {
        loop {
            if self.shutdown_flag.load(Ordering::Acquire) {
                break;
            }
            // Still drain commands so callers don't block forever.
            // All commands will be rejected by IngressQueue (tick_disabled=true).
            self.drain_command_channel();
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── AdaptiveBackoff tests ────────────────────────────────────

    #[test]
    fn backoff_starts_at_initial() {
        let config = BackoffConfig::default();
        let backoff = AdaptiveBackoff::new(&config);
        assert_eq!(backoff.effective_max_skew(), config.initial_max_skew);
    }

    #[test]
    fn backoff_increases_on_rejection() {
        let config = BackoffConfig {
            initial_max_skew: 2,
            backoff_factor: 1.5,
            max_skew_cap: 10,
            decay_rate: 60,
            rejection_rate_threshold: 0.20,
        };
        let mut backoff = AdaptiveBackoff::new(&config);
        let skew = backoff.record_tick(true);
        assert!(skew > 2, "skew should increase after rejection");
    }

    #[test]
    fn backoff_caps_at_max() {
        let config = BackoffConfig {
            initial_max_skew: 2,
            backoff_factor: 10.0,
            max_skew_cap: 5,
            decay_rate: 60,
            rejection_rate_threshold: 0.20,
        };
        let mut backoff = AdaptiveBackoff::new(&config);
        // Many rejections should cap at 5.
        for _ in 0..20 {
            backoff.record_tick(true);
        }
        assert_eq!(backoff.effective_max_skew(), 5);
    }

    #[test]
    fn backoff_decays_after_no_rejections() {
        let config = BackoffConfig {
            initial_max_skew: 2,
            backoff_factor: 2.0,
            max_skew_cap: 100,
            decay_rate: 10,
            rejection_rate_threshold: 0.20,
        };
        let mut backoff = AdaptiveBackoff::new(&config);

        // Cause backoff.
        backoff.record_tick(true);
        let after_rejection = backoff.effective_max_skew();
        assert!(after_rejection > 2);

        // 10 ticks with no rejections should decay.
        for _ in 0..10 {
            backoff.record_tick(false);
        }
        assert_eq!(backoff.effective_max_skew(), 2);
    }

    #[test]
    fn rejection_window_tracks_rate() {
        let mut window = RejectionWindow::new(10);
        // 3 out of 10 = 0.3
        for i in 0..10 {
            window.push(i < 3);
        }
        assert!((window.rate() - 0.3).abs() < 1e-10);

        // Push 10 non-rejections: rate drops to 0.
        for _ in 0..10 {
            window.push(false);
        }
        assert!((window.rate()).abs() < 1e-10);
    }
}
