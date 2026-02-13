//! Egress worker pool for RealtimeAsync observation extraction.
//!
//! Each worker receives [`ObsTask`] requests via a crossbeam channel,
//! pins to the latest snapshot epoch, executes the observation plan,
//! unpins, and sends the result back via a bounded(1) reply channel.
//!
//! Workers allocate their own output buffers and return them via the
//! reply channel. The caller copies into its buffer. This maintains
//! `#![forbid(unsafe_code)]` at the cost of a ~10μs memcpy (negligible
//! vs the 16.7ms tick budget).

use std::sync::Arc;

use crossbeam_channel::Receiver;

use crate::epoch::{EpochCounter, WorkerEpoch};
use crate::ring::SnapshotRing;

use murk_core::Coord;
use murk_obs::{ObsMetadata, ObsPlan};
use murk_space::Space;

/// A task dispatched to an egress worker.
pub(crate) enum ObsTask {
    /// Simple observation (all Fixed regions).
    Simple {
        plan: Arc<ObsPlan>,
        output_len: usize,
        mask_len: usize,
        reply: crossbeam_channel::Sender<ObsResult>,
    },
    /// Agent-relative observation (has AgentRelative regions).
    Agents {
        plan: Arc<ObsPlan>,
        space: Arc<dyn Space>,
        agent_centers: Vec<Coord>,
        output_len: usize,
        mask_len: usize,
        reply: crossbeam_channel::Sender<ObsResult>,
    },
}

/// Result of an egress worker's observation execution.
#[derive(Debug)]
pub(crate) enum ObsResult {
    /// Simple plan result: one metadata + output + mask buffers.
    Simple {
        metadata: ObsMetadata,
        output: Vec<f32>,
        mask: Vec<u8>,
    },
    /// Agent plan result: one metadata per agent + output + mask buffers.
    Agents {
        metadata: Vec<ObsMetadata>,
        output: Vec<f32>,
        mask: Vec<u8>,
    },
    /// Plan execution failed.
    Error(murk_core::error::ObsError),
}

/// Main loop for an egress worker thread, using an index into a shared
/// `Arc<[WorkerEpoch]>` array. This ensures the tick thread's stall
/// detector and the worker see the same `WorkerEpoch` instance.
pub(crate) fn worker_loop_indexed(
    task_rx: Receiver<ObsTask>,
    ring: Arc<SnapshotRing>,
    epoch_counter: Arc<EpochCounter>,
    worker_epochs: Arc<[WorkerEpoch]>,
    worker_index: usize,
) {
    let worker_epoch = &worker_epochs[worker_index];
    worker_loop_inner(task_rx, ring, epoch_counter, worker_epoch);
}

/// Main loop for an egress worker thread (Arc variant, used in tests).
///
/// Runs until the task channel is closed (sender dropped). Each
/// iteration: recv task → pin epoch → execute plan → unpin → reply.
#[cfg(test)]
pub(crate) fn worker_loop(
    task_rx: Receiver<ObsTask>,
    ring: Arc<SnapshotRing>,
    epoch_counter: Arc<EpochCounter>,
    worker_epoch: Arc<WorkerEpoch>,
) {
    worker_loop_inner(task_rx, ring, epoch_counter, &*worker_epoch);
}

fn worker_loop_inner(
    task_rx: Receiver<ObsTask>,
    ring: Arc<SnapshotRing>,
    epoch_counter: Arc<EpochCounter>,
    worker_epoch: &WorkerEpoch,
) {
    while let Ok(task) = task_rx.recv() {
        // Check for cooperative cancellation before starting.
        if worker_epoch.is_cancelled() {
            worker_epoch.clear_cancel();
            send_error(&task, murk_core::error::ObsError::ExecutionFailed {
                reason: "worker cancelled before execution".into(),
            });
            continue;
        }

        // Get latest snapshot.
        let snapshot = match ring.latest() {
            Some(snap) => snap,
            None => {
                send_error(&task, murk_core::error::ObsError::ExecutionFailed {
                    reason: "no snapshot available".into(),
                });
                continue;
            }
        };

        // Pin to the current epoch.
        let epoch = epoch_counter.current();
        worker_epoch.pin(epoch);

        // Execute the plan. Always unpin, even on error.
        let result = execute_task(&task, &*snapshot, epoch_counter.current());
        worker_epoch.unpin();

        // Send result back.
        match task {
            ObsTask::Simple { reply, .. } | ObsTask::Agents { reply, .. } => {
                let _ = reply.send(result);
            }
        }
    }
    // Channel closed — worker exits cleanly.
}

/// Execute a task against a snapshot, returning the result.
fn execute_task(
    task: &ObsTask,
    snapshot: &dyn murk_core::traits::SnapshotAccess,
    current_tick_val: u64,
) -> ObsResult {
    let engine_tick = Some(murk_core::id::TickId(current_tick_val));

    match task {
        ObsTask::Simple {
            plan,
            output_len,
            mask_len,
            ..
        } => {
            let mut output = vec![0.0f32; *output_len];
            let mut mask = vec![0u8; *mask_len];
            match plan.execute(snapshot, engine_tick, &mut output, &mut mask) {
                Ok(metadata) => ObsResult::Simple {
                    metadata,
                    output,
                    mask,
                },
                Err(e) => ObsResult::Error(e),
            }
        }
        ObsTask::Agents {
            plan,
            space,
            agent_centers,
            output_len,
            mask_len,
            ..
        } => {
            let n_agents = agent_centers.len();
            let mut output = vec![0.0f32; output_len * n_agents];
            let mut mask = vec![0u8; mask_len * n_agents];
            match plan.execute_agents(
                snapshot,
                space.as_ref(),
                agent_centers,
                engine_tick,
                &mut output,
                &mut mask,
            ) {
                Ok(metadata) => ObsResult::Agents {
                    metadata,
                    output,
                    mask,
                },
                Err(e) => ObsResult::Error(e),
            }
        }
    }
}

/// Send an error result back through the task's reply channel.
fn send_error(task: &ObsTask, err: murk_core::error::ObsError) {
    match task {
        ObsTask::Simple { reply, .. } => {
            let _ = reply.send(ObsResult::Error(err));
        }
        ObsTask::Agents { reply, .. } => {
            let _ = reply.send(ObsResult::Error(err));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_arena::config::ArenaConfig;
    use murk_arena::pingpong::PingPongArena;
    use murk_arena::static_arena::StaticArena;
    use murk_core::id::{FieldId, ParameterVersion, TickId};
    use murk_core::traits::FieldWriter as _;
    use murk_core::{BoundaryBehavior, FieldDef, FieldMutability, FieldType};
    use murk_obs::spec::ObsRegion;
    use murk_obs::{ObsEntry, ObsSpec};
    use murk_space::{EdgeBehavior, Line1D};
    use std::thread;

    fn make_test_snapshot(tick: u64, value: f32, cells: u32) -> murk_arena::OwnedSnapshot {
        let config = ArenaConfig::new(cells);
        let field_defs = vec![(
            FieldId(0),
            FieldDef {
                name: "energy".into(),
                field_type: FieldType::Scalar,
                mutability: FieldMutability::PerTick,
                units: None,
                bounds: None,
                boundary_behavior: BoundaryBehavior::Clamp,
            },
        )];
        let static_arena = StaticArena::new(&[]).into_shared();
        let mut arena = PingPongArena::new(config, field_defs, static_arena).unwrap();
        {
            let mut guard = arena.begin_tick().unwrap();
            let data = guard.writer.write(FieldId(0)).unwrap();
            data.fill(value);
        }
        arena.publish(TickId(tick), ParameterVersion(0));
        arena.owned_snapshot()
    }

    #[test]
    fn worker_executes_simple_plan() {
        let cells = 10u32;
        let space = Line1D::new(cells, EdgeBehavior::Absorb).unwrap();

        // Build obs plan.
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(murk_space::RegionSpec::All),
                pool: None,
                transform: murk_obs::spec::ObsTransform::Identity,
                dtype: murk_obs::spec::ObsDtype::F32,
            }],
        };
        let plan_result = ObsPlan::compile(&spec, &space).unwrap();
        let plan = Arc::new(plan_result.plan);
        let output_len = plan_result.output_len;
        let mask_len = plan_result.mask_len;

        // Set up ring + epoch.
        let ring = Arc::new(SnapshotRing::new(4));
        ring.push(make_test_snapshot(1, 42.0, cells));

        let epoch_counter = Arc::new(EpochCounter::new());
        epoch_counter.advance();

        let worker_epoch = Arc::new(WorkerEpoch::new(0));

        // Create task channel.
        let (task_tx, task_rx) = crossbeam_channel::bounded(4);
        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);

        // Spawn worker.
        let ring_c = Arc::clone(&ring);
        let epoch_c = Arc::clone(&epoch_counter);
        let we_c = Arc::clone(&worker_epoch);
        let handle = thread::spawn(move || {
            worker_loop(task_rx, ring_c, epoch_c, we_c);
        });

        // Send task.
        task_tx
            .send(ObsTask::Simple {
                plan,
                output_len,
                mask_len,
                reply: reply_tx,
            })
            .unwrap();

        // Get result.
        let result = reply_rx.recv().unwrap();
        match result {
            ObsResult::Simple {
                metadata,
                output,
                mask,
            } => {
                assert_eq!(metadata.tick_id, TickId(1));
                assert_eq!(output.len(), output_len);
                assert!(output.iter().all(|&v| v == 42.0));
                assert_eq!(mask.len(), mask_len);
            }
            other => panic!("expected Simple result, got error: {other:?}"),
        }

        // Worker should be unpinned.
        assert!(!worker_epoch.is_pinned());

        // Drop sender to close channel and join worker.
        drop(task_tx);
        handle.join().unwrap();
    }

    #[test]
    fn worker_unpins_on_error() {
        // With an empty ring, the worker should return an error but still unpin.
        let ring = Arc::new(SnapshotRing::new(4));
        let epoch_counter = Arc::new(EpochCounter::new());
        let worker_epoch = Arc::new(WorkerEpoch::new(0));

        let (task_tx, task_rx) = crossbeam_channel::bounded(4);
        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);

        let ring_c = Arc::clone(&ring);
        let epoch_c = Arc::clone(&epoch_counter);
        let we_c = Arc::clone(&worker_epoch);
        let handle = thread::spawn(move || {
            worker_loop(task_rx, ring_c, epoch_c, we_c);
        });

        // Build a dummy plan — doesn't matter, we'll error before execute.
        let space = Line1D::new(10, EdgeBehavior::Absorb).unwrap();
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(murk_space::RegionSpec::All),
                pool: None,
                transform: murk_obs::spec::ObsTransform::Identity,
                dtype: murk_obs::spec::ObsDtype::F32,
            }],
        };
        let plan_result = ObsPlan::compile(&spec, &space).unwrap();

        task_tx
            .send(ObsTask::Simple {
                plan: Arc::new(plan_result.plan),
                output_len: plan_result.output_len,
                mask_len: plan_result.mask_len,
                reply: reply_tx,
            })
            .unwrap();

        let result = reply_rx.recv().unwrap();
        assert!(matches!(result, ObsResult::Error(_)));
        assert!(!worker_epoch.is_pinned());

        drop(task_tx);
        handle.join().unwrap();
    }

    #[test]
    fn worker_exits_on_channel_close() {
        let ring = Arc::new(SnapshotRing::new(4));
        let epoch_counter = Arc::new(EpochCounter::new());
        let worker_epoch = Arc::new(WorkerEpoch::new(0));

        let (task_tx, task_rx) = crossbeam_channel::bounded::<ObsTask>(4);

        let ring_c = Arc::clone(&ring);
        let epoch_c = Arc::clone(&epoch_counter);
        let we_c = Arc::clone(&worker_epoch);
        let handle = thread::spawn(move || {
            worker_loop(task_rx, ring_c, epoch_c, we_c);
        });

        // Drop sender — worker should exit.
        drop(task_tx);
        handle.join().unwrap();
    }
}
