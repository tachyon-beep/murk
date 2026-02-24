//! Jacobi-style diffusion propagator.
//!
//! Reads heat and velocity from the frozen tick-start view (`reads_previous`)
//! and writes smoothed values plus the heat gradient. Uses a Square4 fast path
//! for direct index arithmetic when available.

#[allow(deprecated)]
use crate::fields::{HEAT, HEAT_GRADIENT, VELOCITY};
use crate::grid_helpers::{neighbours_flat, resolve_axis};
use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use murk_space::{EdgeBehavior, Square4};

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
    ///
    /// # Panics
    ///
    /// Panics if `diffusivity` is negative, NaN, or infinite.
    pub fn new(diffusivity: f64) -> Self {
        assert!(
            diffusivity >= 0.0 && diffusivity.is_finite(),
            "diffusivity must be finite and >= 0, got {diffusivity}"
        );
        Self { diffusivity }
    }

    fn step_square4(
        &self,
        ctx: &mut StepContext<'_>,
        rows: u32,
        cols: u32,
        edge: EdgeBehavior,
    ) -> Result<(), PropagatorError> {
        let rows_i = rows as i32;
        let cols_i = cols as i32;
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

        let cell_count = (rows * cols) as usize;
        check_field_arity(&heat_prev, &vel_prev, cell_count)?;

        let heat_out =
            ctx.writes()
                .write(HEAT)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: "heat field not writable".into(),
                })?;

        for r in 0..rows_i {
            for c in 0..cols_i {
                let i = r as usize * cols as usize + c as usize;
                let nbs = neighbours_flat(r, c, rows_i, cols_i, edge);
                let count = nbs.len() as u32;
                if count > 0 {
                    let sum: f32 = nbs.iter().map(|&ni| heat_prev[ni]).sum();
                    let alpha = (self.diffusivity * dt * count as f64).min(1.0) as f32;
                    let mean = sum / count as f32;
                    heat_out[i] = (1.0 - alpha) * heat_prev[i] + alpha * mean;
                } else {
                    heat_out[i] = heat_prev[i];
                }
            }
        }

        let vel_out =
            ctx.writes()
                .write(VELOCITY)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: "velocity field not writable".into(),
                })?;

        for r in 0..rows_i {
            for c in 0..cols_i {
                let i = r as usize * cols as usize + c as usize;
                let nbs = neighbours_flat(r, c, rows_i, cols_i, edge);
                let count = nbs.len() as u32;
                for comp in 0..2 {
                    let idx = i * 2 + comp;
                    if count > 0 {
                        let sum: f32 = nbs.iter().map(|&ni| vel_prev[ni * 2 + comp]).sum();
                        let alpha = (self.diffusivity * dt * count as f64).min(1.0) as f32;
                        let mean = sum / count as f32;
                        vel_out[idx] = (1.0 - alpha) * vel_prev[idx] + alpha * mean;
                    } else {
                        vel_out[idx] = vel_prev[idx];
                    }
                }
            }
        }

        // Compute heat gradient using central differences.
        // For boundary cells, resolve the neighbour per edge behavior;
        // if a direction has no neighbour (Absorb OOB), use self.
        let grad_out =
            ctx.writes()
                .write(HEAT_GRADIENT)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: "heat_gradient field not writable".into(),
                })?;

        for r in 0..rows_i {
            for c in 0..cols_i {
                let i = r as usize * cols as usize + c as usize;

                let h_east = resolve_axis(c + 1, cols_i, edge)
                    .map(|nc| heat_prev[r as usize * cols as usize + nc as usize])
                    .unwrap_or(heat_prev[i]);
                let h_west = resolve_axis(c - 1, cols_i, edge)
                    .map(|nc| heat_prev[r as usize * cols as usize + nc as usize])
                    .unwrap_or(heat_prev[i]);
                let h_south = resolve_axis(r + 1, rows_i, edge)
                    .map(|nr| heat_prev[nr as usize * cols as usize + c as usize])
                    .unwrap_or(heat_prev[i]);
                let h_north = resolve_axis(r - 1, rows_i, edge)
                    .map(|nr| heat_prev[nr as usize * cols as usize + c as usize])
                    .unwrap_or(heat_prev[i]);

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

        // Compute per-axis extents for signed minimal displacement on wrapped
        // topologies. Without this, raw deltas like +3 on a length-4 ring are
        // used instead of the correct -1.
        let ndim = ctx.space().ndim();
        let mut axis_extents = vec![0i32; ndim];
        for coord in &ordering {
            for k in 0..ndim.min(coord.len()) {
                axis_extents[k] = axis_extents[k].max(coord[k] + 1);
            }
        }

        /// Map a raw coordinate delta to the signed minimal displacement on a
        /// periodic axis of the given extent. For non-periodic (extent <= 1)
        /// axes the raw delta is returned unchanged.
        fn signed_delta(raw: i32, extent: i32) -> i32 {
            if extent <= 1 {
                return raw;
            }
            let half = extent / 2;
            if raw > half {
                raw - extent
            } else if raw < -half {
                raw + extent
            } else {
                raw
            }
        }

        // Precompute gradient neighbour info: (nb_rank, delta_col, delta_row)
        let grad_info: Vec<Vec<(usize, i32, i32)>> = ordering
            .iter()
            .map(|coord| {
                let neighbours = ctx.space().neighbours(coord);
                neighbours
                    .iter()
                    .filter_map(|nb| {
                        ctx.space().canonical_rank(nb).map(|rank| {
                            let dc = if nb.len() >= 2 {
                                signed_delta(
                                    nb[1] - coord[1],
                                    axis_extents.get(1).copied().unwrap_or(0),
                                )
                            } else {
                                0
                            };
                            let dr = signed_delta(
                                nb[0] - coord[0],
                                axis_extents.first().copied().unwrap_or(0),
                            );
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

        check_field_arity(&heat_prev, &vel_prev, cell_count)?;

        // Compute outputs into local buffers, then write all at once
        let mut heat_new = vec![0.0f32; cell_count];
        let mut vel_new = vec![0.0f32; cell_count * 2];
        let mut grad_new = vec![0.0f32; cell_count * 2];

        for i in 0..cell_count {
            let nbs = &neighbour_ranks[i];
            let count = nbs.len() as u32;
            if count > 0 {
                let sum: f32 = nbs.iter().map(|&r| heat_prev[r]).sum();
                let alpha = (self.diffusivity * dt * count as f64).min(1.0) as f32;
                let mean = sum / count as f32;
                heat_new[i] = (1.0 - alpha) * heat_prev[i] + alpha * mean;
            } else {
                heat_new[i] = heat_prev[i];
            }

            for comp in 0..2 {
                let idx = i * 2 + comp;
                if count > 0 {
                    let sum: f32 = nbs.iter().map(|&r| vel_prev[r * 2 + comp]).sum();
                    let alpha = (self.diffusivity * dt * count as f64).min(1.0) as f32;
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
        let heat_out =
            ctx.writes()
                .write(HEAT)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: "heat field not writable".into(),
                })?;
        heat_out.copy_from_slice(&heat_new);

        let vel_out =
            ctx.writes()
                .write(VELOCITY)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: "velocity field not writable".into(),
                })?;
        vel_out.copy_from_slice(&vel_new);

        let grad_out =
            ctx.writes()
                .write(HEAT_GRADIENT)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: "heat_gradient field not writable".into(),
                })?;
        grad_out.copy_from_slice(&grad_new);

        Ok(())
    }
}

/// Validate that field slices have the expected lengths for this propagator.
fn check_field_arity(
    heat: &[f32],
    velocity: &[f32],
    cell_count: usize,
) -> Result<(), PropagatorError> {
    if heat.len() != cell_count {
        return Err(PropagatorError::ExecutionFailed {
            reason: format!(
                "heat field length mismatch: expected {cell_count}, got {}",
                heat.len()
            ),
        });
    }
    if velocity.len() != cell_count * 2 {
        return Err(PropagatorError::ExecutionFailed {
            reason: format!(
                "velocity field length mismatch: expected {} (vec2), got {}",
                cell_count * 2,
                velocity.len()
            ),
        });
    }
    Ok(())
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

    fn max_dt(&self, space: &dyn murk_space::Space) -> Option<f64> {
        if self.diffusivity <= 0.0 {
            return None;
        }

        let max_degree = space.max_neighbour_degree();
        if max_degree == 0 {
            return None;
        }

        // CFL stability constraint: dt <= 1 / (max_degree * D).
        Some(1.0 / (max_degree as f64 * self.diffusivity))
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        if let Some(grid) = ctx.space().downcast_ref::<Square4>() {
            // Extract scalars before the mutable borrow
            let rows = grid.rows();
            let cols = grid.cols();
            let edge = grid.edge_behavior();
            self.step_square4(ctx, rows, cols, edge)
        } else {
            self.step_generic(ctx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(deprecated)]
    use crate::fields::{HEAT, HEAT_GRADIENT, VELOCITY};
    use murk_core::TickId;
    use murk_propagator::scratch::ScratchRegion;
    use murk_space::{EdgeBehavior, Fcc12, Space};
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
        assert!(
            result[7] > 0.0,
            "north neighbour should warm: {}",
            result[7]
        );
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
        let space = crate::test_helpers::test_space();
        let prop = DiffusionPropagator::new(0.25);
        // Square4 has degree 4, so 1 / (4 * 0.25) = 1.0.
        let dt = prop.max_dt(&space).unwrap();
        assert!((dt - 1.0).abs() < 1e-10);

        let prop2 = DiffusionPropagator::new(1.0);
        // Square4 has degree 4, so 1 / (4 * 1.0) = 0.25.
        let dt2 = prop2.max_dt(&space).unwrap();
        assert!((dt2 - 1.0 / 4.0).abs() < 1e-10);

        // Fcc12 still resolves to the previous 12-neighbour bound.
        let fcc = Fcc12::new(4, 4, 4, EdgeBehavior::Wrap).unwrap();
        let dt3 = prop2.max_dt(&fcc).unwrap();
        assert!((dt3 - 1.0 / 12.0).abs() < 1e-10);
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

    #[test]
    fn wrap_edge_uniform_stays_uniform() {
        let grid = Square4::new(5, 5, EdgeBehavior::Wrap).unwrap();
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
                "wrap: uniform heat should stay uniform, got {v}"
            );
        }
    }

    #[test]
    fn wrap_edge_corner_has_four_neighbours() {
        // On a Wrap grid, corner (0,0) should have 4 neighbours.
        // A hot corner should diffuse into all 4 wrapped neighbours.
        let grid = Square4::new(4, 4, EdgeBehavior::Wrap).unwrap();
        let n = grid.cell_count();
        let prop = DiffusionPropagator::new(0.1);

        let mut heat = vec![0.0f32; n];
        heat[0] = 100.0; // corner (0,0)

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
        // Corner should have decreased (diffused into 4 neighbours)
        assert!(result[0] < 100.0, "corner should cool: {}", result[0]);

        // Wrapped neighbours: north=(3,0)=12, south=(1,0)=4, west=(0,3)=3, east=(0,1)=1
        assert!(
            result[12] > 0.0,
            "wrapped north (3,0) should warm: {}",
            result[12]
        );
        assert!(result[4] > 0.0, "south (1,0) should warm: {}", result[4]);
        assert!(
            result[3] > 0.0,
            "wrapped west (0,3) should warm: {}",
            result[3]
        );
        assert!(result[1] > 0.0, "east (0,1) should warm: {}", result[1]);

        // Energy conservation
        let total: f32 = result.iter().sum();
        assert!(
            (total - 100.0).abs() < 1e-3,
            "wrap: energy not conserved: {total}"
        );
    }

    #[test]
    fn wrap_energy_conservation() {
        let grid = Square4::new(5, 5, EdgeBehavior::Wrap).unwrap();
        let n = grid.cell_count();
        let prop = DiffusionPropagator::new(0.1);

        let mut heat = vec![0.0f32; n];
        heat[0] = 100.0;
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
            "wrap: energy not conserved: before={total_before}, after={total_after}"
        );
    }

    #[test]
    fn clamp_edge_uniform_stays_uniform() {
        let grid = Square4::new(5, 5, EdgeBehavior::Clamp).unwrap();
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
                "clamp: uniform heat should stay uniform, got {v}"
            );
        }
    }

    #[test]
    fn clamp_corner_has_four_neighbours() {
        // On a Clamp grid, corner (0,0) has 4 neighbours (including self-loops).
        // So alpha = D * dt * 4, and the self-loops include heat[0] twice.
        let grid = Square4::new(4, 4, EdgeBehavior::Clamp).unwrap();
        let n = grid.cell_count();
        let prop = DiffusionPropagator::new(0.1);

        let mut heat = vec![0.0f32; n];
        heat[0] = 100.0;

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
        // Clamp: corner has 4 neighbours [self, self, (1,0), (0,1)]
        // mean = (100 + 100 + 0 + 0) / 4 = 50
        // alpha = 0.1 * 0.01 * 4 = 0.004
        // heat[0] = (1 - 0.004) * 100 + 0.004 * 50 = 99.6 + 0.2 = 99.8
        assert!(
            (result[0] - 99.8).abs() < 1e-4,
            "clamp corner should be ~99.8, got {}",
            result[0]
        );
    }

    #[test]
    fn diffusion_matches_scalar_diffusion() {
        // Prove that ScalarDiffusion produces bit-identical heat + gradient output
        // compared to the old DiffusionPropagator on a 5x5 Absorb grid with a
        // hot center (heat[12] = 100.0).
        use crate::scalar_diffusion::ScalarDiffusion;
        use murk_propagator::propagator::Propagator;

        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let dt = 0.01;

        // --- Initial field data ---
        let mut heat = vec![0.0f32; n];
        heat[12] = 100.0; // hot center

        // --- Run old DiffusionPropagator ---
        let old_prop = DiffusionPropagator::new(0.1);

        let mut old_reader = MockFieldReader::new();
        old_reader.set_field(HEAT, heat.clone());
        old_reader.set_field(VELOCITY, vec![0.0; n * 2]);

        let mut old_writer = MockFieldWriter::new();
        old_writer.add_field(HEAT, n);
        old_writer.add_field(VELOCITY, n * 2);
        old_writer.add_field(HEAT_GRADIENT, n * 2);

        let mut old_scratch = ScratchRegion::new(0);
        let mut old_ctx = make_ctx(&old_reader, &mut old_writer, &mut old_scratch, &grid, dt);

        old_prop.step(&mut old_ctx).unwrap();

        let old_heat = old_writer.get_field(HEAT).unwrap().to_vec();
        let old_grad = old_writer.get_field(HEAT_GRADIENT).unwrap().to_vec();

        // --- Run new ScalarDiffusion ---
        let new_prop = ScalarDiffusion::builder()
            .input_field(HEAT)
            .output_field(HEAT)
            .coefficient(0.1)
            .gradient_field(HEAT_GRADIENT)
            .build()
            .unwrap();

        let mut new_reader = MockFieldReader::new();
        new_reader.set_field(HEAT, heat);

        let mut new_writer = MockFieldWriter::new();
        new_writer.add_field(HEAT, n);
        new_writer.add_field(HEAT_GRADIENT, n * 2);

        let mut new_scratch = ScratchRegion::new(0);
        let mut new_ctx = StepContext::new(
            &new_reader,
            &new_reader,
            &mut new_writer,
            &mut new_scratch,
            &grid,
            TickId(1),
            dt,
        );

        new_prop.step(&mut new_ctx).unwrap();

        let new_heat = new_writer.get_field(HEAT).unwrap();
        let new_grad = new_writer.get_field(HEAT_GRADIENT).unwrap();

        // --- Compare heat output ---
        assert_eq!(
            old_heat.len(),
            new_heat.len(),
            "heat buffer length mismatch"
        );
        for i in 0..old_heat.len() {
            assert!(
                (old_heat[i] - new_heat[i]).abs() < 1e-6,
                "heat mismatch at cell {i}: old={}, new={}",
                old_heat[i],
                new_heat[i]
            );
        }

        // --- Compare gradient output ---
        assert_eq!(
            old_grad.len(),
            new_grad.len(),
            "gradient buffer length mismatch"
        );
        for i in 0..old_grad.len() {
            assert!(
                (old_grad[i] - new_grad[i]).abs() < 1e-6,
                "gradient mismatch at component {i}: old={}, new={}",
                old_grad[i],
                new_grad[i]
            );
        }
    }

    #[test]
    fn wrap_gradient_at_boundary() {
        // On a 4x4 Wrap grid with linear-x heat, the gradient at (0,0)
        // should wrap around to see heat at (0,3).
        let grid = Square4::new(4, 4, EdgeBehavior::Wrap).unwrap();
        let n = grid.cell_count();
        let prop = DiffusionPropagator::new(0.0);

        let mut heat = vec![0.0f32; n];
        for r in 0..4 {
            for c in 0..4 {
                heat[r * 4 + c] = (c as f32) * 10.0;
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
        // Cell (0,0): east=(0,1)=10, west=wrap to (0,3)=30
        // grad_x = (10 - 30) / 2 = -10
        assert!(
            (grad[0] - (-10.0)).abs() < 1e-6,
            "wrap grad_x at (0,0) should be -10, got {}",
            grad[0]
        );
    }

    #[test]
    fn generic_path_wrap_gradient_sign() {
        // BUG-101: step_generic computed gradient deltas as raw nb[k] - coord[k],
        // which is wrong on wrapped axes. On Ring1D(4) with heat = [0, 10, 20, 30],
        // cell 0's left neighbour is cell 3. Raw delta = 3 - 0 = +3, but the
        // correct signed displacement is -1. The gradient dh/dr at cell 0 should
        // reflect that cell 3 (heat=30) is one step to the LEFT.
        use murk_space::Ring1D;

        let ring = Ring1D::new(4).unwrap();
        let n = ring.cell_count(); // 4
        let prop = DiffusionPropagator::new(0.0); // zero diffusion, only test gradient

        // Linear heat ramp: [0, 10, 20, 30]
        let heat: Vec<f32> = (0..n).map(|i| i as f32 * 10.0).collect();

        let mut reader = MockFieldReader::new();
        reader.set_field(HEAT, heat);
        reader.set_field(VELOCITY, vec![0.0; n * 2]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(HEAT, n);
        writer.add_field(VELOCITY, n * 2);
        writer.add_field(HEAT_GRADIENT, n * 2);

        let mut scratch = ScratchRegion::new(0);
        // Ring1D is 1D so only grad_y (row component) is meaningful; grad_x = 0.
        let mut ctx = StepContext::new(
            &reader,
            &reader,
            &mut writer,
            &mut scratch,
            &ring,
            TickId(1),
            0.01,
        );

        prop.step(&mut ctx).unwrap();

        let grad = writer.get_field(HEAT_GRADIENT).unwrap();

        // Cell 0: neighbours are cell 1 (heat=10, dr=+1) and cell 3 (heat=30, dr=-1).
        // dh/dr from cell 1: (10 - 0) / 1 = 10
        // dh/dr from cell 3: (30 - 0) / -1 = -30
        // Average: (10 + (-30)) / 2 = -10
        let grad_y_cell0 = grad[0 * 2 + 1];
        assert!(
            (grad_y_cell0 - (-10.0)).abs() < 1e-4,
            "gradient at cell 0 should be -10 (wrap boundary), got {grad_y_cell0}"
        );

        // Cell 1 (interior): neighbours are cell 0 (dr=-1) and cell 2 (dr=+1).
        // dh/dr from cell 0: (0 - 10) / -1 = 10
        // dh/dr from cell 2: (20 - 10) / 1 = 10
        // Average: 10
        let grad_y_cell1 = grad[1 * 2 + 1];
        assert!(
            (grad_y_cell1 - 10.0).abs() < 1e-4,
            "gradient at cell 1 should be 10, got {grad_y_cell1}"
        );

        // All grad_x components should be zero (1D space, no column axis).
        for i in 0..n {
            assert!(
                grad[i * 2].abs() < 1e-6,
                "grad_x at cell {i} should be 0, got {}",
                grad[i * 2]
            );
        }
    }

    #[test]
    fn wrong_velocity_arity_returns_error() {
        // Velocity should be vec2 (2 * cell_count) but we provide scalar (1 * cell_count).
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = DiffusionPropagator::new(0.1);

        let mut reader = MockFieldReader::new();
        reader.set_field(HEAT, vec![0.0; n]);
        reader.set_field(VELOCITY, vec![0.0; n]); // Wrong: should be n*2

        let mut writer = MockFieldWriter::new();
        writer.add_field(HEAT, n);
        writer.add_field(VELOCITY, n); // Wrong arity
        writer.add_field(HEAT_GRADIENT, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        let result = prop.step(&mut ctx);
        assert!(result.is_err(), "expected PropagatorError for wrong velocity arity, got Ok");
    }

    #[test]
    #[should_panic(expected = "diffusivity must be finite")]
    fn rejects_negative_diffusivity() {
        DiffusionPropagator::new(-1.0);
    }

    #[test]
    #[should_panic(expected = "diffusivity must be finite")]
    fn rejects_nan_diffusivity() {
        DiffusionPropagator::new(f64::NAN);
    }

    #[test]
    #[should_panic(expected = "diffusivity must be finite")]
    fn rejects_infinite_diffusivity() {
        DiffusionPropagator::new(f64::INFINITY);
    }

    #[test]
    fn zero_diffusivity_max_dt_is_none() {
        let space = Square4::new(4, 4, EdgeBehavior::Wrap).unwrap();
        let prop = DiffusionPropagator::new(0.0);
        assert!(
            prop.max_dt(&space).is_none(),
            "zero diffusivity should give max_dt=None, got {:?}",
            prop.max_dt(&space)
        );
    }

    #[test]
    fn alpha_clamped_prevents_sign_inversion() {
        // Even if dt is larger than CFL allows (simulating a bypass),
        // alpha should be clamped so values don't go negative.
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = DiffusionPropagator::new(10.0); // very high diffusivity

        let mut heat = vec![0.0f32; n];
        heat[4] = 100.0; // center of 3x3

        let mut reader = MockFieldReader::new();
        reader.set_field(HEAT, heat);
        reader.set_field(VELOCITY, vec![0.0; n * 2]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(HEAT, n);
        writer.add_field(VELOCITY, n * 2);
        writer.add_field(HEAT_GRADIENT, n * 2);

        let mut scratch = ScratchRegion::new(0);
        // dt=1.0 with diffusivity=10.0: alpha = 10*1*4 = 40 >> 1
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1.0);

        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(HEAT).unwrap();
        // With alpha clamped to 1.0, center becomes the mean of neighbours = 0.0
        // Without clamping: (1-40)*100 + 40*0 = -3900 (catastrophic!)
        for (i, &v) in result.iter().enumerate() {
            assert!(v >= 0.0, "cell {i} went negative ({v}): alpha clamp failed");
        }
    }
}
