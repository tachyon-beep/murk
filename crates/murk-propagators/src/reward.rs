//! Multi-field reward propagator.
//!
//! Reads heat and agent presence through the in-tick overlay (`reads()`)
//! to see current-tick diffusion and movement results. Writes reward.

use crate::fields::{AGENT_PRESENCE, HEAT, HEAT_GRADIENT, REWARD};
use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};

/// Reward propagator for RL training.
///
/// Computes per-cell reward:
/// - `reward[i] = heat[i] * heat_bonus + step_cost` if agent present
/// - `reward[i] = 0.0` otherwise
pub struct RewardPropagator {
    heat_bonus: f32,
    step_cost: f32,
}

impl RewardPropagator {
    /// Create a new reward propagator.
    ///
    /// `heat_bonus` scales the heat value at the agent's cell.
    /// `step_cost` is added per step (typically negative for a movement penalty).
    pub fn new(heat_bonus: f32, step_cost: f32) -> Self {
        Self {
            heat_bonus,
            step_cost,
        }
    }
}

impl Propagator for RewardPropagator {
    fn name(&self) -> &str {
        "RewardPropagator"
    }

    fn reads(&self) -> FieldSet {
        [HEAT, AGENT_PRESENCE, HEAT_GRADIENT].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(REWARD, WriteMode::Full)]
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let heat = ctx
            .reads()
            .read(HEAT)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: "heat field not readable".into(),
            })?;

        let presence =
            ctx.reads()
                .read(AGENT_PRESENCE)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: "agent_presence field not readable".into(),
                })?;

        let cell_count = ctx.space().cell_count();
        let heat_copy = heat[..cell_count].to_vec();
        let presence_copy = presence[..cell_count].to_vec();

        let reward =
            ctx.writes()
                .write(REWARD)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: "reward field not writable".into(),
                })?;

        for i in 0..cell_count {
            if presence_copy[i] != 0.0 {
                reward[i] = heat_copy[i] * self.heat_bonus + self.step_cost;
            } else {
                reward[i] = 0.0;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::TickId;
    use murk_propagator::scratch::ScratchRegion;
    use murk_space::{EdgeBehavior, Space, Square4};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    #[test]
    fn reward_computed_for_agent_cells() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = RewardPropagator::new(2.0, -0.1);

        let mut reader = MockFieldReader::new();
        let mut heat = vec![0.0f32; n];
        heat[4] = 10.0; // center has heat
        reader.set_field(HEAT, heat);

        let mut presence = vec![0.0f32; n];
        presence[4] = 1.0; // agent at center
        reader.set_field(AGENT_PRESENCE, presence);

        reader.set_field(HEAT_GRADIENT, vec![0.0f32; n * 2]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(REWARD, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = StepContext::new(
            &reader,
            &reader,
            &mut writer,
            &mut scratch,
            &grid,
            TickId(1),
            0.01,
        );

        prop.step(&mut ctx).unwrap();

        let reward = writer.get_field(REWARD).unwrap();
        assert!(
            (reward[4] - (10.0 * 2.0 + (-0.1))).abs() < 1e-6,
            "reward at agent cell: {}",
            reward[4]
        );
        assert_eq!(reward[0], 0.0, "reward at empty cell should be 0");
    }

    #[test]
    fn reward_zero_for_empty_cells() {
        let grid = Square4::new(2, 2, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = RewardPropagator::new(1.0, -1.0);

        let mut reader = MockFieldReader::new();
        reader.set_field(HEAT, vec![5.0; n]);
        reader.set_field(AGENT_PRESENCE, vec![0.0; n]); // no agents
        reader.set_field(HEAT_GRADIENT, vec![0.0; n * 2]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(REWARD, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = StepContext::new(
            &reader,
            &reader,
            &mut writer,
            &mut scratch,
            &grid,
            TickId(1),
            0.01,
        );

        prop.step(&mut ctx).unwrap();

        let reward = writer.get_field(REWARD).unwrap();
        assert!(reward.iter().all(|&v| v == 0.0));
    }
}
