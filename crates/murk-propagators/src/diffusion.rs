//! Jacobi-style diffusion propagator.
//!
//! Reads heat and velocity from the frozen tick-start view (`reads_previous`)
//! and writes smoothed values plus the heat gradient. Uses a Square4 fast path
//! for direct index arithmetic when available.

use crate::fields::{HEAT, HEAT_GRADIENT, VELOCITY};
use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use murk_space::Square4;

/// Jacobi diffusion propagator for heat and velocity fields.
///
/// Each tick: `heat_new[i] = (1 - α) * heat_prev[i] + α * mean(heat_prev[neighbours])`
/// where `α = diffusivity * dt * num_neighbours`.
///
/// The same kernel is applied per-component for the velocity field.
/// Also computes the central-difference heat gradient.
pub struct DiffusionPropagator {
    diffusivity: f64,
}

impl DiffusionPropagator {
    /// Create a new diffusion propagator with the given diffusivity coefficient.
    pub fn new(diffusivity: f64) -> Self {
        Self { diffusivity }
    }

    fn step_square4(
        &self,
        ctx: &mut StepContext<'_>,
        grid: &Square4,
    ) -> Result<(), PropagatorError> {
        let rows = grid.rows() as usize;
        let cols = grid.cols() as usize;
        let dt = ctx.dt();

        let heat_prev = ctx
            .reads_previous()
            .read(HEAT)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: "heat field not readable".into(),
            })?
            .to_vec();

        let vel_prev = ctx
            .reads_previous()
            .read(VELOCITY)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: "velocity field not readable".into(),
            })?
            .to_vec();

        let heat_out = ctx
            .writes()
            .write(HEAT)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: "heat field not writable".into(),
            })?;

        // Diffuse heat using Square4 index arithmetic
        for r in 0..rows {
            for c in 0..cols {
                let i = r * cols + c;
                let mut sum = 0.0f32;
                let mut count = 0u32;

                if r > 0 {
                    sum += heat_prev[(r - 1) * cols + c];
                    count += 1;
                }
                if r + 1 < rows {
                    sum += heat_prev[(r + 1) * cols + c];
                    count += 1;
                }
                if c > 0 {
                    sum += heat_prev[r * cols + (c - 1)];
                    count += 1;
                }
                if c + 1 < cols {
                    sum += heat_prev[r * cols + (c + 1)];
                    count += 1;
                }

                if count > 0 {
                    let alpha = (self.diffusivity * dt * count as f64) as f32;
                    let mean = sum / count as f32;
                    heat_out[i] = (1.0 - alpha) * heat_prev[i] + alpha * mean;
                } else {
                    heat_out[i] = heat_prev[i];
                }
            }
        }

        // Diffuse velocity (2 components per cell)
        let vel_out = ctx
            .writes()
            .write(VELOCITY)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: "velocity field not writable".into(),
            })?;

        for r in 0..rows {
            for c in 0..cols {
                let i = r * cols + c;
                for comp in 0..2 {
                    let idx = i * 2 + comp;
                    let mut sum = 0.0f32;
                    let mut count = 0u32;

                    if r > 0 {
                        sum += vel_prev[((r - 1) * cols + c) * 2 + comp];
                        count += 1;
                    }
                    if r + 1 < rows {
                        sum += vel_prev[((r + 1) * cols + c) * 2 + comp];
                        count += 1;
                    }
                    if c > 0 {
                        sum += vel_prev[(r * cols + (c - 1)) * 2 + comp];
                        count += 1;
                    }
                    if c + 1 < cols {
                        sum += vel_prev[(r * cols + (c + 1)) * 2 + comp];
                        count += 1;
                    }

                    if count > 0 {
                        let alpha = (self.diffusivity * dt * count as f64) as f32;
                        let mean = sum / count as f32;
                        vel_out[idx] = (1.0 - alpha) * vel_prev[idx] + alpha * mean;
                    } else {
                        vel_out[idx] = vel_prev[idx];
                    }
                }
            }
        }

        // Compute heat gradient using central differences
        let grad_out = ctx
            .writes()
            .write(HEAT_GRADIENT)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: "heat_gradient field not writable".into(),
            })?;

        for r in 0..rows {
            for c in 0..cols {
                let i = r * cols + c;
                let h_east = if c + 1 < cols {
                    heat_prev[r * cols + (c + 1)]
                } else {
                    heat_prev[i]
                };
                let h_west = if c > 0 {
                    heat_prev[r * cols + (c - 1)]
                } else {
                    heat_prev[i]
                };
                let h_south = if r + 1 < rows {
                    heat_prev[(r + 1) * cols + c]
                } else {
                    heat_prev[i]
                };
                let h_north = if r > 0 {
                    heat_prev[(r - 1) * cols + c]
                } else {
                    heat_prev[i]
                };

                grad_out[i * 2] = (h_east - h_west) / 2.0;
                grad_out[i * 2 + 1] = (h_south - h_north) / 2.0;
            }
        }

        Ok(())
    }

    fn step_generic(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let dt = ctx.dt();

        // Precompute spatial topology before taking any mutable borrows
        let ordering = ctx.space().canonical_ordering();
        let cell_count = ordering.len();

        // Precompute neighbour ranks for each cell
        let neighbour_ranks: Vec<Vec<usize>> = ordering
            .iter()
            .map(|coord| {
                let neighbours = ctx.space().neighbours(coord);
                neighbours
                    .iter()
                    .filter_map(|nb| ctx.space().canonical_rank(nb))
                    .collect()
            })
            .collect();

        // Precompute gradient neighbour info: (nb_rank, delta_col, delta_row)
        let grad_info: Vec<Vec<(usize, i32, i32)>> = ordering
            .iter()
            .map(|coord| {
                let neighbours = ctx.space().neighbours(coord);
                neighbours
                    .iter()
                    .filter_map(|nb| {
                        ctx.space().canonical_rank(nb).map(|rank| {
                            let dc = if nb.len() >= 2 { nb[1] - coord[1] } else { 0 };
                            let dr = nb[0] - coord[0];
                            (rank, dc, dr)
                        })
                    })
                    .collect()
            })
            .collect();

        // Copy previous-generation data
        let heat_prev = ctx
            .reads_previous()
            .read(HEAT)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: "heat field not readable".into(),
            })?
            .to_vec();

        let vel_prev = ctx
            .reads_previous()
            .read(VELOCITY)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: "velocity field not readable".into(),
            })?
            .to_vec();

        // Compute outputs into local buffers, then write all at once
        let mut heat_new = vec![0.0f32; cell_count];
        let mut vel_new = vec![0.0f32; cell_count * 2];
        let mut grad_new = vec![0.0f32; cell_count * 2];

        for i in 0..cell_count {
            let nbs = &neighbour_ranks[i];
            let count = nbs.len() as u32;
            if count > 0 {
                let sum: f32 = nbs.iter().map(|&r| heat_prev[r]).sum();
                let alpha = (self.diffusivity * dt * count as f64) as f32;
                let mean = sum / count as f32;
                heat_new[i] = (1.0 - alpha) * heat_prev[i] + alpha * mean;
            } else {
                heat_new[i] = heat_prev[i];
            }

            for comp in 0..2 {
                let idx = i * 2 + comp;
                if count > 0 {
                    let sum: f32 = nbs.iter().map(|&r| vel_prev[r * 2 + comp]).sum();
                    let alpha = (self.diffusivity * dt * count as f64) as f32;
                    let mean = sum / count as f32;
                    vel_new[idx] = (1.0 - alpha) * vel_prev[idx] + alpha * mean;
                } else {
                    vel_new[idx] = vel_prev[idx];
                }
            }

            let gi = &grad_info[i];
            let mut gx = 0.0f32;
            let mut gy = 0.0f32;
            let mut xc = 0u32;
            let mut yc = 0u32;
            for &(rank, dc, dr) in gi {
                let dh = heat_prev[rank] - heat_prev[i];
                if dc != 0 {
                    gx += dh / dc as f32;
                    xc += 1;
                }
                if dr != 0 {
                    gy += dh / dr as f32;
                    yc += 1;
                }
            }
            grad_new[i * 2] = if xc > 0 { gx / xc as f32 } else { 0.0 };
            grad_new[i * 2 + 1] = if yc > 0 { gy / yc as f32 } else { 0.0 };
        }

        // Write results
        let heat_out = ctx
            .writes()
            .write(HEAT)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: "heat field not writable".into(),
            })?;
        heat_out.copy_from_slice(&heat_new);

        let vel_out = ctx
            .writes()
            .write(VELOCITY)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: "velocity field not writable".into(),
            })?;
        vel_out.copy_from_slice(&vel_new);

        let grad_out = ctx
            .writes()
            .write(HEAT_GRADIENT)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: "heat_gradient field not writable".into(),
            })?;
        grad_out.copy_from_slice(&grad_new);

        Ok(())
    }
}

impl Propagator for DiffusionPropagator {
    fn name(&self) -> &str {
        "DiffusionPropagator"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        [HEAT, VELOCITY].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![
            (HEAT, WriteMode::Full),
            (VELOCITY, WriteMode::Full),
            (HEAT_GRADIENT, WriteMode::Full),
        ]
    }

    fn max_dt(&self) -> Option<f64> {
        // CFL stability constraint: dt <= 1 / (4 * D)
        Some(1.0 / (4.0 * self.diffusivity))
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        if let Some(grid) = ctx.space().downcast_ref::<Square4>() {
            // Clone the grid data we need before the mutable borrow
            let rows = grid.rows();
            let cols = grid.cols();
            let grid_copy =
                Square4::new(rows, cols, grid.edge_behavior()).map_err(|e| {
                    PropagatorError::ExecutionFailed {
                        reason: format!("failed to copy grid: {e}"),
                    }
                })?;
            self.step_square4(ctx, &grid_copy)
        } else {
            self.step_generic(ctx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fields::{HEAT, HEAT_GRADIENT, VELOCITY};
    use murk_core::TickId;
    use murk_propagator::scratch::ScratchRegion;
    use murk_space::{EdgeBehavior, Space};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    fn make_ctx<'a>(
        reader: &'a MockFieldReader,
        writer: &'a mut MockFieldWriter,
        scratch: &'a mut ScratchRegion,
        space: &'a Square4,
        dt: f64,
    ) -> StepContext<'a> {
        StepContext::new(reader, reader, writer, scratch, space, TickId(1), dt)
    }

    #[test]
    fn uniform_heat_stays_uniform() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = DiffusionPropagator::new(0.1);

        let mut reader = MockFieldReader::new();
        reader.set_field(HEAT, vec![10.0; n]);
        reader.set_field(VELOCITY, vec![0.0; n * 2]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(HEAT, n);
        writer.add_field(VELOCITY, n * 2);
        writer.add_field(HEAT_GRADIENT, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let heat = writer.get_field(HEAT).unwrap();
        for &v in heat {
            assert!(
                (v - 10.0).abs() < 1e-6,
                "uniform heat should stay uniform, got {v}"
            );
        }
    }

    #[test]
    fn hot_center_spreads() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = DiffusionPropagator::new(0.1);

        let mut heat = vec![0.0f32; n];
        heat[12] = 100.0; // center of 5x5

        let mut reader = MockFieldReader::new();
        reader.set_field(HEAT, heat.clone());
        reader.set_field(VELOCITY, vec![0.0; n * 2]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(HEAT, n);
        writer.add_field(VELOCITY, n * 2);
        writer.add_field(HEAT_GRADIENT, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(HEAT).unwrap();
        // Center should have decreased
        assert!(result[12] < 100.0, "center should cool: {}", result[12]);
        // Neighbours should have increased
        assert!(result[7] > 0.0, "north neighbour should warm: {}", result[7]);
        assert!(
            result[17] > 0.0,
            "south neighbour should warm: {}",
            result[17]
        );
        assert!(
            result[11] > 0.0,
            "west neighbour should warm: {}",
            result[11]
        );
        assert!(
            result[13] > 0.0,
            "east neighbour should warm: {}",
            result[13]
        );
    }

    #[test]
    fn energy_conservation() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = DiffusionPropagator::new(0.1);

        let mut heat = vec![0.0f32; n];
        heat[12] = 100.0;
        let total_before: f32 = heat.iter().sum();

        let mut reader = MockFieldReader::new();
        reader.set_field(HEAT, heat);
        reader.set_field(VELOCITY, vec![0.0; n * 2]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(HEAT, n);
        writer.add_field(VELOCITY, n * 2);
        writer.add_field(HEAT_GRADIENT, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(HEAT).unwrap();
        let total_after: f32 = result.iter().sum();
        assert!(
            (total_before - total_after).abs() < 1e-3,
            "energy not conserved: before={total_before}, after={total_after}"
        );
    }

    #[test]
    fn max_dt_constraint() {
        let prop = DiffusionPropagator::new(0.25);
        assert_eq!(prop.max_dt(), Some(1.0)); // 1 / (4 * 0.25)

        let prop2 = DiffusionPropagator::new(1.0);
        assert_eq!(prop2.max_dt(), Some(0.25)); // 1 / (4 * 1.0)
    }

    #[test]
    fn gradient_correctness() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = DiffusionPropagator::new(0.0); // zero diffusivity to not change heat

        // Linear gradient in x: col 0=0, col 1=10, col 2=20
        let mut heat = vec![0.0f32; n];
        for r in 0..3 {
            for c in 0..3 {
                heat[r * 3 + c] = (c as f32) * 10.0;
            }
        }

        let mut reader = MockFieldReader::new();
        reader.set_field(HEAT, heat);
        reader.set_field(VELOCITY, vec![0.0; n * 2]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(HEAT, n);
        writer.add_field(VELOCITY, n * 2);
        writer.add_field(HEAT_GRADIENT, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let grad = writer.get_field(HEAT_GRADIENT).unwrap();
        // Center cell (1,1): grad_x = (20-0)/2 = 10, grad_y = (10-10)/2 = 0
        let center = 4; // cell (1,1)
        assert!(
            (grad[center * 2] - 10.0).abs() < 1e-6,
            "grad_x at center should be 10, got {}",
            grad[center * 2]
        );
        assert!(
            grad[center * 2 + 1].abs() < 1e-6,
            "grad_y at center should be 0, got {}",
            grad[center * 2 + 1]
        );
    }

    #[test]
    fn boundary_cells_use_self_for_missing_neighbours() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = DiffusionPropagator::new(0.0);

        // All zeros except corner
        let mut heat = vec![0.0f32; n];
        heat[0] = 50.0;

        let mut reader = MockFieldReader::new();
        reader.set_field(HEAT, heat);
        reader.set_field(VELOCITY, vec![0.0; n * 2]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(HEAT, n);
        writer.add_field(VELOCITY, n * 2);
        writer.add_field(HEAT_GRADIENT, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        // Corner (0,0) gradient: east=(0-50)/1 normalized, west=self
        // grad_x = (heat[1] - heat[0]) / 2 = (0-50)/2 = -25
        let grad = writer.get_field(HEAT_GRADIENT).unwrap();
        assert!(
            (grad[0] - (-25.0)).abs() < 1e-6,
            "corner grad_x should be -25, got {}",
            grad[0]
        );
    }
}
