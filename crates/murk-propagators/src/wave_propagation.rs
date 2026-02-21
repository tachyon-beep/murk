//! Second-order wave equation propagator.
//!
//! Produces propagating wavefronts, reflection off boundaries, and
//! interference patterns — qualitatively different from diffusion.
//! Requires two scalar fields: displacement and velocity.
//!
//! Uses leapfrog (symplectic Euler) integration:
//! ```text
//! laplacian[i] = mean(neighbours) - displacement[i]
//! acceleration[i] = wave_speed² * laplacian[i] - damping * velocity[i]
//! new_velocity[i] = velocity[i] + acceleration[i] * dt
//! new_displacement[i] = displacement[i] + new_velocity[i] * dt
//! ```
//!
//! Has a [`Square4`] fast path and a generic fallback.
//! Implements `max_dt()` for CFL stability.
//!
//! Constructed via the builder pattern: [`WavePropagation::builder`].

use crate::grid_helpers::neighbours_flat;
use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use murk_space::{EdgeBehavior, Square4};

/// A second-order wave equation propagator.
///
/// Models wave dynamics on a discrete spatial grid. Produces propagating
/// wavefronts, boundary reflections, and interference patterns.
///
/// # CFL stability
///
/// The maximum stable timestep is `1 / (wave_speed * sqrt(max_degree))`.
/// For worst-case FCC-12 topology: `1 / (wave_speed * sqrt(12))`.
#[derive(Debug)]
pub struct WavePropagation {
    displacement_field: FieldId,
    velocity_field: FieldId,
    wave_speed: f64,
    damping: f64,
}

/// Builder for [`WavePropagation`].
///
/// Required fields: `displacement_field` and `velocity_field`.
pub struct WavePropagationBuilder {
    displacement_field: Option<FieldId>,
    velocity_field: Option<FieldId>,
    wave_speed: f64,
    damping: f64,
}

impl WavePropagation {
    /// Create a new builder for configuring a `WavePropagation` propagator.
    pub fn builder() -> WavePropagationBuilder {
        WavePropagationBuilder {
            displacement_field: None,
            velocity_field: None,
            wave_speed: 1.0,
            damping: 0.0,
        }
    }

    /// Square4 fast path.
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
        let c2 = (self.wave_speed * self.wave_speed) as f32;
        let damp = self.damping as f32;
        let dt_f32 = dt as f32;

        let prev_d = ctx
            .reads_previous()
            .read(self.displacement_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!(
                    "displacement field {:?} not readable",
                    self.displacement_field
                ),
            })?
            .to_vec();

        let prev_v = ctx
            .reads_previous()
            .read(self.velocity_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("velocity field {:?} not readable", self.velocity_field),
            })?
            .to_vec();

        // Compute new values into local buffers (avoids two concurrent write borrows)
        let n = (rows as usize) * (cols as usize);
        let mut new_d = vec![0.0f32; n];
        let mut new_v = vec![0.0f32; n];

        for r in 0..rows_i {
            for c in 0..cols_i {
                let i = r as usize * cols as usize + c as usize;
                let nbs = neighbours_flat(r, c, rows_i, cols_i, edge);
                let count = nbs.len();
                let laplacian = if count > 0 {
                    let sum: f32 = nbs.iter().map(|&ni| prev_d[ni]).sum();
                    sum / count as f32 - prev_d[i]
                } else {
                    0.0
                };
                let accel = c2 * laplacian - damp * prev_v[i];
                new_v[i] = prev_v[i] + accel * dt_f32;
                new_d[i] = prev_d[i] + new_v[i] * dt_f32;
            }
        }

        let out_d = ctx.writes().write(self.displacement_field).ok_or_else(|| {
            PropagatorError::ExecutionFailed {
                reason: format!(
                    "displacement field {:?} not writable",
                    self.displacement_field
                ),
            }
        })?;
        out_d.copy_from_slice(&new_d);

        let out_v = ctx.writes().write(self.velocity_field).ok_or_else(|| {
            PropagatorError::ExecutionFailed {
                reason: format!("velocity field {:?} not writable", self.velocity_field),
            }
        })?;
        out_v.copy_from_slice(&new_v);

        Ok(())
    }

    /// Generic fallback.
    fn step_generic(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let dt = ctx.dt();
        let c2 = (self.wave_speed * self.wave_speed) as f32;
        let damp = self.damping as f32;
        let dt_f32 = dt as f32;

        let ordering = ctx.space().canonical_ordering();
        let cell_count = ordering.len();

        let neighbour_ranks: Vec<Vec<usize>> = ordering
            .iter()
            .map(|coord| {
                ctx.space()
                    .neighbours(coord)
                    .iter()
                    .filter_map(|nb| ctx.space().canonical_rank(nb))
                    .collect()
            })
            .collect();

        let prev_d = ctx
            .reads_previous()
            .read(self.displacement_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!(
                    "displacement field {:?} not readable",
                    self.displacement_field
                ),
            })?
            .to_vec();

        let prev_v = ctx
            .reads_previous()
            .read(self.velocity_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("velocity field {:?} not readable", self.velocity_field),
            })?
            .to_vec();

        let mut new_d = vec![0.0f32; cell_count];
        let mut new_v = vec![0.0f32; cell_count];

        for i in 0..cell_count {
            let nbs = &neighbour_ranks[i];
            let count = nbs.len();
            let laplacian = if count > 0 {
                let sum: f32 = nbs.iter().map(|&r| prev_d[r]).sum();
                sum / count as f32 - prev_d[i]
            } else {
                0.0
            };
            let accel = c2 * laplacian - damp * prev_v[i];
            new_v[i] = prev_v[i] + accel * dt_f32;
            new_d[i] = prev_d[i] + new_v[i] * dt_f32;
        }

        let out_d = ctx.writes().write(self.displacement_field).ok_or_else(|| {
            PropagatorError::ExecutionFailed {
                reason: format!(
                    "displacement field {:?} not writable",
                    self.displacement_field
                ),
            }
        })?;
        out_d.copy_from_slice(&new_d);

        let out_v = ctx.writes().write(self.velocity_field).ok_or_else(|| {
            PropagatorError::ExecutionFailed {
                reason: format!("velocity field {:?} not writable", self.velocity_field),
            }
        })?;
        out_v.copy_from_slice(&new_v);

        Ok(())
    }
}

impl WavePropagationBuilder {
    /// Set the displacement field (read previous, write current).
    pub fn displacement_field(mut self, field: FieldId) -> Self {
        self.displacement_field = Some(field);
        self
    }

    /// Set the velocity field (read previous, write current).
    pub fn velocity_field(mut self, field: FieldId) -> Self {
        self.velocity_field = Some(field);
        self
    }

    /// Set the wave propagation speed (default: 1.0). Must be > 0.
    pub fn wave_speed(mut self, speed: f64) -> Self {
        self.wave_speed = speed;
        self
    }

    /// Set the damping coefficient (default: 0.0). Must be >= 0.
    pub fn damping(mut self, damping: f64) -> Self {
        self.damping = damping;
        self
    }

    /// Build the propagator, validating all configuration.
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - `displacement_field` is not set
    /// - `velocity_field` is not set
    /// - `wave_speed` is not > 0 or is NaN
    /// - `damping` is negative or NaN
    pub fn build(self) -> Result<WavePropagation, String> {
        let displacement_field = self
            .displacement_field
            .ok_or_else(|| "displacement_field is required".to_string())?;
        let velocity_field = self
            .velocity_field
            .ok_or_else(|| "velocity_field is required".to_string())?;

        if !(self.wave_speed > 0.0) {
            return Err(format!(
                "wave_speed must be finite and > 0, got {}",
                self.wave_speed
            ));
        }
        if !(self.damping >= 0.0) {
            return Err(format!(
                "damping must be finite and >= 0, got {}",
                self.damping
            ));
        }

        Ok(WavePropagation {
            displacement_field,
            velocity_field,
            wave_speed: self.wave_speed,
            damping: self.damping,
        })
    }
}

impl Propagator for WavePropagation {
    fn name(&self) -> &str {
        "WavePropagation"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        [self.displacement_field, self.velocity_field]
            .into_iter()
            .collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![
            (self.displacement_field, WriteMode::Full),
            (self.velocity_field, WriteMode::Full),
        ]
    }

    fn max_dt(&self) -> Option<f64> {
        // CFL: dt <= 1 / (wave_speed * sqrt(max_degree))
        // Worst case: FCC-12 with degree 12.
        Some(1.0 / (self.wave_speed * 12.0_f64.sqrt()))
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        if let Some(grid) = ctx.space().downcast_ref::<Square4>() {
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
    use murk_core::TickId;
    use murk_propagator::scratch::ScratchRegion;
    use murk_space::{EdgeBehavior, Space};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    const F_DISP: FieldId = FieldId(100);
    const F_VEL: FieldId = FieldId(101);

    fn make_ctx<'a>(
        reader: &'a MockFieldReader,
        writer: &'a mut MockFieldWriter,
        scratch: &'a mut ScratchRegion,
        space: &'a Square4,
        dt: f64,
    ) -> StepContext<'a> {
        StepContext::new(reader, reader, writer, scratch, space, TickId(1), dt)
    }

    // ---------------------------------------------------------------
    // Builder tests
    // ---------------------------------------------------------------

    #[test]
    fn builder_minimal() {
        let prop = WavePropagation::builder()
            .displacement_field(F_DISP)
            .velocity_field(F_VEL)
            .build()
            .unwrap();

        assert_eq!(prop.name(), "WavePropagation");
        assert!(prop.reads().is_empty(), "reads() should be empty");

        let rp = prop.reads_previous();
        assert!(rp.contains(F_DISP));
        assert!(rp.contains(F_VEL));

        let w = prop.writes();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0], (F_DISP, WriteMode::Full));
        assert_eq!(w[1], (F_VEL, WriteMode::Full));

        // CFL check for default wave_speed=1.0
        let expected_dt = 1.0 / 12.0_f64.sqrt();
        let actual_dt = prop.max_dt().unwrap();
        assert!((actual_dt - expected_dt).abs() < 1e-10);
    }

    #[test]
    fn builder_rejects_missing_displacement() {
        let result = WavePropagation::builder().velocity_field(F_VEL).build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("displacement_field"));
    }

    #[test]
    fn builder_rejects_missing_velocity() {
        let result = WavePropagation::builder()
            .displacement_field(F_DISP)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("velocity_field"));
    }

    #[test]
    fn builder_rejects_zero_wave_speed() {
        let result = WavePropagation::builder()
            .displacement_field(F_DISP)
            .velocity_field(F_VEL)
            .wave_speed(0.0)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("wave_speed"));
    }

    #[test]
    fn builder_rejects_nan_wave_speed() {
        let result = WavePropagation::builder()
            .displacement_field(F_DISP)
            .velocity_field(F_VEL)
            .wave_speed(f64::NAN)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("wave_speed"));
    }

    #[test]
    fn builder_rejects_negative_damping() {
        let result = WavePropagation::builder()
            .displacement_field(F_DISP)
            .velocity_field(F_VEL)
            .damping(-0.1)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("damping"));
    }

    #[test]
    fn max_dt_is_cfl() {
        let prop = WavePropagation::builder()
            .displacement_field(F_DISP)
            .velocity_field(F_VEL)
            .wave_speed(2.0)
            .build()
            .unwrap();
        let expected = 1.0 / (2.0 * 12.0_f64.sqrt());
        let actual = prop.max_dt().unwrap();
        assert!(
            (actual - expected).abs() < 1e-10,
            "CFL: expected {expected}, got {actual}"
        );
    }

    // ---------------------------------------------------------------
    // Step logic tests
    // ---------------------------------------------------------------

    #[test]
    fn zero_initial_stays_zero() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = WavePropagation::builder()
            .displacement_field(F_DISP)
            .velocity_field(F_VEL)
            .wave_speed(1.0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_DISP, vec![0.0; n]);
        reader.set_field(F_VEL, vec![0.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_DISP, n);
        writer.add_field(F_VEL, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop.step(&mut ctx).unwrap();

        let disp = writer.get_field(F_DISP).unwrap();
        let vel = writer.get_field(F_VEL).unwrap();
        assert!(disp.iter().all(|&v| v == 0.0), "zero stays zero");
        assert!(vel.iter().all(|&v| v == 0.0), "zero stays zero");
    }

    #[test]
    fn impulse_propagates() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = WavePropagation::builder()
            .displacement_field(F_DISP)
            .velocity_field(F_VEL)
            .wave_speed(1.0)
            .build()
            .unwrap();

        // Displacement impulse at center (cell 12)
        let mut disp = vec![0.0f32; n];
        disp[12] = 10.0;

        let mut reader = MockFieldReader::new();
        reader.set_field(F_DISP, disp);
        reader.set_field(F_VEL, vec![0.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_DISP, n);
        writer.add_field(F_VEL, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop.step(&mut ctx).unwrap();

        let vel = writer.get_field(F_VEL).unwrap();
        // Center should get negative velocity (restoring force pulls it back)
        assert!(
            vel[12] < 0.0,
            "center velocity should be negative, got {}",
            vel[12]
        );
        // Neighbors should get positive velocity (wave spreading outward)
        assert!(
            vel[7] > 0.0,
            "north neighbor should get positive velocity, got {}",
            vel[7]
        );
    }

    #[test]
    fn damping_reduces_energy() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let make_prop = |damping: f64| {
            WavePropagation::builder()
                .displacement_field(F_DISP)
                .velocity_field(F_VEL)
                .wave_speed(1.0)
                .damping(damping)
                .build()
                .unwrap()
        };

        let disp = vec![0.0f32; n];
        // Give the system non-zero initial velocity so damping has something to act on.
        let mut vel = vec![0.0f32; n];
        vel[12] = 5.0;

        // Run undamped
        let prop_undamped = make_prop(0.0);
        let mut reader = MockFieldReader::new();
        reader.set_field(F_DISP, disp.clone());
        reader.set_field(F_VEL, vel.clone());
        let mut writer = MockFieldWriter::new();
        writer.add_field(F_DISP, n);
        writer.add_field(F_VEL, n);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop_undamped.step(&mut ctx).unwrap();
        let undamped_energy: f32 = writer.get_field(F_VEL).unwrap().iter().map(|v| v * v).sum();

        // Run damped
        let prop_damped = make_prop(5.0);
        let mut reader2 = MockFieldReader::new();
        reader2.set_field(F_DISP, disp);
        reader2.set_field(F_VEL, vel);
        let mut writer2 = MockFieldWriter::new();
        writer2.add_field(F_DISP, n);
        writer2.add_field(F_VEL, n);
        let mut scratch2 = ScratchRegion::new(0);
        let mut ctx2 = make_ctx(&reader2, &mut writer2, &mut scratch2, &grid, 0.01);
        prop_damped.step(&mut ctx2).unwrap();
        let damped_energy: f32 = writer2
            .get_field(F_VEL)
            .unwrap()
            .iter()
            .map(|v| v * v)
            .sum();

        assert!(
            damped_energy < undamped_energy,
            "damped energy ({damped_energy}) should be less than undamped ({undamped_energy})"
        );
    }
}
