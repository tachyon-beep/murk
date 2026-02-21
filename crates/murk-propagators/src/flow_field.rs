//! Negative-gradient flow field propagator.
//!
//! Reads a scalar potential field from the previous tick (`reads_previous`) and
//! computes the negative gradient (flow direction) into a 2-component vector
//! field. Optionally normalizes the result to unit length.
//!
//! Has a [`Square4`] fast path for direct index arithmetic and a generic
//! fallback using `Space::canonical_ordering()`.
//!
//! Constructed via the builder pattern: [`FlowField::builder`].

use crate::grid_helpers::resolve_axis;
use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use murk_space::{EdgeBehavior, Square4};

/// A negative-gradient flow field propagator.
///
/// Each tick computes the negative central-difference gradient of a scalar
/// potential field from the previous tick into a 2-component vector field
/// (flow_x, flow_y). When `normalize` is true, the resulting vectors are
/// normalized to unit length.
///
/// # Square4 fast path
///
/// ```text
/// fx[i] = -(h_east - h_west) / 2.0
/// fy[i] = -(h_south - h_north) / 2.0
/// ```
///
/// For Absorb boundaries where a neighbour is out-of-bounds, falls back
/// to `prev[i]` (self value), producing a one-sided difference.
///
/// # Generic fallback
///
/// Same as `GradientCompute`'s generic path but negated: uses
/// `canonical_ordering()` + `canonical_rank()` + `neighbours()` to compute
/// per-axis gradients, then negates.
///
/// # Construction
///
/// Use the builder pattern:
///
/// ```
/// use murk_core::FieldId;
/// use murk_propagators::FlowField;
///
/// let prop = FlowField::builder()
///     .potential_field(FieldId(10))
///     .flow_field(FieldId(11))
///     .build()
///     .unwrap();
/// ```
#[derive(Debug)]
pub struct FlowField {
    potential_field: FieldId,
    flow_field: FieldId,
    normalize: bool,
}

/// Builder for [`FlowField`].
///
/// Required fields: `potential_field` and `flow_field`.
pub struct FlowFieldBuilder {
    potential_field: Option<FieldId>,
    flow_field: Option<FieldId>,
    normalize: bool,
}

impl FlowField {
    /// Create a new builder for configuring a `FlowField` propagator.
    pub fn builder() -> FlowFieldBuilder {
        FlowFieldBuilder {
            potential_field: None,
            flow_field: None,
            normalize: false,
        }
    }

    /// Square4 fast path: central-difference negative gradient using direct
    /// index arithmetic.
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
            .read(self.potential_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("potential field {:?} not readable", self.potential_field),
            })?
            .to_vec();

        let flow_out = ctx.writes().write(self.flow_field).ok_or_else(|| {
            PropagatorError::ExecutionFailed {
                reason: format!("flow field {:?} not writable", self.flow_field),
            }
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

                let mut fx = -(h_east - h_west) / 2.0;
                let mut fy = -(h_south - h_north) / 2.0;

                if self.normalize {
                    let mag = (fx * fx + fy * fy).sqrt();
                    if mag > 1e-12 {
                        fx /= mag;
                        fy /= mag;
                    }
                }

                flow_out[i * 2] = fx;
                flow_out[i * 2 + 1] = fy;
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
            .read(self.potential_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("potential field {:?} not readable", self.potential_field),
            })?
            .to_vec();

        // Compute negated gradient into local buffer
        let mut flow_buf = vec![0.0f32; cell_count * 2];

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
            // Negate the gradient to get flow direction (toward lower potential)
            let mut fx = if xc > 0 { -(gx / xc as f32) } else { 0.0 };
            let mut fy = if yc > 0 { -(gy / yc as f32) } else { 0.0 };

            if self.normalize {
                let mag = (fx * fx + fy * fy).sqrt();
                if mag > 1e-12 {
                    fx /= mag;
                    fy /= mag;
                }
            }

            flow_buf[i * 2] = fx;
            flow_buf[i * 2 + 1] = fy;
        }

        // Write results
        let flow_out = ctx.writes().write(self.flow_field).ok_or_else(|| {
            PropagatorError::ExecutionFailed {
                reason: format!("flow field {:?} not writable", self.flow_field),
            }
        })?;
        flow_out.copy_from_slice(&flow_buf);

        Ok(())
    }
}

impl FlowFieldBuilder {
    /// Set the input scalar potential field (read from previous tick).
    pub fn potential_field(mut self, field: FieldId) -> Self {
        self.potential_field = Some(field);
        self
    }

    /// Set the output 2-component vector field to write the flow into.
    pub fn flow_field(mut self, field: FieldId) -> Self {
        self.flow_field = Some(field);
        self
    }

    /// Set whether to normalize the output flow vectors to unit length.
    /// Default: `false`.
    pub fn normalize(mut self, normalize: bool) -> Self {
        self.normalize = normalize;
        self
    }

    /// Build the propagator, validating all configuration.
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - `potential_field` is not set
    /// - `flow_field` is not set
    pub fn build(self) -> Result<FlowField, String> {
        let potential_field = self
            .potential_field
            .ok_or_else(|| "potential_field is required".to_string())?;
        let flow_field = self
            .flow_field
            .ok_or_else(|| "flow_field is required".to_string())?;

        Ok(FlowField {
            potential_field,
            flow_field,
            normalize: self.normalize,
        })
    }
}

impl Propagator for FlowField {
    fn name(&self) -> &str {
        "FlowField"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        [self.potential_field].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.flow_field, WriteMode::Full)]
    }

    fn max_dt(&self, _space: &dyn murk_space::Space) -> Option<f64> {
        None // flow field computation has no stability constraint
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
    const F_POTENTIAL: FieldId = FieldId(100);
    const F_FLOW: FieldId = FieldId(101);

    fn make_ctx<'a>(
        reader: &'a MockFieldReader,
        writer: &'a mut MockFieldWriter,
        scratch: &'a mut ScratchRegion,
        space: &'a Square4,
    ) -> StepContext<'a> {
        StepContext::new(reader, reader, writer, scratch, space, TickId(1), 0.1)
    }

    // ---------------------------------------------------------------
    // Builder tests
    // ---------------------------------------------------------------

    #[test]
    fn builder_minimal() {
        let prop = FlowField::builder()
            .potential_field(F_POTENTIAL)
            .flow_field(F_FLOW)
            .build()
            .unwrap();

        assert_eq!(prop.name(), "FlowField");
        assert_eq!(prop.reads(), FieldSet::empty());

        let rp = prop.reads_previous();
        assert!(rp.contains(F_POTENTIAL));
        assert_eq!(rp.len(), 1);

        let w = prop.writes();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0], (F_FLOW, WriteMode::Full));

        let space = murk_space::Square4::new(4, 4, murk_space::EdgeBehavior::Wrap).unwrap();
        assert!(prop.max_dt(&space).is_none());
    }

    #[test]
    fn builder_rejects_missing_potential() {
        let result = FlowField::builder().flow_field(F_FLOW).build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("potential_field"));
    }

    #[test]
    fn builder_rejects_missing_flow() {
        let result = FlowField::builder().potential_field(F_POTENTIAL).build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("flow_field"));
    }

    // ---------------------------------------------------------------
    // Flow field physics tests (Square4 fast path)
    // ---------------------------------------------------------------

    #[test]
    fn uniform_potential_zero_flow() {
        // Uniform potential => all-zero flow
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = FlowField::builder()
            .potential_field(F_POTENTIAL)
            .flow_field(F_FLOW)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_POTENTIAL, vec![42.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_FLOW, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);

        prop.step(&mut ctx).unwrap();

        let flow = writer.get_field(F_FLOW).unwrap();
        for i in 0..n {
            assert!(
                flow[i * 2].abs() < 1e-6,
                "flow_x at cell {i} should be 0, got {}",
                flow[i * 2]
            );
            assert!(
                flow[i * 2 + 1].abs() < 1e-6,
                "flow_y at cell {i} should be 0, got {}",
                flow[i * 2 + 1]
            );
        }
    }

    #[test]
    fn flow_points_toward_lower_potential() {
        // Linear ramp: potential increases with column index.
        // col 0=0, col 1=10, col 2=20
        // Gradient points east (positive x), so flow = -gradient points west
        // ... wait, negative gradient of increasing-to-the-east means flow
        // points toward lower potential = toward west (negative x)?
        //
        // Actually: if potential increases eastward, gradient_x is positive,
        // so flow_x = -gradient_x is negative. But the task says "flow points
        // east" for a linear ramp. Let's re-read: the task says potential_field
        // has a linear ramp, and "flow points east (positive x)".
        //
        // The convention: potential is HIGH in the east. Flow should go DOWNHILL
        // = toward lower potential = toward west. But the test name says
        // "flow_points_toward_lower_potential", so the ramp must go the other way:
        // potential DECREASES eastward, so flow is eastward.
        //
        // Let's set up: potential decreases with column index.
        // col 0=20, col 1=10, col 2=0
        // Then gradient_x = (h_east - h_west)/2 = negative, flow_x = -grad_x = positive.
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = FlowField::builder()
            .potential_field(F_POTENTIAL)
            .flow_field(F_FLOW)
            .build()
            .unwrap();

        let mut potential = vec![0.0f32; n];
        for r in 0..3 {
            for c in 0..3 {
                // Potential decreases eastward
                potential[r * 3 + c] = (2 - c) as f32 * 10.0;
            }
        }

        let mut reader = MockFieldReader::new();
        reader.set_field(F_POTENTIAL, potential);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_FLOW, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);

        prop.step(&mut ctx).unwrap();

        let flow = writer.get_field(F_FLOW).unwrap();
        // Center cell (1,1): h_east=0, h_west=20
        // grad_x = (0 - 20) / 2 = -10
        // flow_x = -(-10) = 10 (positive = east)
        let center = 4;
        assert!(
            flow[center * 2] > 0.0,
            "flow_x at center should be positive (eastward), got {}",
            flow[center * 2]
        );
        assert!(
            (flow[center * 2] - 10.0).abs() < 1e-6,
            "flow_x at center should be 10, got {}",
            flow[center * 2]
        );
        assert!(
            flow[center * 2 + 1].abs() < 1e-6,
            "flow_y at center should be 0, got {}",
            flow[center * 2 + 1]
        );
    }

    #[test]
    fn normalized_flow_is_unit_length() {
        // Diagonal ramp: potential decreases in both x and y.
        // potential[r][c] = (2-c)*10 + (2-r)*10
        // At center (1,1): potential = 10+10 = 20
        // h_east(1,2)=0+10=10, h_west(1,0)=20+10=30
        // h_south(2,1)=10+0=10, h_north(0,1)=10+20=30
        // grad_x = (10-30)/2 = -10, flow_x = 10
        // grad_y = (10-30)/2 = -10, flow_y = 10
        // magnitude = sqrt(100+100) = sqrt(200) ≈ 14.14
        // normalized: (10/14.14, 10/14.14) ≈ (0.707, 0.707), magnitude ≈ 1.0
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = FlowField::builder()
            .potential_field(F_POTENTIAL)
            .flow_field(F_FLOW)
            .normalize(true)
            .build()
            .unwrap();

        let mut potential = vec![0.0f32; n];
        for r in 0..3 {
            for c in 0..3 {
                potential[r * 3 + c] = (2 - c) as f32 * 10.0 + (2 - r) as f32 * 10.0;
            }
        }

        let mut reader = MockFieldReader::new();
        reader.set_field(F_POTENTIAL, potential);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_FLOW, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);

        prop.step(&mut ctx).unwrap();

        let flow = writer.get_field(F_FLOW).unwrap();
        let center = 4;
        let fx = flow[center * 2];
        let fy = flow[center * 2 + 1];
        let mag = (fx * fx + fy * fy).sqrt();
        assert!(
            (mag - 1.0).abs() < 1e-5,
            "normalized flow at center should have magnitude ~1.0, got {} (fx={}, fy={})",
            mag,
            fx,
            fy
        );
    }
}
