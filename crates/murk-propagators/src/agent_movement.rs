//! Incremental agent movement propagator.
//!
//! Reads actions from a shared [`ActionBuffer`] and updates agent presence
//! using [`WriteMode::Incremental`] (seeded from previous generation).

use crate::fields::AGENT_PRESENCE;
use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use murk_space::Square4;
use std::sync::{Arc, Mutex};

/// Cardinal direction for agent movement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Direction {
    /// Agent does not move.
    Stay = 0,
    /// Move one cell north (row - 1).
    North = 1,
    /// Move one cell south (row + 1).
    South = 2,
    /// Move one cell east (col + 1).
    East = 3,
    /// Move one cell west (col - 1).
    West = 4,
}

impl Direction {
    /// Returns the (row_offset, col_offset) for this direction.
    pub fn offset_2d(self) -> (i32, i32) {
        match self {
            Direction::Stay => (0, 0),
            Direction::North => (-1, 0),
            Direction::South => (1, 0),
            Direction::East => (0, 1),
            Direction::West => (0, -1),
        }
    }
}

/// A single agent action to be processed by the movement propagator.
#[derive(Clone, Debug)]
pub struct AgentAction {
    /// Unique agent identifier (used for deterministic ordering).
    pub agent_id: u16,
    /// Direction to move.
    pub direction: Direction,
}

/// Thread-safe buffer for injecting agent actions into the propagator pipeline.
pub type ActionBuffer = Arc<Mutex<Vec<AgentAction>>>;

/// Creates a new empty action buffer.
pub fn new_action_buffer() -> ActionBuffer {
    Arc::new(Mutex::new(Vec::new()))
}

/// Incremental agent movement propagator.
///
/// On tick 0 (when presence is all zeros), places agents at their initial
/// positions. On subsequent ticks, reads the action buffer and moves agents.
/// Collision resolution: if target cell is occupied, agent stays.
/// Boundary resolution: if target cell is OOB, agent stays.
pub struct AgentMovementPropagator {
    action_buffer: ActionBuffer,
    initial_positions: Vec<(u16, usize)>,
}

impl AgentMovementPropagator {
    /// Create a new movement propagator.
    ///
    /// `initial_positions` maps `(agent_id, flat_index)` for tick-0 placement.
    /// `action_buffer` is shared with the external action source.
    pub fn new(action_buffer: ActionBuffer, initial_positions: Vec<(u16, usize)>) -> Self {
        Self {
            action_buffer,
            initial_positions,
        }
    }
}

impl Propagator for AgentMovementPropagator {
    fn name(&self) -> &str {
        "AgentMovementPropagator"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(AGENT_PRESENCE, WriteMode::Incremental)]
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let cell_count = ctx.space().cell_count();

        // Precompute grid dimensions before taking the mutable writer borrow
        let grid_dims = ctx
            .space()
            .downcast_ref::<Square4>()
            .map(|g| (g.rows() as usize, g.cols() as usize));

        // For generic spaces, precompute a direction-offset → target-rank map
        // for every cell. Key: (cell_rank, dr, dc) → target_rank.
        // For Square4, we skip this (use index arithmetic instead).
        let generic_targets: Vec<Vec<(i32, i32, usize)>> = if grid_dims.is_none() {
            let ordering = ctx.space().canonical_ordering();
            ordering
                .iter()
                .map(|coord| {
                    let neighbours = ctx.space().neighbours(coord);
                    neighbours
                        .iter()
                        .filter_map(|nb| {
                            ctx.space().canonical_rank(nb).map(|rank| {
                                let dr = nb[0] - coord[0];
                                let dc = if nb.len() > 1 { nb[1] - coord[1] } else { 0 };
                                (dr, dc, rank)
                            })
                        })
                        .collect()
                })
                .collect()
        } else {
            Vec::new()
        };

        // Lock actions and sort for deterministic processing
        let mut actions =
            self.action_buffer
                .lock()
                .map_err(|_| PropagatorError::ExecutionFailed {
                    reason: "action buffer lock poisoned".into(),
                })?;
        actions.sort_by_key(|a| a.agent_id);
        let actions_snapshot: Vec<AgentAction> = actions.drain(..).collect();
        drop(actions);

        // Now take the mutable write borrow
        let presence =
            ctx.writes()
                .write(AGENT_PRESENCE)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: "agent_presence field not writable".into(),
                })?;

        // Tick 0 init: if all zeros, place agents at initial positions
        let all_zero = presence.iter().all(|&v| v == 0.0);
        if all_zero && !self.initial_positions.is_empty() {
            for &(agent_id, flat_idx) in &self.initial_positions {
                if flat_idx < cell_count {
                    presence[flat_idx] = (agent_id as f32) + 1.0;
                }
            }
        }

        if actions_snapshot.is_empty() {
            return Ok(());
        }

        let (rows, cols) = grid_dims.unwrap_or((0, 0));

        for action in &actions_snapshot {
            if action.direction == Direction::Stay {
                continue;
            }

            let agent_marker = (action.agent_id as f32) + 1.0;

            // Find current position of this agent
            let current_pos = match presence
                .iter()
                .position(|&v| (v - agent_marker).abs() < 0.5)
            {
                Some(pos) => pos,
                None => continue,
            };

            let (dr, dc) = action.direction.offset_2d();

            let target = if rows > 0 && cols > 0 {
                let r = (current_pos / cols) as i32;
                let c = (current_pos % cols) as i32;
                let nr = r + dr;
                let nc = c + dc;
                if nr < 0 || nr >= rows as i32 || nc < 0 || nc >= cols as i32 {
                    None
                } else {
                    Some((nr as usize) * cols + (nc as usize))
                }
            } else if current_pos < generic_targets.len() {
                generic_targets[current_pos]
                    .iter()
                    .find(|&&(r, c, _)| r == dr && c == dc)
                    .map(|&(_, _, rank)| rank)
            } else {
                None
            };

            let target = match target {
                Some(t) => t,
                None => continue,
            };

            if presence[target] != 0.0 {
                continue;
            }

            presence[target] = presence[current_pos];
            presence[current_pos] = 0.0;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::{FieldWriter, TickId};
    use murk_propagator::scratch::ScratchRegion;
    use murk_space::{EdgeBehavior, Space};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    fn make_ctx<'a>(
        reader: &'a MockFieldReader,
        writer: &'a mut MockFieldWriter,
        scratch: &'a mut ScratchRegion,
        space: &'a Square4,
    ) -> StepContext<'a> {
        StepContext::new(reader, reader, writer, scratch, space, TickId(1), 0.01)
    }

    fn setup_presence(
        grid: &Square4,
        initial: &[(u16, usize)],
    ) -> (MockFieldReader, MockFieldWriter) {
        let n = grid.cell_count();
        let reader = MockFieldReader::new();
        let mut writer = MockFieldWriter::new();

        let mut presence = vec![0.0f32; n];
        for &(agent_id, pos) in initial {
            presence[pos] = (agent_id as f32) + 1.0;
        }
        writer.add_field(AGENT_PRESENCE, n);
        // Simulate Incremental mode: pre-fill writer with presence data
        let field = writer.get_field(AGENT_PRESENCE).unwrap().to_vec();
        drop(field);
        // We need to write initial data into the writer's buffer
        let buf = writer.write(AGENT_PRESENCE).unwrap();
        buf.copy_from_slice(&presence);

        (reader, writer)
    }

    #[test]
    fn tick0_initialization() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let ab = new_action_buffer();
        let prop = AgentMovementPropagator::new(ab, vec![(0, 4), (1, 0)]);

        let reader = MockFieldReader::new();
        let mut writer = MockFieldWriter::new();
        writer.add_field(AGENT_PRESENCE, 9);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);

        prop.step(&mut ctx).unwrap();

        let presence = writer.get_field(AGENT_PRESENCE).unwrap();
        assert_eq!(presence[4], 1.0); // agent 0 → marker 1.0
        assert_eq!(presence[0], 2.0); // agent 1 → marker 2.0
    }

    #[test]
    fn move_north() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let ab = new_action_buffer();

        // Agent 0 at center (1,1) = flat index 4
        let (reader, mut writer) = setup_presence(&grid, &[(0, 4)]);

        ab.lock().unwrap().push(AgentAction {
            agent_id: 0,
            direction: Direction::North,
        });

        let prop = AgentMovementPropagator::new(ab, vec![]);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);

        prop.step(&mut ctx).unwrap();

        let presence = writer.get_field(AGENT_PRESENCE).unwrap();
        assert_eq!(presence[4], 0.0); // old position cleared
        assert_eq!(presence[1], 1.0); // moved to (0,1)
    }

    #[test]
    fn move_south() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let ab = new_action_buffer();
        let (reader, mut writer) = setup_presence(&grid, &[(0, 4)]);

        ab.lock().unwrap().push(AgentAction {
            agent_id: 0,
            direction: Direction::South,
        });

        let prop = AgentMovementPropagator::new(ab, vec![]);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let presence = writer.get_field(AGENT_PRESENCE).unwrap();
        assert_eq!(presence[4], 0.0);
        assert_eq!(presence[7], 1.0); // (2,1)
    }

    #[test]
    fn move_east() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let ab = new_action_buffer();
        let (reader, mut writer) = setup_presence(&grid, &[(0, 4)]);

        ab.lock().unwrap().push(AgentAction {
            agent_id: 0,
            direction: Direction::East,
        });

        let prop = AgentMovementPropagator::new(ab, vec![]);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let presence = writer.get_field(AGENT_PRESENCE).unwrap();
        assert_eq!(presence[4], 0.0);
        assert_eq!(presence[5], 1.0); // (1,2)
    }

    #[test]
    fn move_west() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let ab = new_action_buffer();
        let (reader, mut writer) = setup_presence(&grid, &[(0, 4)]);

        ab.lock().unwrap().push(AgentAction {
            agent_id: 0,
            direction: Direction::West,
        });

        let prop = AgentMovementPropagator::new(ab, vec![]);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let presence = writer.get_field(AGENT_PRESENCE).unwrap();
        assert_eq!(presence[4], 0.0);
        assert_eq!(presence[3], 1.0); // (1,0)
    }

    #[test]
    fn boundary_blocks_movement() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let ab = new_action_buffer();

        // Agent 0 at top-left corner (0,0) = flat index 0
        let (reader, mut writer) = setup_presence(&grid, &[(0, 0)]);

        ab.lock().unwrap().push(AgentAction {
            agent_id: 0,
            direction: Direction::North, // OOB
        });

        let prop = AgentMovementPropagator::new(ab, vec![]);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let presence = writer.get_field(AGENT_PRESENCE).unwrap();
        assert_eq!(presence[0], 1.0); // agent stayed
    }

    #[test]
    fn collision_blocks_movement() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let ab = new_action_buffer();

        // Agent 0 at (1,1)=4, Agent 1 at (0,1)=1
        let (reader, mut writer) = setup_presence(&grid, &[(0, 4), (1, 1)]);

        ab.lock().unwrap().push(AgentAction {
            agent_id: 0,
            direction: Direction::North, // target (0,1) occupied by agent 1
        });

        let prop = AgentMovementPropagator::new(ab, vec![]);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let presence = writer.get_field(AGENT_PRESENCE).unwrap();
        assert_eq!(presence[4], 1.0); // agent 0 stayed (marker 1.0)
        assert_eq!(presence[1], 2.0); // agent 1 still there (marker 2.0)
    }

    #[test]
    fn action_buffer_cleared_after_processing() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let ab = new_action_buffer();
        let (reader, mut writer) = setup_presence(&grid, &[(0, 4)]);

        ab.lock().unwrap().push(AgentAction {
            agent_id: 0,
            direction: Direction::Stay,
        });

        let prop = AgentMovementPropagator::new(ab.clone(), vec![]);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        assert!(ab.lock().unwrap().is_empty());
    }
}
