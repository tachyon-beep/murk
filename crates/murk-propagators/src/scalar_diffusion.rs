//! Parameterized Jacobi-style scalar diffusion propagator.
//!
//! A generalization of [`DiffusionPropagator`](crate::DiffusionPropagator) that
//! operates on arbitrary [`FieldId`]s instead of hardcoded constants. Supports
//! optional exponential decay, fixed-value sources, value clamping, and
//! central-difference gradient output.
//!
//! Constructed via the builder pattern: [`ScalarDiffusion::builder`].

use crate::grid_helpers::{neighbours_flat, resolve_axis};
use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use murk_space::{EdgeBehavior, Square4};

/// A parameterized Jacobi-style scalar diffusion propagator.
///
/// Each tick computes:
/// ```text
/// out[i] = (1 - alpha) * prev[i] + alpha * mean(prev[neighbours])
/// ```
/// where `alpha = coefficient * dt * num_neighbours`.
///
/// Optionally applies exponential decay, fixed-value sources, and value clamping.
/// If a gradient field is configured, computes central-difference gradients of
/// the **previous-tick** (pre-diffusion) values into a 2-component vector field.
/// This is consistent with the Jacobi stencil: all reads come from the frozen
/// tick-start snapshot.
///
/// # Construction
///
/// Use the builder pattern:
///
/// ```
/// use murk_core::FieldId;
/// use murk_propagators::ScalarDiffusion;
///
/// let prop = ScalarDiffusion::builder()
///     .input_field(FieldId(10))
///     .output_field(FieldId(11))
///     .coefficient(0.1)
///     .build()
///     .unwrap();
/// ```
#[derive(Debug)]
pub struct ScalarDiffusion {
    input_field: FieldId,
    output_field: FieldId,
    gradient_field: Option<FieldId>,
    coefficient: f64,
    decay: f64,
    sources: Vec<(usize, f32)>,
    clamp_min: Option<f32>,
    clamp_max: Option<f32>,
}

/// Builder for [`ScalarDiffusion`].
///
/// Required fields: `input_field` and `output_field`. All others have sensible
/// defaults (zero coefficient, zero decay, no sources, no clamping, no gradient).
pub struct ScalarDiffusionBuilder {
    input_field: Option<FieldId>,
    output_field: Option<FieldId>,
    gradient_field: Option<FieldId>,
    coefficient: f64,
    decay: f64,
    sources: Vec<(usize, f32)>,
    clamp_min: Option<f32>,
    clamp_max: Option<f32>,
}

impl ScalarDiffusion {
    /// Create a new builder for configuring a `ScalarDiffusion` propagator.
    pub fn builder() -> ScalarDiffusionBuilder {
        ScalarDiffusionBuilder {
            input_field: None,
            output_field: None,
            gradient_field: None,
            coefficient: 0.0,
            decay: 0.0,
            sources: Vec::new(),
            clamp_min: None,
            clamp_max: None,
        }
    }

    /// Apply decay, sources, and clamping to a mutable output buffer.
    fn apply_post_processing(&self, out: &mut [f32], dt: f64) {
        // True exponential decay: v *= exp(-decay * dt)
        // Safe for all dt values (never goes negative or inverts sign).
        if self.decay > 0.0 {
            let decay_factor = (-(self.decay * dt)).exp() as f32;
            for v in out.iter_mut() {
                *v *= decay_factor;
            }
        }

        // Fixed sources
        for &(idx, val) in &self.sources {
            if idx < out.len() {
                out[idx] = val;
            }
        }

        // Clamping
        match (self.clamp_min, self.clamp_max) {
            (Some(lo), Some(hi)) => {
                for v in out.iter_mut() {
                    *v = v.clamp(lo, hi);
                }
            }
            (Some(lo), None) => {
                for v in out.iter_mut() {
                    if *v < lo {
                        *v = lo;
                    }
                }
            }
            (None, Some(hi)) => {
                for v in out.iter_mut() {
                    if *v > hi {
                        *v = hi;
                    }
                }
            }
            (None, None) => {}
        }
    }

    /// Square4 fast path: direct index arithmetic for 2D grids.
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

        let prev = ctx
            .reads_previous()
            .read(self.input_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("input field {:?} not readable", self.input_field),
            })?
            .to_vec();

        let out = ctx
            .writes()
            .write(self.output_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("output field {:?} not writable", self.output_field),
            })?;

        for r in 0..rows_i {
            for c in 0..cols_i {
                let i = r as usize * cols as usize + c as usize;
                let nbs = neighbours_flat(r, c, rows_i, cols_i, edge);
                let count = nbs.len() as u32;
                if count > 0 {
                    let sum: f32 = nbs.iter().map(|&ni| prev[ni]).sum();
                    let alpha = (self.coefficient * dt * count as f64).min(1.0) as f32;
                    let mean = sum / count as f32;
                    out[i] = (1.0 - alpha) * prev[i] + alpha * mean;
                } else {
                    out[i] = prev[i];
                }
            }
        }

        // Apply decay, sources, clamping
        self.apply_post_processing(out, dt);

        // Compute gradient if requested
        if let Some(grad_field) = self.gradient_field {
            let grad_out =
                ctx.writes()
                    .write(grad_field)
                    .ok_or_else(|| PropagatorError::ExecutionFailed {
                        reason: format!("gradient field {:?} not writable", grad_field),
                    })?;

            for r in 0..rows_i {
                for c in 0..cols_i {
                    let i = r as usize * cols as usize + c as usize;

                    let h_east = resolve_axis(c + 1, cols_i, edge)
                        .map(|nc| prev[r as usize * cols as usize + nc as usize])
                        .unwrap_or(prev[i]);
                    let h_west = resolve_axis(c - 1, cols_i, edge)
                        .map(|nc| prev[r as usize * cols as usize + nc as usize])
                        .unwrap_or(prev[i]);
                    let h_south = resolve_axis(r + 1, rows_i, edge)
                        .map(|nr| prev[nr as usize * cols as usize + c as usize])
                        .unwrap_or(prev[i]);
                    let h_north = resolve_axis(r - 1, rows_i, edge)
                        .map(|nr| prev[nr as usize * cols as usize + c as usize])
                        .unwrap_or(prev[i]);

                    grad_out[i * 2] = (h_east - h_west) / 2.0;
                    grad_out[i * 2 + 1] = (h_south - h_north) / 2.0;
                }
            }
        }

        Ok(())
    }

    /// Generic fallback using `Space::canonical_ordering()`.
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

        // Copy previous-generation data
        let prev = ctx
            .reads_previous()
            .read(self.input_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("input field {:?} not readable", self.input_field),
            })?
            .to_vec();

        // Compute diffusion into local buffer
        let mut out_buf = vec![0.0f32; cell_count];

        for i in 0..cell_count {
            let nbs = &neighbour_ranks[i];
            let count = nbs.len() as u32;
            if count > 0 {
                let sum: f32 = nbs.iter().map(|&r| prev[r]).sum();
                let alpha = (self.coefficient * dt * count as f64).min(1.0) as f32;
                let mean = sum / count as f32;
                out_buf[i] = (1.0 - alpha) * prev[i] + alpha * mean;
            } else {
                out_buf[i] = prev[i];
            }
        }

        // Apply decay, sources, clamping
        self.apply_post_processing(&mut out_buf, dt);

        // Write diffused output
        let out = ctx
            .writes()
            .write(self.output_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("output field {:?} not writable", self.output_field),
            })?;
        out.copy_from_slice(&out_buf);

        // Compute gradient if requested
        if let Some(grad_field) = self.gradient_field {
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

            let mut grad_buf = vec![0.0f32; cell_count * 2];

            for i in 0..cell_count {
                let gi = &grad_info[i];
                let mut gx = 0.0f32;
                let mut gy = 0.0f32;
                let mut xc = 0u32;
                let mut yc = 0u32;
                for &(rank, dc, dr) in gi {
                    let dh = prev[rank] - prev[i];
                    if dc != 0 {
                        gx += dh / dc as f32;
                        xc += 1;
                    }
                    if dr != 0 {
                        gy += dh / dr as f32;
                        yc += 1;
                    }
                }
                grad_buf[i * 2] = if xc > 0 { gx / xc as f32 } else { 0.0 };
                grad_buf[i * 2 + 1] = if yc > 0 { gy / yc as f32 } else { 0.0 };
            }

            let grad_out =
                ctx.writes()
                    .write(grad_field)
                    .ok_or_else(|| PropagatorError::ExecutionFailed {
                        reason: format!("gradient field {:?} not writable", grad_field),
                    })?;
            grad_out.copy_from_slice(&grad_buf);
        }

        Ok(())
    }
}

impl ScalarDiffusionBuilder {
    /// Set the input field to read from the previous tick.
    pub fn input_field(mut self, field: FieldId) -> Self {
        self.input_field = Some(field);
        self
    }

    /// Set the output field to write diffused values into.
    pub fn output_field(mut self, field: FieldId) -> Self {
        self.output_field = Some(field);
        self
    }

    /// Set the optional gradient output field (2-component vector).
    pub fn gradient_field(mut self, field: FieldId) -> Self {
        self.gradient_field = Some(field);
        self
    }

    /// Set the diffusion coefficient (default 0.0). Must be >= 0.
    pub fn coefficient(mut self, c: f64) -> Self {
        self.coefficient = c;
        self
    }

    /// Set the exponential decay rate per tick (default 0.0). Must be >= 0.
    pub fn decay(mut self, d: f64) -> Self {
        self.decay = d;
        self
    }

    /// Add a fixed-value source cell. Each tick, `cells[idx]` is reset to `value`.
    pub fn source(mut self, idx: usize, value: f32) -> Self {
        self.sources.push((idx, value));
        self
    }

    /// Set all fixed-value source cells at once.
    pub fn sources(mut self, sources: Vec<(usize, f32)>) -> Self {
        self.sources = sources;
        self
    }

    /// Set the minimum clamp value.
    pub fn clamp_min(mut self, min: f32) -> Self {
        self.clamp_min = Some(min);
        self
    }

    /// Set the maximum clamp value.
    pub fn clamp_max(mut self, max: f32) -> Self {
        self.clamp_max = Some(max);
        self
    }

    /// Build the propagator, validating all configuration.
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - `input_field` is not set
    /// - `output_field` is not set
    /// - `coefficient` is negative
    /// - `decay` is negative
    /// - `clamp_min > clamp_max` (when both are set)
    pub fn build(self) -> Result<ScalarDiffusion, String> {
        let input_field = self
            .input_field
            .ok_or_else(|| "input_field is required".to_string())?;
        let output_field = self
            .output_field
            .ok_or_else(|| "output_field is required".to_string())?;

        if !(self.coefficient >= 0.0) || !self.coefficient.is_finite() {
            return Err(format!(
                "coefficient must be finite and >= 0, got {}",
                self.coefficient
            ));
        }
        if !(self.decay >= 0.0) || !self.decay.is_finite() {
            return Err(format!(
                "decay must be finite and >= 0, got {}",
                self.decay
            ));
        }
        if let (Some(lo), Some(hi)) = (self.clamp_min, self.clamp_max) {
            if lo > hi {
                return Err(format!(
                    "clamp_min ({lo}) must be <= clamp_max ({hi})"
                ));
            }
        }

        Ok(ScalarDiffusion {
            input_field,
            output_field,
            gradient_field: self.gradient_field,
            coefficient: self.coefficient,
            decay: self.decay,
            sources: self.sources,
            clamp_min: self.clamp_min,
            clamp_max: self.clamp_max,
        })
    }
}

impl Propagator for ScalarDiffusion {
    fn name(&self) -> &str {
        "ScalarDiffusion"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        [self.input_field].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        let mut w = vec![(self.output_field, WriteMode::Full)];
        if let Some(gf) = self.gradient_field {
            w.push((gf, WriteMode::Full));
        }
        w
    }

    fn max_dt(&self) -> Option<f64> {
        if self.coefficient > 0.0 {
            // CFL stability constraint: dt <= 1 / (max_degree * D)
            // Use worst-case degree 12 (Fcc12) so it's safe for all topologies.
            Some(1.0 / (12.0 * self.coefficient))
        } else {
            None
        }
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

    // Test field IDs far from the hardcoded constants to avoid collision.
    const F_HEAT: FieldId = FieldId(100);
    const F_OUT: FieldId = FieldId(101);
    const F_GRAD: FieldId = FieldId(102);

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
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_OUT)
            .build()
            .unwrap();

        assert_eq!(prop.name(), "ScalarDiffusion");
        assert_eq!(prop.reads(), FieldSet::empty());

        let rp = prop.reads_previous();
        assert!(rp.contains(F_HEAT));
        assert_eq!(rp.len(), 1);

        let w = prop.writes();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0], (F_OUT, WriteMode::Full));
    }

    #[test]
    fn builder_rejects_negative_coefficient() {
        let result = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_OUT)
            .coefficient(-0.1)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("coefficient"));
    }

    #[test]
    fn builder_rejects_missing_input() {
        let result = ScalarDiffusion::builder()
            .output_field(F_OUT)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("input_field"));
    }

    #[test]
    fn builder_rejects_missing_output() {
        let result = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("output_field"));
    }

    #[test]
    fn builder_rejects_negative_decay() {
        let result = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_OUT)
            .decay(-1.0)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("decay"));
    }

    #[test]
    fn builder_rejects_inverted_clamp() {
        let result = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_OUT)
            .clamp_min(10.0)
            .clamp_max(5.0)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("clamp_min"));
    }

    #[test]
    fn builder_rejects_nan_coefficient() {
        let result = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_OUT)
            .coefficient(f64::NAN)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("coefficient"));
    }

    #[test]
    fn builder_rejects_infinite_coefficient() {
        let result = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_OUT)
            .coefficient(f64::INFINITY)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("coefficient"));
    }

    #[test]
    fn builder_rejects_nan_decay() {
        let result = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_OUT)
            .decay(f64::NAN)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("decay"));
    }

    #[test]
    fn builder_rejects_infinite_decay() {
        let result = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_OUT)
            .decay(f64::INFINITY)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("decay"));
    }

    // ---------------------------------------------------------------
    // Diffusion physics tests
    // ---------------------------------------------------------------

    #[test]
    fn uniform_heat_stays_uniform() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.1)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, vec![10.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let heat = writer.get_field(F_HEAT).unwrap();
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
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.1)
            .build()
            .unwrap();

        let mut heat = vec![0.0f32; n];
        heat[12] = 100.0; // center of 5x5

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, heat);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(F_HEAT).unwrap();
        // Center should have decreased
        assert!(result[12] < 100.0, "center should cool: {}", result[12]);
        // Neighbours should have increased
        assert!(result[7] > 0.0, "north neighbour should warm: {}", result[7]);
        assert!(result[17] > 0.0, "south neighbour should warm: {}", result[17]);
        assert!(result[11] > 0.0, "west neighbour should warm: {}", result[11]);
        assert!(result[13] > 0.0, "east neighbour should warm: {}", result[13]);
    }

    #[test]
    fn energy_conservation() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.1)
            .build()
            .unwrap();

        let mut heat = vec![0.0f32; n];
        heat[12] = 100.0;
        let total_before: f32 = heat.iter().sum();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, heat);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(F_HEAT).unwrap();
        let total_after: f32 = result.iter().sum();
        assert!(
            (total_before - total_after).abs() < 1e-3,
            "energy not conserved: before={total_before}, after={total_after}"
        );
    }

    #[test]
    fn max_dt_constraint() {
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.25)
            .build()
            .unwrap();
        // 1 / (12 * 0.25) = 1/3
        let dt = prop.max_dt().unwrap();
        assert!((dt - 1.0 / 3.0).abs() < 1e-10);

        let prop2 = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(1.0)
            .build()
            .unwrap();
        // 1 / (12 * 1.0) = 1/12
        let dt2 = prop2.max_dt().unwrap();
        assert!((dt2 - 1.0 / 12.0).abs() < 1e-10);

        // Zero coefficient -> no constraint
        let prop3 = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.0)
            .build()
            .unwrap();
        assert!(prop3.max_dt().is_none());
    }

    #[test]
    fn decay_reduces_values() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.0)
            .decay(0.5)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, vec![10.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let dt = 0.1;
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, dt);

        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(F_HEAT).unwrap();
        // With coefficient=0, diffusion is identity. Then true exponential decay:
        // out[i] = 10.0 * exp(-0.5 * 0.1) = 10.0 * exp(-0.05) ≈ 9.5123
        let expected = 10.0_f64 * (-0.5_f64 * 0.1).exp();
        for &v in result {
            assert!(
                (v as f64 - expected).abs() < 1e-4,
                "decay should reduce to ~{expected:.4}, got {v}"
            );
        }
    }

    #[test]
    fn source_injection() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.1)
            .source(0, 42.0)
            .source(12, 99.0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, vec![0.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(F_HEAT).unwrap();
        assert!(
            (result[0] - 42.0).abs() < 1e-6,
            "source at 0 should be 42.0, got {}",
            result[0]
        );
        assert!(
            (result[12] - 99.0).abs() < 1e-6,
            "source at 12 should be 99.0, got {}",
            result[12]
        );
    }

    #[test]
    fn clamp_min_applied() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.0)
            .clamp_min(0.0)
            .build()
            .unwrap();

        let mut data = vec![5.0f32; n];
        data[0] = -10.0;
        data[4] = -3.0;

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, data);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(F_HEAT).unwrap();
        assert!(
            result[0] >= 0.0,
            "negative value should be clamped to 0, got {}",
            result[0]
        );
        assert!(
            result[4] >= 0.0,
            "negative value should be clamped to 0, got {}",
            result[4]
        );
        // Positive values should be unchanged (coefficient=0, no diffusion)
        assert!(
            (result[1] - 5.0).abs() < 1e-6,
            "positive value should be unchanged, got {}",
            result[1]
        );
    }

    #[test]
    fn separate_input_output_fields() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_OUT)
            .coefficient(0.0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, vec![7.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_OUT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(F_OUT).unwrap();
        for &v in result {
            assert!(
                (v - 7.0).abs() < 1e-6,
                "output should match input with coefficient=0, got {v}"
            );
        }

        // Verify F_HEAT was declared in reads_previous, F_OUT in writes
        let rp = prop.reads_previous();
        assert!(rp.contains(F_HEAT));
        assert!(!rp.contains(F_OUT));

        let w = prop.writes();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].0, F_OUT);
    }

    #[test]
    fn gradient_field_included_in_writes() {
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_OUT)
            .gradient_field(F_GRAD)
            .build()
            .unwrap();

        let w = prop.writes();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0], (F_OUT, WriteMode::Full));
        assert_eq!(w[1], (F_GRAD, WriteMode::Full));
    }

    #[test]
    fn wrap_energy_conservation() {
        let grid = Square4::new(5, 5, EdgeBehavior::Wrap).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.1)
            .build()
            .unwrap();

        let mut heat = vec![0.0f32; n];
        heat[0] = 100.0;
        let total_before: f32 = heat.iter().sum();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, heat);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(F_HEAT).unwrap();
        let total_after: f32 = result.iter().sum();
        assert!(
            (total_before - total_after).abs() < 1e-3,
            "wrap: energy not conserved: before={total_before}, after={total_after}"
        );
    }

    #[test]
    fn gradient_computation() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .gradient_field(F_GRAD)
            .coefficient(0.0) // no diffusion, just gradient
            .build()
            .unwrap();

        // Linear gradient in x: col 0=0, col 1=10, col 2=20
        let mut heat = vec![0.0f32; n];
        for r in 0..3 {
            for c in 0..3 {
                heat[r * 3 + c] = (c as f32) * 10.0;
            }
        }

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, heat);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);
        writer.add_field(F_GRAD, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let grad = writer.get_field(F_GRAD).unwrap();
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
    fn clamp_max_applied() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.0)
            .clamp_max(50.0)
            .build()
            .unwrap();

        let mut data = vec![10.0f32; n];
        data[0] = 100.0;

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, data);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(F_HEAT).unwrap();
        assert!(
            (result[0] - 50.0).abs() < 1e-6,
            "value above max should be clamped to 50, got {}",
            result[0]
        );
        assert!(
            (result[1] - 10.0).abs() < 1e-6,
            "value below max should be unchanged, got {}",
            result[1]
        );
    }

    #[test]
    fn decay_and_diffusion_combined() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.1)
            .decay(1.0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, vec![10.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let dt = 0.01;
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, dt);

        prop.step(&mut ctx).unwrap();

        // Uniform field: diffusion leaves values at 10.0
        // Then true exponential decay: out[i] = 10.0 * exp(-1.0 * 0.01) ≈ 9.9005
        let expected = 10.0_f64 * (-1.0_f64 * 0.01).exp();
        let result = writer.get_field(F_HEAT).unwrap();
        for &v in result {
            assert!(
                (v as f64 - expected).abs() < 1e-3,
                "combined decay+diffusion on uniform should yield ~{expected:.4}, got {v}"
            );
        }
    }

    #[test]
    fn sources_override_diffusion_and_decay() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.1)
            .decay(1.0)
            .source(4, 50.0) // center cell
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, vec![10.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(F_HEAT).unwrap();
        // Source is applied after decay, so center is pinned to 50.0
        assert!(
            (result[4] - 50.0).abs() < 1e-6,
            "source should override diffusion+decay, got {}",
            result[4]
        );
    }
}
