//! User-facing `RealtimeAsyncWorld` API and shutdown state machine.
//!
//! This is the primary mode for RL training: the tick engine runs on a
//! dedicated background thread at a configurable rate (default 60 Hz),
//! while an egress thread pool serves observation requests concurrently.
//!
//! # Architecture
//!
//! ```text
//! User Thread(s)              Tick Thread              Egress Workers (N)
//!     |                           |                         |
//!     |--submit_commands()------->| cmd_rx.try_recv()       |
//!     |   [cmd_tx: bounded(64)]   | engine.submit_commands()|
//!     |<--receipts via reply_tx---| engine.execute_tick()   |
//!     |                           | ring.push(snap)         |
//!     |                           | epoch_counter.advance() |
//!     |                           | check_stalled_workers() |
//!     |                           | sleep(budget - elapsed) |
//!     |                           |                         |
//!     |--observe()------------------------------------->    |
//!     |   [obs_tx: bounded(N*4)]               task_rx.recv()
//!     |   blocks on reply_rx                   ring.latest()
//!     |                                        pin epoch
//!     |                                        execute ObsPlan
//!     |                                        unpin epoch
//!     |<--result via reply_tx--------------------------    |
//! ```

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use murk_arena::OwnedSnapshot;
use murk_core::command::{Command, Receipt};
use murk_core::error::ObsError;
use murk_core::Coord;
use murk_obs::{ObsMetadata, ObsPlan};
use murk_space::Space;

use crate::config::{AsyncConfig, BackoffConfig, ConfigError, WorldConfig};
use crate::egress::{ObsResult, ObsTask};
use crate::epoch::{EpochCounter, WorkerEpoch};
use crate::ring::SnapshotRing;
use crate::tick::TickEngine;
use crate::tick_thread::{IngressBatch, TickThreadState};

// ── Error types ──────────────────────────────────────────────────

/// Error submitting commands to the tick thread.
#[derive(Debug, PartialEq, Eq)]
pub enum SubmitError {
    /// The tick thread has shut down.
    Shutdown,
    /// The command channel is full (back-pressure).
    ChannelFull,
}

impl std::fmt::Display for SubmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shutdown => write!(f, "tick thread has shut down"),
            Self::ChannelFull => write!(f, "command channel full"),
        }
    }
}

impl std::error::Error for SubmitError {}

// ── ShutdownReport ───────────────────────────────────────────────

/// Report from the shutdown state machine.
#[derive(Debug)]
pub struct ShutdownReport {
    /// Total time spent in the shutdown sequence.
    pub total_ms: u64,
    /// Time spent draining the tick thread.
    pub drain_ms: u64,
    /// Time spent quiescing workers.
    pub quiesce_ms: u64,
    /// Whether the tick thread was joined successfully.
    pub tick_joined: bool,
    /// Number of worker threads joined.
    pub workers_joined: usize,
}

// ── ShutdownState ────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShutdownState {
    Running,
    Draining,
    Quiescing,
    Dropped,
}

// ── RealtimeAsyncWorld ───────────────────────────────────────────

/// Realtime asynchronous simulation world.
///
/// Runs the tick engine on a background thread and serves observations
/// from a pool of egress workers. This is the primary API for RL
/// training environments.
pub struct RealtimeAsyncWorld {
    ring: Arc<SnapshotRing>,
    epoch_counter: Arc<EpochCounter>,
    worker_epochs: Arc<[WorkerEpoch]>,
    cmd_tx: Option<crossbeam_channel::Sender<IngressBatch>>,
    obs_tx: Option<crossbeam_channel::Sender<ObsTask>>,
    shutdown_flag: Arc<AtomicBool>,
    tick_stopped: Arc<AtomicBool>,
    tick_thread: Option<JoinHandle<TickEngine>>,
    worker_threads: Vec<JoinHandle<()>>,
    state: ShutdownState,
    /// Recovered from tick thread on shutdown, used for `reset()`.
    /// Wrapped in Mutex so RealtimeAsyncWorld is Sync (TickEngine
    /// contains `Vec<Box<dyn Propagator>>` which is Send but not Sync).
    /// Never contended: only accessed during reset() which takes &mut self.
    recovered_engine: Mutex<Option<TickEngine>>,
    config: AsyncConfig,
    backoff_config: BackoffConfig,
    seed: u64,
    tick_rate_hz: f64,
    /// Shared space for agent-relative observations and engine reconstruction.
    space: Arc<dyn Space>,
}

impl RealtimeAsyncWorld {
    /// Create a new realtime async world and spawn all threads.
    ///
    /// The `WorldConfig` is consumed: the `TickEngine` is moved into
    /// the tick thread. The `space` is shared via `Arc` for egress
    /// workers that need it for agent-relative observations.
    pub fn new(config: WorldConfig, async_config: AsyncConfig) -> Result<Self, ConfigError> {
        let tick_rate_hz = config.tick_rate_hz.unwrap_or(60.0);
        if !tick_rate_hz.is_finite() || tick_rate_hz <= 0.0 {
            return Err(ConfigError::InvalidTickRate {
                value: tick_rate_hz,
            });
        }

        let seed = config.seed;
        let ring_size = config.ring_buffer_size;
        let backoff_config = config.backoff.clone();
        let max_epoch_hold_ms = async_config.max_epoch_hold_ms;
        let cancel_grace_ms = async_config.cancel_grace_ms;

        // Share the space for agent-relative observations.
        let space: Arc<dyn Space> = Arc::from(config.space);

        // Reconstruct WorldConfig with the Arc'd space (Box from Arc).
        // We clone the space Arc for our own reference, and give
        // TickEngine a Box wrapper.
        let engine_space: Box<dyn Space> = Box::new(ArcSpaceWrapper(Arc::clone(&space)));
        let engine_config = WorldConfig {
            space: engine_space,
            fields: config.fields,
            propagators: config.propagators,
            dt: config.dt,
            seed: config.seed,
            ring_buffer_size: config.ring_buffer_size,
            max_ingress_queue: config.max_ingress_queue,
            tick_rate_hz: config.tick_rate_hz,
            backoff: backoff_config.clone(),
        };

        let engine = TickEngine::new(engine_config)?;

        let worker_count = async_config.resolved_worker_count();
        let ring = Arc::new(SnapshotRing::new(ring_size));
        let epoch_counter = Arc::new(EpochCounter::new());
        let worker_epochs: Arc<[WorkerEpoch]> = (0..worker_count as u32)
            .map(WorkerEpoch::new)
            .collect::<Vec<_>>()
            .into();

        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let tick_stopped = Arc::new(AtomicBool::new(false));

        // Command channel: bounded(64) — tick thread drains each tick.
        let (cmd_tx, cmd_rx) = crossbeam_channel::bounded(64);

        // Task channel for egress workers: bounded(worker_count * 4).
        let (obs_tx, obs_rx) = crossbeam_channel::bounded(worker_count * 4);

        // Spawn tick thread — returns TickEngine on exit for reset().
        let tick_ring = Arc::clone(&ring);
        let tick_epoch = Arc::clone(&epoch_counter);
        let tick_workers = Arc::clone(&worker_epochs);
        let tick_shutdown = Arc::clone(&shutdown_flag);
        let tick_stopped_flag = Arc::clone(&tick_stopped);
        let stored_backoff = backoff_config.clone();
        let tick_thread = thread::Builder::new()
            .name("murk-tick".into())
            .spawn(move || {
                let state = TickThreadState::new(
                    engine,
                    tick_ring,
                    tick_epoch,
                    tick_workers,
                    cmd_rx,
                    tick_shutdown,
                    tick_stopped_flag,
                    tick_rate_hz,
                    max_epoch_hold_ms,
                    cancel_grace_ms,
                    &backoff_config,
                );
                state.run()
            })
            .expect("failed to spawn tick thread");

        // Spawn egress worker threads.
        let worker_threads = Self::spawn_egress_workers(
            worker_count,
            &obs_rx,
            &ring,
            &epoch_counter,
            &worker_epochs,
        );

        Ok(Self {
            ring,
            epoch_counter,
            worker_epochs,
            cmd_tx: Some(cmd_tx),
            obs_tx: Some(obs_tx),
            shutdown_flag,
            tick_stopped,
            tick_thread: Some(tick_thread),
            worker_threads,
            state: ShutdownState::Running,
            recovered_engine: Mutex::new(None),
            config: async_config,
            backoff_config: stored_backoff,
            seed,
            tick_rate_hz,
            space,
        })
    }

    /// Submit commands to be processed in the next tick.
    ///
    /// Non-blocking: sends the batch via channel and blocks only for
    /// the receipt reply (which arrives within one tick period).
    pub fn submit_commands(&self, commands: Vec<Command>) -> Result<Vec<Receipt>, SubmitError> {
        let cmd_tx = self.cmd_tx.as_ref().ok_or(SubmitError::Shutdown)?;

        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
        let batch = IngressBatch {
            commands,
            reply: reply_tx,
        };

        cmd_tx.try_send(batch).map_err(|e| match e {
            crossbeam_channel::TrySendError::Full(_) => SubmitError::ChannelFull,
            crossbeam_channel::TrySendError::Disconnected(_) => SubmitError::Shutdown,
        })?;

        // Wait for receipts (blocks until tick thread processes the batch).
        reply_rx.recv().map_err(|_| SubmitError::Shutdown)
    }

    /// Extract an observation from the latest snapshot.
    ///
    /// Blocking: dispatches to an egress worker and waits for the result.
    /// The output and mask buffers must be pre-allocated.
    pub fn observe(
        &self,
        plan: &Arc<ObsPlan>,
        output: &mut [f32],
        mask: &mut [u8],
    ) -> Result<ObsMetadata, ObsError> {
        let obs_tx = self
            .obs_tx
            .as_ref()
            .ok_or_else(|| ObsError::ExecutionFailed {
                reason: "world is shut down".into(),
            })?;

        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
        let task = ObsTask::Simple {
            plan: Arc::clone(plan),
            output_len: output.len(),
            mask_len: mask.len(),
            reply: reply_tx,
        };

        obs_tx.send(task).map_err(|_| ObsError::ExecutionFailed {
            reason: "egress pool shut down".into(),
        })?;

        let result = reply_rx.recv().map_err(|_| ObsError::ExecutionFailed {
            reason: "worker disconnected".into(),
        })?;

        match result {
            ObsResult::Simple {
                metadata,
                output: buf,
                mask: mbuf,
            } => {
                if buf.len() > output.len() || mbuf.len() > mask.len() {
                    return Err(ObsError::ExecutionFailed {
                        reason: format!(
                            "output buffer too small: need ({}, {}) got ({}, {})",
                            buf.len(),
                            mbuf.len(),
                            output.len(),
                            mask.len()
                        ),
                    });
                }
                output[..buf.len()].copy_from_slice(&buf);
                mask[..mbuf.len()].copy_from_slice(&mbuf);
                Ok(metadata)
            }
            ObsResult::Error(e) => Err(e),
            _ => Err(ObsError::ExecutionFailed {
                reason: "unexpected result type".into(),
            }),
        }
    }

    /// Extract agent-relative observations from the latest snapshot.
    ///
    /// Each agent gets `output_len / n_agents` elements in the output buffer.
    pub fn observe_agents(
        &self,
        plan: &Arc<ObsPlan>,
        space: &Arc<dyn Space>,
        agent_centers: &[Coord],
        output: &mut [f32],
        mask: &mut [u8],
    ) -> Result<Vec<ObsMetadata>, ObsError> {
        let obs_tx = self
            .obs_tx
            .as_ref()
            .ok_or_else(|| ObsError::ExecutionFailed {
                reason: "world is shut down".into(),
            })?;

        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
        let n_agents = agent_centers.len();
        let per_agent_output = if n_agents > 0 {
            output.len() / n_agents
        } else {
            0
        };
        let per_agent_mask = if n_agents > 0 {
            mask.len() / n_agents
        } else {
            0
        };

        let task = ObsTask::Agents {
            plan: Arc::clone(plan),
            space: Arc::clone(space),
            agent_centers: agent_centers.to_vec(),
            output_len: per_agent_output,
            mask_len: per_agent_mask,
            reply: reply_tx,
        };

        obs_tx.send(task).map_err(|_| ObsError::ExecutionFailed {
            reason: "egress pool shut down".into(),
        })?;

        let result = reply_rx.recv().map_err(|_| ObsError::ExecutionFailed {
            reason: "worker disconnected".into(),
        })?;

        match result {
            ObsResult::Agents {
                metadata,
                output: buf,
                mask: mbuf,
            } => {
                if buf.len() > output.len() || mbuf.len() > mask.len() {
                    return Err(ObsError::ExecutionFailed {
                        reason: format!(
                            "output buffer too small: need ({}, {}) got ({}, {})",
                            buf.len(),
                            mbuf.len(),
                            output.len(),
                            mask.len()
                        ),
                    });
                }
                output[..buf.len()].copy_from_slice(&buf);
                mask[..mbuf.len()].copy_from_slice(&mbuf);
                Ok(metadata)
            }
            ObsResult::Error(e) => Err(e),
            _ => Err(ObsError::ExecutionFailed {
                reason: "unexpected result type".into(),
            }),
        }
    }

    /// Spawn egress worker threads (shared between `new` and `reset`).
    fn spawn_egress_workers(
        worker_count: usize,
        obs_rx: &crossbeam_channel::Receiver<ObsTask>,
        ring: &Arc<SnapshotRing>,
        epoch_counter: &Arc<EpochCounter>,
        worker_epochs: &Arc<[WorkerEpoch]>,
    ) -> Vec<JoinHandle<()>> {
        let mut worker_threads = Vec::with_capacity(worker_count);
        for i in 0..worker_count {
            let obs_rx = obs_rx.clone();
            let ring = Arc::clone(ring);
            let epoch = Arc::clone(epoch_counter);
            let worker_epochs_ref = Arc::clone(worker_epochs);
            let handle = thread::Builder::new()
                .name(format!("murk-egress-{i}"))
                .spawn(move || {
                    crate::egress::worker_loop_indexed(obs_rx, ring, epoch, worker_epochs_ref, i);
                })
                .expect("failed to spawn egress worker");
            worker_threads.push(handle);
        }
        worker_threads
    }

    /// Get the latest snapshot directly from the ring.
    pub fn latest_snapshot(&self) -> Option<Arc<OwnedSnapshot>> {
        self.ring.latest()
    }

    /// Current epoch (lock-free read).
    pub fn current_epoch(&self) -> u64 {
        self.epoch_counter.current()
    }

    /// Shutdown the world with the 4-state machine.
    ///
    /// 1. **Running → Draining (≤33ms):** Set shutdown flag, unpark tick
    ///    thread (wakes it from budget sleep immediately), wait for tick stop.
    /// 2. **Draining → Quiescing (≤200ms):** Cancel workers, drop obs channel.
    /// 3. **Quiescing → Dropped (≤10ms):** Join all threads.
    pub fn shutdown(&mut self) -> ShutdownReport {
        if self.state == ShutdownState::Dropped {
            return ShutdownReport {
                total_ms: 0,
                drain_ms: 0,
                quiesce_ms: 0,
                tick_joined: true,
                workers_joined: 0,
            };
        }

        let start = Instant::now();

        // Phase 1: Running → Draining
        self.state = ShutdownState::Draining;
        self.shutdown_flag.store(true, Ordering::Release);

        // Wake the tick thread if it's parked in a budget sleep.
        // park_timeout is used instead of thread::sleep, so unpark()
        // provides immediate wakeup regardless of tick_rate_hz.
        if let Some(handle) = &self.tick_thread {
            handle.thread().unpark();
        }

        // Wait for tick thread to acknowledge (≤33ms budget).
        let drain_deadline = Instant::now() + Duration::from_millis(33);
        while !self.tick_stopped.load(Ordering::Acquire) {
            if Instant::now() > drain_deadline {
                break;
            }
            thread::yield_now();
        }
        let drain_ms = start.elapsed().as_millis() as u64;

        // Phase 2: Draining → Quiescing
        self.state = ShutdownState::Quiescing;

        // Cancel all workers.
        for w in self.worker_epochs.iter() {
            w.request_cancel();
        }

        // Drop the command and observation channels to unblock workers.
        self.cmd_tx.take();
        self.obs_tx.take();

        // Wait for all workers to unpin (≤200ms budget).
        let quiesce_deadline = Instant::now() + Duration::from_millis(200);
        loop {
            let all_unpinned = self.worker_epochs.iter().all(|w| !w.is_pinned());
            if all_unpinned || Instant::now() > quiesce_deadline {
                break;
            }
            thread::yield_now();
        }
        let quiesce_ms = start.elapsed().as_millis() as u64 - drain_ms;

        // Phase 3: Quiescing → Dropped
        self.state = ShutdownState::Dropped;

        let tick_joined = if let Some(handle) = self.tick_thread.take() {
            match handle.join() {
                Ok(engine) => {
                    *self.recovered_engine.lock().unwrap() = Some(engine);
                    true
                }
                Err(_) => false,
            }
        } else {
            true
        };

        let mut workers_joined = 0;
        for handle in self.worker_threads.drain(..) {
            if handle.join().is_ok() {
                workers_joined += 1;
            }
        }

        let total_ms = start.elapsed().as_millis() as u64;
        ShutdownReport {
            total_ms,
            drain_ms,
            quiesce_ms,
            tick_joined,
            workers_joined,
        }
    }

    /// Reset the world: stop all threads, reset engine state, restart.
    ///
    /// This is the RL episode-boundary operation. The engine is recovered
    /// from the tick thread, reset in-place, then respawned with fresh
    /// channels and worker threads.
    pub fn reset(&mut self, seed: u64) -> Result<(), ConfigError> {
        // Shutdown if still running (recovers engine via JoinHandle).
        if self.state != ShutdownState::Dropped {
            self.shutdown();
        }

        self.seed = seed;

        // Recover the engine from the previous tick thread.
        let mut engine = self
            .recovered_engine
            .lock()
            .unwrap()
            .take()
            .ok_or(ConfigError::EngineRecoveryFailed)?;

        // Reset the engine (clears arena, ingress, tick counter).
        // If reset fails, restore the engine so a subsequent reset can retry.
        if let Err(e) = engine.reset() {
            *self.recovered_engine.lock().unwrap() = Some(engine);
            return Err(e);
        }

        // Fresh shared state.
        let worker_count = self.config.resolved_worker_count();
        self.ring = Arc::new(SnapshotRing::new(self.ring.capacity()));
        self.epoch_counter = Arc::new(EpochCounter::new());
        self.worker_epochs = (0..worker_count as u32)
            .map(WorkerEpoch::new)
            .collect::<Vec<_>>()
            .into();
        self.shutdown_flag = Arc::new(AtomicBool::new(false));
        self.tick_stopped = Arc::new(AtomicBool::new(false));

        // Fresh channels.
        let (cmd_tx, cmd_rx) = crossbeam_channel::bounded(64);
        let (obs_tx, obs_rx) = crossbeam_channel::bounded(worker_count * 4);
        self.cmd_tx = Some(cmd_tx);
        self.obs_tx = Some(obs_tx);

        // Respawn tick thread.
        let tick_ring = Arc::clone(&self.ring);
        let tick_epoch = Arc::clone(&self.epoch_counter);
        let tick_workers = Arc::clone(&self.worker_epochs);
        let tick_shutdown = Arc::clone(&self.shutdown_flag);
        let tick_stopped_flag = Arc::clone(&self.tick_stopped);
        let tick_rate_hz = self.tick_rate_hz;
        let max_epoch_hold_ms = self.config.max_epoch_hold_ms;
        let cancel_grace_ms = self.config.cancel_grace_ms;
        let backoff_config = self.backoff_config.clone();
        self.tick_thread = Some(
            thread::Builder::new()
                .name("murk-tick".into())
                .spawn(move || {
                    let state = TickThreadState::new(
                        engine,
                        tick_ring,
                        tick_epoch,
                        tick_workers,
                        cmd_rx,
                        tick_shutdown,
                        tick_stopped_flag,
                        tick_rate_hz,
                        max_epoch_hold_ms,
                        cancel_grace_ms,
                        &backoff_config,
                    );
                    state.run()
                })
                .expect("failed to spawn tick thread"),
        );

        // Respawn egress workers.
        self.worker_threads = Self::spawn_egress_workers(
            worker_count,
            &obs_rx,
            &self.ring,
            &self.epoch_counter,
            &self.worker_epochs,
        );

        self.state = ShutdownState::Running;
        Ok(())
    }

    /// The shared space used for agent-relative observations.
    pub fn space(&self) -> &dyn Space {
        self.space.as_ref()
    }
}

impl Drop for RealtimeAsyncWorld {
    fn drop(&mut self) {
        if self.state != ShutdownState::Dropped {
            self.shutdown();
        }
    }
}

// ── ArcSpaceWrapper ──────────────────────────────────────────────

/// Wrapper that implements `Space` by delegating to an `Arc<dyn Space>`.
///
/// This allows `TickEngine` to take a `Box<dyn Space>` while the
/// `RealtimeAsyncWorld` retains a shared `Arc` reference for egress.
struct ArcSpaceWrapper(Arc<dyn Space>);

impl murk_space::Space for ArcSpaceWrapper {
    fn ndim(&self) -> usize {
        self.0.ndim()
    }

    fn cell_count(&self) -> usize {
        self.0.cell_count()
    }

    fn neighbours(&self, coord: &Coord) -> smallvec::SmallVec<[Coord; 8]> {
        self.0.neighbours(coord)
    }

    fn distance(&self, a: &Coord, b: &Coord) -> f64 {
        self.0.distance(a, b)
    }

    fn compile_region(
        &self,
        spec: &murk_space::RegionSpec,
    ) -> Result<murk_space::RegionPlan, murk_space::error::SpaceError> {
        self.0.compile_region(spec)
    }

    fn canonical_ordering(&self) -> Vec<Coord> {
        self.0.canonical_ordering()
    }

    fn canonical_rank(&self, coord: &Coord) -> Option<usize> {
        self.0.canonical_rank(coord)
    }

    fn instance_id(&self) -> murk_core::SpaceInstanceId {
        self.0.instance_id()
    }

    fn topology_eq(&self, other: &dyn murk_space::Space) -> bool {
        // Unwrap ArcSpaceWrapper so the inner space's downcast-based
        // comparison sees the concrete type, not this wrapper.
        if let Some(w) = (other as &dyn std::any::Any).downcast_ref::<ArcSpaceWrapper>() {
            self.0.topology_eq(&*w.0)
        } else {
            self.0.topology_eq(other)
        }
    }
}

// Delegate optional Space methods if they exist.
impl std::fmt::Debug for ArcSpaceWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ArcSpaceWrapper").finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::id::FieldId;
    use murk_core::{BoundaryBehavior, FieldDef, FieldMutability, FieldType};
    use murk_obs::spec::ObsRegion;
    use murk_obs::{ObsEntry, ObsSpec};
    use murk_space::{EdgeBehavior, Line1D};
    use murk_test_utils::ConstPropagator;

    fn scalar_field(name: &str) -> FieldDef {
        FieldDef {
            name: name.to_string(),
            field_type: FieldType::Scalar,
            mutability: FieldMutability::PerTick,
            units: None,
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        }
    }

    fn test_config() -> WorldConfig {
        WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("energy")],
            propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), 42.0))],
            dt: 0.1,
            seed: 42,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: Some(60.0),
            backoff: crate::config::BackoffConfig::default(),
        }
    }

    #[test]
    fn lifecycle_start_and_shutdown() {
        let mut world = RealtimeAsyncWorld::new(test_config(), AsyncConfig::default()).unwrap();

        // Wait for at least one snapshot (polling with timeout).
        let deadline = Instant::now() + Duration::from_secs(2);
        while world.latest_snapshot().is_none() {
            if Instant::now() > deadline {
                panic!("no snapshot produced within 2s");
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let epoch = world.current_epoch();
        assert!(epoch > 0, "epoch should have advanced");

        let report = world.shutdown();
        assert!(report.tick_joined);
        assert!(report.workers_joined > 0);
    }

    #[test]
    fn observe_returns_data() {
        let mut world = RealtimeAsyncWorld::new(test_config(), AsyncConfig::default()).unwrap();

        // Wait for at least one snapshot.
        let deadline = Instant::now() + Duration::from_secs(2);
        while world.latest_snapshot().is_none() {
            if Instant::now() > deadline {
                panic!("no snapshot produced within 2s");
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        // Compile an obs plan.
        let space = world.space();
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(murk_space::RegionSpec::All),
                pool: None,
                transform: murk_obs::spec::ObsTransform::Identity,
                dtype: murk_obs::spec::ObsDtype::F32,
            }],
        };
        let plan_result = ObsPlan::compile(&spec, space).unwrap();
        let plan = Arc::new(plan_result.plan);

        let mut output = vec![0.0f32; plan_result.output_len];
        let mut mask = vec![0u8; plan_result.mask_len];

        let meta = world.observe(&plan, &mut output, &mut mask).unwrap();
        assert!(meta.tick_id.0 > 0);
        assert_eq!(output.len(), 10);
        assert!(output.iter().all(|&v| v == 42.0));

        world.shutdown();
    }

    #[test]
    fn concurrent_observe() {
        let world = RealtimeAsyncWorld::new(test_config(), AsyncConfig::default()).unwrap();

        // Wait for at least one snapshot.
        let deadline = Instant::now() + Duration::from_secs(2);
        while world.latest_snapshot().is_none() {
            if Instant::now() > deadline {
                panic!("no snapshot produced within 2s");
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        let space = world.space();
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(murk_space::RegionSpec::All),
                pool: None,
                transform: murk_obs::spec::ObsTransform::Identity,
                dtype: murk_obs::spec::ObsDtype::F32,
            }],
        };
        let plan_result = ObsPlan::compile(&spec, space).unwrap();
        let plan = Arc::new(plan_result.plan);

        // Spawn 4 threads all calling observe concurrently.
        let world = Arc::new(world);
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let w = Arc::clone(&world);
                let p = Arc::clone(&plan);
                let out_len = plan_result.output_len;
                let mask_len = plan_result.mask_len;
                std::thread::spawn(move || {
                    let mut output = vec![0.0f32; out_len];
                    let mut mask = vec![0u8; mask_len];
                    let meta = w.observe(&p, &mut output, &mut mask).unwrap();
                    assert!(meta.tick_id.0 > 0);
                    assert!(output.iter().all(|&v| v == 42.0));
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        // Shutdown via Arc — need to unwrap.
        // Since we can't call shutdown on Arc, Drop will handle it.
        drop(world);
    }

    #[test]
    fn submit_commands_flow_through() {
        let mut world = RealtimeAsyncWorld::new(test_config(), AsyncConfig::default()).unwrap();

        // Wait for first tick.
        let deadline = Instant::now() + Duration::from_secs(2);
        while world.latest_snapshot().is_none() {
            if Instant::now() > deadline {
                panic!("no snapshot produced within 2s");
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        // Submit a command.
        let cmd = Command {
            payload: murk_core::command::CommandPayload::SetParameter {
                key: murk_core::id::ParameterKey(0),
                value: 1.0,
            },
            expires_after_tick: murk_core::id::TickId(10000),
            source_id: None,
            source_seq: None,
            priority_class: 1,
            arrival_seq: 0,
        };
        let receipts = world.submit_commands(vec![cmd]).unwrap();
        assert_eq!(receipts.len(), 1);
        assert!(receipts[0].accepted);

        world.shutdown();
    }

    #[test]
    fn drop_triggers_shutdown() {
        let world = RealtimeAsyncWorld::new(test_config(), AsyncConfig::default()).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        drop(world);
        // If this doesn't hang, shutdown worked.
    }

    #[test]
    fn shutdown_budget() {
        let mut world = RealtimeAsyncWorld::new(test_config(), AsyncConfig::default()).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        let report = world.shutdown();
        // Shutdown should complete well within 2s (generous for slow CI runners).
        assert!(
            report.total_ms < 2000,
            "shutdown took too long: {}ms",
            report.total_ms
        );
    }

    /// Regression test: with a very slow tick rate, shutdown must still
    /// complete within the documented budget. Before the fix, the tick
    /// thread used `thread::sleep` which was uninterruptible by the
    /// shutdown flag, causing shutdown to block for the full tick budget.
    #[test]
    fn shutdown_fast_with_slow_tick_rate() {
        let config = WorldConfig {
            tick_rate_hz: Some(0.5), // 2-second tick budget
            ..test_config()
        };
        let mut world = RealtimeAsyncWorld::new(config, AsyncConfig::default()).unwrap();

        // Wait for at least one tick to complete so the ring is non-empty
        // and the tick thread enters its budget sleep.
        let deadline = Instant::now() + Duration::from_secs(5);
        while world.latest_snapshot().is_none() {
            if Instant::now() > deadline {
                panic!("no snapshot produced within 5s");
            }
            std::thread::sleep(Duration::from_millis(10));
        }

        // Give the tick thread time to enter its budget sleep.
        std::thread::sleep(Duration::from_millis(50));

        let start = Instant::now();
        let report = world.shutdown();
        let wall_ms = start.elapsed().as_millis();

        // Shutdown should complete well under the 2-second tick budget.
        // Allow 500ms for CI overhead; the point is it shouldn't take 2s.
        assert!(
            wall_ms < 500,
            "shutdown took {wall_ms}ms with 0.5Hz tick rate \
             (report: total={}ms, drain={}ms, quiesce={}ms)",
            report.total_ms,
            report.drain_ms,
            report.quiesce_ms
        );
        assert!(report.tick_joined);
    }

    #[test]
    fn reset_lifecycle() {
        let mut world = RealtimeAsyncWorld::new(test_config(), AsyncConfig::default()).unwrap();

        // Wait for some ticks (generous timeout for slow CI runners).
        let deadline = Instant::now() + Duration::from_secs(5);
        while world.current_epoch() < 5 {
            if Instant::now() > deadline {
                panic!("epoch didn't reach 5 within 5s");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let epoch_before = world.current_epoch();
        assert!(epoch_before >= 5);

        // Reset with a new seed.
        world.reset(99).unwrap();

        // After reset, epoch should restart from 0.
        assert_eq!(world.current_epoch(), 0);

        // The world should produce new snapshots.
        let deadline = Instant::now() + Duration::from_secs(2);
        while world.latest_snapshot().is_none() {
            if Instant::now() > deadline {
                panic!("no snapshot after reset within 2s");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(world.current_epoch() > 0, "should be ticking after reset");

        world.shutdown();
    }

    #[test]
    fn arc_space_wrapper_topology_eq() {
        // Two ArcSpaceWrappers around identical Line1D spaces must compare
        // as topologically equal. Before the fix, the inner downcast saw
        // ArcSpaceWrapper instead of Line1D and returned false.
        let a = ArcSpaceWrapper(Arc::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()));
        let b = ArcSpaceWrapper(Arc::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()));
        assert!(
            a.topology_eq(&b),
            "identical Line1D through ArcSpaceWrapper should be topology-equal"
        );

        // Different sizes must not match.
        let c = ArcSpaceWrapper(Arc::new(Line1D::new(20, EdgeBehavior::Absorb).unwrap()));
        assert!(
            !a.topology_eq(&c),
            "different Line1D sizes should not be topology-equal"
        );

        // Comparing ArcSpaceWrapper with a bare Line1D should also work.
        let bare = Line1D::new(10, EdgeBehavior::Absorb).unwrap();
        assert!(
            a.topology_eq(&bare),
            "ArcSpaceWrapper(Line1D) vs bare Line1D should be topology-equal"
        );
    }
}
