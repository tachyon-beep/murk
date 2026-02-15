//! Determinism verification integration tests (R-DET-1 through R-DET-6).
//!
//! Each test: build config → run N ticks recording to ReplayWriter<Vec<u8>> →
//! rebuild config → replay via ReplayReader<&[u8]> through fresh LockstepWorld →
//! compare hashes per tick.

use murk_core::command::{Command, CommandPayload};
use murk_core::id::{Coord, FieldId, ParameterKey, TickId};
use murk_core::{BoundaryBehavior, FieldDef, FieldMutability, FieldType};
use murk_engine::{BackoffConfig, LockstepWorld, WorldConfig};
use murk_propagators::agent_movement::{
    new_action_buffer, ActionBuffer, AgentAction, AgentMovementPropagator, Direction,
};
use murk_propagators::{reference_fields, DiffusionPropagator, RewardPropagator};
use murk_replay::codec::{deserialize_command, serialize_command};
use murk_replay::hash::snapshot_hash;
use murk_replay::types::{BuildMetadata, InitDescriptor};
use murk_replay::{ReplayReader, ReplayWriter};
use murk_space::{EdgeBehavior, Square4};
use murk_test_utils::{ConstPropagator, FailingPropagator, IdentityPropagator};

// ── Helpers ─────────────────────────────────────────────────────

fn test_metadata() -> BuildMetadata {
    BuildMetadata {
        toolchain: env!("CARGO_PKG_VERSION").to_string(),
        target_triple: "test".to_string(),
        murk_version: "0.1.0".to_string(),
        compile_flags: "test".to_string(),
    }
}

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

fn sparse_field(name: &str) -> FieldDef {
    FieldDef {
        name: name.to_string(),
        field_type: FieldType::Scalar,
        mutability: FieldMutability::Sparse,
        units: None,
        bounds: None,
        boundary_behavior: BoundaryBehavior::Clamp,
    }
}

/// Record a run: step the world N ticks, writing each frame to the replay.
/// Returns the replay buffer and final world state for verification.
fn record_run(
    world: &mut LockstepWorld,
    writer: &mut ReplayWriter<&mut Vec<u8>>,
    ticks: u64,
    field_count: u32,
    commands_per_tick: &dyn Fn(u64) -> Vec<Command>,
) {
    for tick in 1..=ticks {
        let cmds = commands_per_tick(tick);
        let serialized: Vec<_> = cmds.iter().map(serialize_command).collect();
        let result = world.step_sync(cmds).unwrap();
        let hash = snapshot_hash(&result.snapshot, field_count);
        let frame = murk_replay::Frame {
            tick_id: tick,
            commands: serialized,
            snapshot_hash: hash,
        };
        writer.write_raw_frame(&frame).unwrap();
    }
}

/// Replay a recording through a fresh world, comparing hashes at every tick.
fn verify_replay(buf: &[u8], mut world: LockstepWorld, field_count: u32) {
    let mut reader = ReplayReader::open(buf).unwrap();

    while let Some(frame) = reader.next_frame().unwrap() {
        let commands: Vec<Command> = frame
            .commands
            .iter()
            .map(|sc| deserialize_command(sc).unwrap())
            .collect();

        let result = world.step_sync(commands).unwrap();
        let replayed_hash = snapshot_hash(&result.snapshot, field_count);

        assert_eq!(
            frame.snapshot_hash, replayed_hash,
            "determinism failure at tick {}: recorded={:#018x}, replayed={:#018x}",
            frame.tick_id, frame.snapshot_hash, replayed_hash,
        );
    }
}

// ── Reference pipeline config builder ───────────────────────────

fn reference_config(seed: u64, action_buffer: ActionBuffer) -> WorldConfig {
    let cell_count = 10 * 10;
    let initial_positions = murk_bench::init_agent_positions(cell_count, 4, seed);

    WorldConfig {
        space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
        fields: reference_fields(),
        propagators: vec![
            Box::new(DiffusionPropagator::new(0.1)),
            Box::new(AgentMovementPropagator::new(
                action_buffer,
                initial_positions,
            )),
            Box::new(RewardPropagator::new(1.0, -0.01)),
        ],
        dt: 0.1,
        seed,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    }
}

// ═══════════════════════════════════════════════════════════════
// MUST scenarios (1000+ ticks each)
// ═══════════════════════════════════════════════════════════════

/// Scenario 1: Sequential-commit vs Jacobi — reference pipeline has
/// reads_previous() (DiffusionPropagator) + reads() (RewardPropagator).
#[test]
fn determinism_sequential_vs_jacobi() {
    let seed = 42;
    let ticks = 1000;
    let field_count = 5; // reference pipeline: heat, velocity, agent_presence, gradient, reward

    // Record
    let ab_record = new_action_buffer();
    let config_record = reference_config(seed, ab_record);
    let mut world_record = LockstepWorld::new(config_record).unwrap();
    let meta = test_metadata();
    let init = InitDescriptor {
        seed,
        config_hash: 0,
        field_count,
        cell_count: 100,
        space_descriptor: vec![],
    };

    let mut buf = Vec::new();
    {
        let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
        record_run(
            &mut world_record,
            &mut writer,
            ticks,
            field_count,
            &|_| vec![],
        );
    }

    // Replay
    let ab_replay = new_action_buffer();
    let config_replay = reference_config(seed, ab_replay);
    let world_replay = LockstepWorld::new(config_replay).unwrap();
    verify_replay(&buf, world_replay, field_count);
}

/// Scenario 2: Multi-source command ordering — 3 sources submitting
/// commands with varying priority_class, source_id, source_seq.
#[test]
fn determinism_multi_source_ordering() {
    let seed = 77;
    let ticks = 1000;
    let field_count = 1;

    let make_config = || WorldConfig {
        space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
        fields: vec![scalar_field("energy")],
        propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))],
        dt: 0.1,
        seed,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    // Generate commands for each tick: 3 sources with varying priorities
    let commands_for_tick = |tick: u64| -> Vec<Command> {
        let mut cmds = Vec::new();
        for source in 1..=3u64 {
            let count = (tick % 5) + 1; // 1-5 commands per source per tick
            for seq in 0..count {
                cmds.push(Command {
                    payload: CommandPayload::SetParameter {
                        key: ParameterKey(0),
                        value: (tick * source + seq) as f64,
                    },
                    expires_after_tick: TickId(tick + 100),
                    source_id: Some(source),
                    source_seq: Some(tick * 10 + seq),
                    priority_class: (source % 3) as u8,
                    arrival_seq: 0,
                });
            }
        }
        cmds
    };

    // Record
    let mut world_record = LockstepWorld::new(make_config()).unwrap();
    let meta = test_metadata();
    let init = InitDescriptor {
        seed,
        config_hash: 0,
        field_count,
        cell_count: 100,
        space_descriptor: vec![],
    };

    let mut buf = Vec::new();
    {
        let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
        record_run(
            &mut world_record,
            &mut writer,
            ticks,
            field_count,
            &commands_for_tick,
        );
    }

    // Replay
    let world_replay = LockstepWorld::new(make_config()).unwrap();
    verify_replay(&buf, world_replay, field_count);
}

/// Scenario 3: WriteMode::Incremental — AgentMovementPropagator uses
/// Incremental mode. Inject actions via ActionBuffer every tick.
#[test]
fn determinism_incremental_write_mode() {
    let seed = 99;
    let ticks = 1000;
    let field_count = 5;

    // We need matching action sequences for record and replay.
    // Use deterministic actions based on tick number.
    fn inject_actions(ab: &ActionBuffer, tick: u64) {
        let mut actions = ab.lock().unwrap();
        // Rotate agents through directions based on tick
        let directions = [
            Direction::North,
            Direction::South,
            Direction::East,
            Direction::West,
            Direction::Stay,
        ];
        for agent_id in 0..4u16 {
            let dir_idx = ((tick as usize) + (agent_id as usize)) % directions.len();
            actions.push(AgentAction {
                agent_id,
                direction: directions[dir_idx],
            });
        }
    }

    // Record
    let ab_record = new_action_buffer();
    let config_record = reference_config(seed, ab_record.clone());
    let mut world_record = LockstepWorld::new(config_record).unwrap();
    let meta = test_metadata();
    let init = InitDescriptor {
        seed,
        config_hash: 0,
        field_count,
        cell_count: 100,
        space_descriptor: vec![],
    };

    let mut buf = Vec::new();
    {
        let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
        for tick in 1..=ticks {
            inject_actions(&ab_record, tick);
            let result = world_record.step_sync(vec![]).unwrap();
            let hash = snapshot_hash(&result.snapshot, field_count);
            let frame = murk_replay::Frame {
                tick_id: tick,
                commands: vec![],
                snapshot_hash: hash,
            };
            writer.write_raw_frame(&frame).unwrap();
        }
    }

    // Replay with identical action injection
    let ab_replay = new_action_buffer();
    let config_replay = reference_config(seed, ab_replay.clone());
    let mut world_replay = LockstepWorld::new(config_replay).unwrap();

    let mut reader = ReplayReader::open(buf.as_slice()).unwrap();
    let mut tick = 0u64;
    while let Some(frame) = reader.next_frame().unwrap() {
        tick += 1;
        inject_actions(&ab_replay, tick);
        let result = world_replay.step_sync(vec![]).unwrap();
        let replayed_hash = snapshot_hash(&result.snapshot, field_count);
        assert_eq!(
            frame.snapshot_hash, replayed_hash,
            "incremental mode determinism failure at tick {tick}"
        );
    }
    assert_eq!(tick, ticks);
}

/// Scenario 4: Arena double-buffer recycling — reference profile, 1000+
/// ticks, ping-pong naturally exercises recycling.
#[test]
fn determinism_arena_recycling() {
    let seed = 123;
    let ticks = 1100;
    let field_count = 5;

    // Record
    let ab_record = new_action_buffer();
    let config_record = reference_config(seed, ab_record);
    let mut world_record = LockstepWorld::new(config_record).unwrap();
    let meta = test_metadata();
    let init = InitDescriptor {
        seed,
        config_hash: 0,
        field_count,
        cell_count: 100,
        space_descriptor: vec![],
    };

    let mut buf = Vec::new();
    {
        let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
        record_run(
            &mut world_record,
            &mut writer,
            ticks,
            field_count,
            &|_| vec![],
        );
    }

    // Replay
    let ab_replay = new_action_buffer();
    let config_replay = reference_config(seed, ab_replay);
    let world_replay = LockstepWorld::new(config_replay).unwrap();
    verify_replay(&buf, world_replay, field_count);
}

/// Scenario 5: Sparse field modification — SetField commands modify cells
/// intermittently on a Sparse mutability field.
#[test]
fn determinism_sparse_field() {
    let seed = 456;
    let ticks = 1000;
    let field_count = 2; // const field + sparse field

    let make_config = || WorldConfig {
        space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
        fields: vec![scalar_field("energy"), sparse_field("marker")],
        propagators: vec![
            Box::new(ConstPropagator::new("const_energy", FieldId(0), 1.0)),
            Box::new(ConstPropagator::new("const_marker", FieldId(1), 0.0)),
        ],
        dt: 0.1,
        seed,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    // SetField commands every 10 ticks
    let commands_for_tick = |tick: u64| -> Vec<Command> {
        if tick.is_multiple_of(10) {
            let cell = (tick / 10) % 100;
            let row = (cell / 10) as i32;
            let col = (cell % 10) as i32;
            vec![Command {
                payload: CommandPayload::SetField {
                    coord: Coord::from_slice(&[row, col]),
                    field_id: FieldId(1),
                    value: tick as f32,
                },
                expires_after_tick: TickId(tick + 100),
                source_id: Some(1),
                source_seq: Some(tick),
                priority_class: 1,
                arrival_seq: 0,
            }]
        } else {
            vec![]
        }
    };

    // Record
    let mut world_record = LockstepWorld::new(make_config()).unwrap();
    let meta = test_metadata();
    let init = InitDescriptor {
        seed,
        config_hash: 0,
        field_count,
        cell_count: 100,
        space_descriptor: vec![],
    };

    let mut buf = Vec::new();
    {
        let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
        record_run(
            &mut world_record,
            &mut writer,
            ticks,
            field_count,
            &commands_for_tick,
        );
    }

    // Replay
    let world_replay = LockstepWorld::new(make_config()).unwrap();
    verify_replay(&buf, world_replay, field_count);
}

// ═══════════════════════════════════════════════════════════════
// SHOULD scenarios
// ═══════════════════════════════════════════════════════════════

/// Scenario 6: Tick rollback recovery — FailingPropagator fails at tick 50,
/// succeeds after. Verify identical post-recovery state.
#[test]
fn determinism_rollback_recovery() {
    let seed = 789;
    // FailingPropagator(succeed_count=49) succeeds ticks 1-49, fails at tick 50
    let field_count = 1;

    let make_config = || WorldConfig {
        space: Box::new(Square4::new(5, 5, EdgeBehavior::Absorb).unwrap()),
        fields: vec![scalar_field("energy")],
        propagators: vec![Box::new(FailingPropagator::new("failer", FieldId(0), 49))],
        dt: 0.1,
        seed,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    // Record: step through failure and recovery
    let meta = test_metadata();
    let init = InitDescriptor {
        seed,
        config_hash: 0,
        field_count,
        cell_count: 25,
        space_descriptor: vec![],
    };

    let mut buf = Vec::new();
    let mut world_record = LockstepWorld::new(make_config()).unwrap();
    {
        let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
        for tick in 1..=100u64 {
            match world_record.step_sync(vec![]) {
                Ok(result) => {
                    let hash = snapshot_hash(&result.snapshot, field_count);
                    let frame = murk_replay::Frame {
                        tick_id: tick,
                        commands: vec![],
                        snapshot_hash: hash,
                    };
                    writer.write_raw_frame(&frame).unwrap();
                }
                Err(_) => {
                    // Record the snapshot hash even after failure
                    // (world rolls back, snapshot reflects pre-failure state)
                    let snap = world_record.snapshot();
                    let hash = snapshot_hash(&snap, field_count);
                    let frame = murk_replay::Frame {
                        tick_id: tick,
                        commands: vec![],
                        snapshot_hash: hash,
                    };
                    writer.write_raw_frame(&frame).unwrap();
                }
            }
        }
    }

    // Replay
    let mut world_replay = LockstepWorld::new(make_config()).unwrap();
    let mut reader = ReplayReader::open(buf.as_slice()).unwrap();
    let mut tick = 0u64;
    while let Some(frame) = reader.next_frame().unwrap() {
        tick += 1;
        match world_replay.step_sync(vec![]) {
            Ok(result) => {
                let replayed_hash = snapshot_hash(&result.snapshot, field_count);
                assert_eq!(
                    frame.snapshot_hash, replayed_hash,
                    "rollback recovery determinism failure at tick {tick}"
                );
            }
            Err(_) => {
                let snap = world_replay.snapshot();
                let replayed_hash = snapshot_hash(&snap, field_count);
                assert_eq!(
                    frame.snapshot_hash, replayed_hash,
                    "rollback recovery determinism failure at tick {tick} (post-failure)"
                );
            }
        }
    }
    assert!(tick > 0);
}

/// Scenario 7: GlobalParameter mid-episode — SetParameter/SetParameterBatch
/// commands at various ticks.
#[test]
fn determinism_global_parameter() {
    let seed = 111;
    let ticks = 1000;
    let field_count = 1;

    let make_config = || WorldConfig {
        space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
        fields: vec![scalar_field("energy")],
        propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))],
        dt: 0.1,
        seed,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    let commands_for_tick = |tick: u64| -> Vec<Command> {
        let mut cmds = Vec::new();
        // SetParameter every 50 ticks
        if tick.is_multiple_of(50) {
            cmds.push(Command {
                payload: CommandPayload::SetParameter {
                    key: ParameterKey(0),
                    value: tick as f64 * 0.001,
                },
                expires_after_tick: TickId(tick + 100),
                source_id: Some(1),
                source_seq: Some(tick),
                priority_class: 0,
                arrival_seq: 0,
            });
        }
        // SetParameterBatch every 100 ticks
        if tick.is_multiple_of(100) {
            cmds.push(Command {
                payload: CommandPayload::SetParameterBatch {
                    params: vec![
                        (ParameterKey(0), tick as f64 * 0.01),
                        (ParameterKey(1), tick as f64 * 0.02),
                    ],
                },
                expires_after_tick: TickId(tick + 100),
                source_id: Some(2),
                source_seq: Some(tick),
                priority_class: 0,
                arrival_seq: 0,
            });
        }
        cmds
    };

    // Record
    let mut world_record = LockstepWorld::new(make_config()).unwrap();
    let meta = test_metadata();
    let init = InitDescriptor {
        seed,
        config_hash: 0,
        field_count,
        cell_count: 100,
        space_descriptor: vec![],
    };

    let mut buf = Vec::new();
    {
        let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
        record_run(
            &mut world_record,
            &mut writer,
            ticks,
            field_count,
            &commands_for_tick,
        );
    }

    // Replay
    let world_replay = LockstepWorld::new(make_config()).unwrap();
    verify_replay(&buf, world_replay, field_count);
}

/// Scenario 8: 10+ propagator pipeline — 10+ propagators with separate fields.
#[test]
fn determinism_large_pipeline() {
    let seed = 222;
    let ticks = 1000;
    let num_propagators = 12;
    let field_count = num_propagators as u32;

    let make_config = || {
        let fields: Vec<FieldDef> = (0..num_propagators)
            .map(|i| scalar_field(&format!("field_{i}")))
            .collect();

        let mut propagators: Vec<Box<dyn murk_propagator::Propagator>> = Vec::new();
        // First half: ConstPropagators with different values
        for i in 0..num_propagators / 2 {
            propagators.push(Box::new(ConstPropagator::new(
                format!("const_{i}"),
                FieldId(i as u32),
                (i + 1) as f32,
            )));
        }
        // Second half: IdentityPropagators copying from first half
        for i in num_propagators / 2..num_propagators {
            let src = i - num_propagators / 2;
            propagators.push(Box::new(IdentityPropagator::new(
                format!("copy_{src}_to_{i}"),
                FieldId(src as u32),
                FieldId(i as u32),
            )));
        }

        WorldConfig {
            space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
            fields,
            propagators,
            dt: 0.1,
            seed,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: BackoffConfig::default(),
        }
    };

    // Record
    let mut world_record = LockstepWorld::new(make_config()).unwrap();
    let meta = test_metadata();
    let init = InitDescriptor {
        seed,
        config_hash: 0,
        field_count,
        cell_count: 100,
        space_descriptor: vec![],
    };

    let mut buf = Vec::new();
    {
        let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
        record_run(
            &mut world_record,
            &mut writer,
            ticks,
            field_count,
            &|_| vec![],
        );
    }

    // Replay
    let world_replay = LockstepWorld::new(make_config()).unwrap();
    verify_replay(&buf, world_replay, field_count);
}
