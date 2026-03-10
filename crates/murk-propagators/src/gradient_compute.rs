//! Standalone finite-difference gradient propagator.
//!
//! Reads a scalar field from the previous tick (`reads_previous`) and
//! computes the central-difference gradient into a 2-component vector field.
//! Has a [`Square4`] fast path for direct index arithmetic and a generic
//! fallback using `Space::canonical_ordering()`.
//!
//! Constructed via the builder pattern: [`GradientCompute::builder`].

use crate::grid_helpers::resolve_axis;
use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use murk_space::{EdgeBehavior, Square4};

/// A standalone finite-difference gradient propagator.
///
/// Each tick computes the central-difference gradient of a scalar field
/// from the previous tick into a 2-component vector field (grad_x, grad_y).
///
/// # Square4 fast path
///
/// ```text
/// grad_x[i] = (h_east - h_west) / 2.0
/// grad_y[i] = (h_south - h_north) / 2.0
/// ```
///
/// For Absorb boundaries where a neighbour is out-of-bounds, falls back
/// to `prev[i]` (self value), producing a one-sided difference.
///
/// # Generic fallback
///
/// ```text
/// For each neighbour with (rank, dc, dr):
///   dh = prev[rank] - prev[i]
///   if dc != 0: gx += dh/dc, xc++
///   if dr != 0: gy += dh/dr, yc++
/// grad_x = gx / xc (or 0)
/// grad_y = gy / yc (or 0)
/// ```
///
/// # Construction
///
/// Use the builder pattern:
///
/// ```
/// use murk_core::FieldId;
/// use murk_propagators::GradientCompute;
///
/// let prop = GradientCompute::builder()
///     .input_field(FieldId(10))
///     .output_field(FieldId(11))
///     .build()
///     .unwrap();
/// ```
#[derive(Debug)]
pub struct GradientCompute {
    input_field: FieldId,
    output_field: FieldId,
}

/// Builder for [`GradientCompute`].
///
/// Required fields: `input_field` and `output_field`.
pub struct GradientComputeBuilder {
    input_field: Option<FieldId>,
    output_field: Option<FieldId>,
}

impl GradientCompute {
    /// Create a new builder for configuring a `GradientCompute` propagator.
    pub fn builder() -> GradientComputeBuilder {
        GradientComputeBuilder {
            input_field: None,
            output_field: None,
        }
    }

    /// Square4 fast path: central-difference gradient using direct index arithmetic.
    fn step_square4(
        &self,
        ctx: &mut StepContext<'_>,
        rows: u32,
        cols: u32,
        edge: EdgeBehavior,
    ) -> Result<(), PropagatorError> {
        let rows_i = rows as i32;
        let cols_i = cols as i32;

        let prev = ctx
            .reads_previous()
            .read(self.input_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("input field {:?} not readable", self.input_field),
            })?
            .to_vec();

        let grad_out = ctx.writes().write(self.output_field).ok_or_else(|| {
            PropagatorError::ExecutionFailed {
                reason: format!("output field {:?} not writable", self.output_field),
            }
        })?;

        let cell_count = rows as usize * cols as usize;
        if grad_out.len() < cell_count * 2 {
            return Err(PropagatorError::ExecutionFailed {
                reason: format!(
                    "output field {:?} has {} elements, need {} (2 per cell for gradient)",
                    self.output_field,
                    grad_out.len(),
                    cell_count * 2,
                ),
            });
        }

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

        Ok(())
    }

    /// Generic fallback using `Space::canonical_ordering()`.
    fn step_generic(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        // Precompute spatial topology before taking any mutable borrows
        let ordering = ctx.space().canonical_ordering();
        let cell_count = ordering.len();

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
        let prev = ctx
            .reads_previous()
            .read(self.input_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("input field {:?} not readable", self.input_field),
            })?
            .to_vec();

        // Compute gradient into local buffer
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

        // Write results
        let grad_out = ctx.writes().write(self.output_field).ok_or_else(|| {
            PropagatorError::ExecutionFailed {
                reason: format!("output field {:?} not writable", self.output_field),
            }
        })?;
        if grad_out.len() != cell_count * 2 {
            return Err(PropagatorError::ExecutionFailed {
                reason: format!(
                    "output field {:?} has {} elements, need {} (2 per cell for gradient)",
                    self.output_field,
                    grad_out.len(),
                    cell_count * 2,
                ),
            });
        }
        grad_out.copy_from_slice(&grad_buf);

        Ok(())
    }
}

impl GradientComputeBuilder {
    /// Set the input scalar field to compute the gradient of (read from previous tick).
    pub fn input_field(mut self, field: FieldId) -> Self {
        self.input_field = Some(field);
        self
    }

    /// Set the output 2-component vector field to write the gradient into.
    pub fn output_field(mut self, field: FieldId) -> Self {
        self.output_field = Some(field);
        self
    }

    /// Build the propagator, validating all configuration.
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - `input_field` is not set
    /// - `output_field` is not set
    pub fn build(self) -> Result<GradientCompute, String> {
        let input_field = self
            .input_field
            .ok_or_else(|| "input_field is required".to_string())?;
        let output_field = self
            .output_field
            .ok_or_else(|| "output_field is required".to_string())?;

        Ok(GradientCompute {
            input_field,
            output_field,
        })
    }
}

impl Propagator for GradientCompute {
    fn name(&self) -> &str {
        "GradientCompute"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        [self.input_field].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.output_field, WriteMode::Full)]
    }

    fn max_dt(&self, _space: &dyn murk_space::Space) -> Option<f64> {
        None // gradient computation has no stability constraint
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
    const F_SCALAR: FieldId = FieldId(100);
    const F_GRAD: FieldId = FieldId(101);

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
        let prop = GradientCompute::builder()
            .input_field(F_SCALAR)
            .output_field(F_GRAD)
            .build()
            .unwrap();

        assert_eq!(prop.name(), "GradientCompute");
        assert_eq!(prop.reads(), FieldSet::empty());

        let rp = prop.reads_previous();
        assert!(rp.contains(F_SCALAR));
        assert_eq!(rp.len(), 1);

        let w = prop.writes();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0], (F_GRAD, WriteMode::Full));

        let space = crate::test_helpers::test_space();
        assert!(prop.max_dt(&space).is_none());
    }

    #[test]
    fn builder_rejects_missing_fields() {
        // Missing both
        let result = GradientComputeBuilder {
            input_field: None,
            output_field: None,
        }
        .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("input_field"));

        // Missing output
        let result = GradientCompute::builder().input_field(F_SCALAR).build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("output_field"));

        // Missing input
        let result = GradientCompute::builder().output_field(F_GRAD).build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("input_field"));
    }

    // ---------------------------------------------------------------
    // Gradient physics tests (Square4 fast path)
    // ---------------------------------------------------------------

    #[test]
    fn linear_x_gradient() {
        // 3x3 Absorb grid, linear x-ramp: col 0=0, col 1=10, col 2=20
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = GradientCompute::builder()
            .input_field(F_SCALAR)
            .output_field(F_GRAD)
            .build()
            .unwrap();

        let mut scalar = vec![0.0f32; n];
        for r in 0..3 {
            for c in 0..3 {
                scalar[r * 3 + c] = (c as f32) * 10.0;
            }
        }

        let mut reader = MockFieldReader::new();
        reader.set_field(F_SCALAR, scalar);

        let mut writer = MockFieldWriter::new();
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
    fn uniform_field_zero_gradient() {
        // 5x5 Wrap grid, uniform field => all gradients zero
        let grid = Square4::new(5, 5, EdgeBehavior::Wrap).unwrap();
        let n = grid.cell_count();
        let prop = GradientCompute::builder()
            .input_field(F_SCALAR)
            .output_field(F_GRAD)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_SCALAR, vec![42.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_GRAD, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let grad = writer.get_field(F_GRAD).unwrap();
        for i in 0..n {
            assert!(
                grad[i * 2].abs() < 1e-6,
                "grad_x at cell {i} should be 0, got {}",
                grad[i * 2]
            );
            assert!(
                grad[i * 2 + 1].abs() < 1e-6,
                "grad_y at cell {i} should be 0, got {}",
                grad[i * 2 + 1]
            );
        }
    }

    #[test]
    fn boundary_gradient_absorb() {
        // 3x3 Absorb grid: corner cell (0,0) with Absorb boundary.
        // hot corner = 50, rest = 0.
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = GradientCompute::builder()
            .input_field(F_SCALAR)
            .output_field(F_GRAD)
            .build()
            .unwrap();

        let mut scalar = vec![0.0f32; n];
        scalar[0] = 50.0;

        let mut reader = MockFieldReader::new();
        reader.set_field(F_SCALAR, scalar);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_GRAD, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let grad = writer.get_field(F_GRAD).unwrap();
        // Corner (0,0):
        //   east = scalar[1] = 0, west = Absorb OOB => self = 50
        //   grad_x = (0 - 50) / 2 = -25
        //   south = scalar[3] = 0, north = Absorb OOB => self = 50
        //   grad_y = (0 - 50) / 2 = -25
        assert!(
            (grad[0] - (-25.0)).abs() < 1e-6,
            "corner grad_x should be -25, got {}",
            grad[0]
        );
        assert!(
            (grad[1] - (-25.0)).abs() < 1e-6,
            "corner grad_y should be -25, got {}",
            grad[1]
        );
    }

    #[test]
    fn wrap_gradient_at_boundary() {
        // 4x4 Wrap grid, linear-x: col c has value c*10.
        // Cell (0,0): east=(0,1)=10, west=wrap to (0,3)=30
        // grad_x = (10 - 30) / 2 = -10
        let grid = Square4::new(4, 4, EdgeBehavior::Wrap).unwrap();
        let n = grid.cell_count();
        let prop = GradientCompute::builder()
            .input_field(F_SCALAR)
            .output_field(F_GRAD)
            .build()
            .unwrap();

        let mut scalar = vec![0.0f32; n];
        for r in 0..4 {
            for c in 0..4 {
                scalar[r * 4 + c] = (c as f32) * 10.0;
            }
        }

        let mut reader = MockFieldReader::new();
        reader.set_field(F_SCALAR, scalar);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_GRAD, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        prop.step(&mut ctx).unwrap();

        let grad = writer.get_field(F_GRAD).unwrap();
        // Cell (0,0): east=(0,1)=10, west=wrap to (0,3)=30
        // grad_x = (10 - 30) / 2 = -10
        assert!(
            (grad[0] - (-10.0)).abs() < 1e-6,
            "wrap grad_x at (0,0) should be -10, got {}",
            grad[0]
        );
        // grad_y: south=(1,0)=0, north=wrap to (3,0)=0 => (0-0)/2 = 0
        assert!(
            grad[1].abs() < 1e-6,
            "wrap grad_y at (0,0) should be 0, got {}",
            grad[1]
        );
    }

    // ---------------------------------------------------------------
    // Buffer validation (P1: guard against scalar output fields)
    // ---------------------------------------------------------------

    #[test]
    fn scalar_output_field_returns_error() {
        // Output field has cell_count elements (scalar) instead of cell_count*2 (vector).
        // GradientCompute must return ExecutionFailed, not panic with index OOB.
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = GradientCompute::builder()
            .input_field(F_SCALAR)
            .output_field(F_GRAD)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_SCALAR, vec![1.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_GRAD, n); // scalar-sized: n, not n*2

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);

        let result = prop.step(&mut ctx);
        assert!(
            result.is_err(),
            "expected error for undersized output buffer"
        );
        let err = result.unwrap_err();
        match err {
            PropagatorError::ExecutionFailed { reason } => {
                assert!(
                    reason.contains("elements"),
                    "error should mention element count, got: {reason}"
                );
            }
            other => panic!("expected ExecutionFailed, got {other:?}"),
        }
    }
}
