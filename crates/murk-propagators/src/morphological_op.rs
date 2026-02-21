//! Morphological erosion/dilation propagator.
//!
//! Operates on a scalar field binarized by a threshold: values above the
//! threshold are "present" (1), at or below are "absent" (0).
//!
//! - **Dilate**: output is 1.0 if *any* cell within `radius` hops is present.
//! - **Erode**: output is 1.0 only if *all* cells within `radius` hops are present.
//!
//! Useful for computing reachability, expanding danger zones, shrinking
//! safe zones, and smoothing binary masks.
//!
//! Uses BFS through `Space::neighbours()` for topology-agnostic operation.
//!
//! Constructed via the builder pattern: [`MorphologicalOp::builder`].

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use std::collections::{HashSet, VecDeque};

/// Morphological operation type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MorphOp {
    /// Output is 1.0 if any cell in the neighborhood is present.
    Dilate,
    /// Output is 1.0 only if all cells in the neighborhood are present.
    Erode,
}

/// A morphological erosion/dilation propagator.
///
/// Reads a scalar field from the previous tick, binarizes it using
/// `threshold`, applies the morphological operation within `radius`
/// hops, and writes the result as a binary field (0.0 or 1.0).
#[derive(Debug)]
pub struct MorphologicalOp {
    input_field: FieldId,
    output_field: FieldId,
    op: MorphOp,
    radius: u32,
    threshold: f32,
}

/// Builder for [`MorphologicalOp`].
///
/// Required fields: `input_field` and `output_field`.
pub struct MorphologicalOpBuilder {
    input_field: Option<FieldId>,
    output_field: Option<FieldId>,
    op: MorphOp,
    radius: u32,
    threshold: f32,
}

impl MorphologicalOp {
    /// Create a new builder for configuring a `MorphologicalOp` propagator.
    pub fn builder() -> MorphologicalOpBuilder {
        MorphologicalOpBuilder {
            input_field: None,
            output_field: None,
            op: MorphOp::Dilate,
            radius: 1,
            threshold: 0.5,
        }
    }
}

impl MorphologicalOpBuilder {
    /// Set the input scalar field.
    pub fn input_field(mut self, field: FieldId) -> Self {
        self.input_field = Some(field);
        self
    }

    /// Set the output binary field.
    pub fn output_field(mut self, field: FieldId) -> Self {
        self.output_field = Some(field);
        self
    }

    /// Set the morphological operation (default: Dilate).
    pub fn op(mut self, op: MorphOp) -> Self {
        self.op = op;
        self
    }

    /// Set the BFS radius in hops (default: 1). Must be >= 1.
    pub fn radius(mut self, radius: u32) -> Self {
        self.radius = radius;
        self
    }

    /// Set the binarization threshold (default: 0.5).
    /// Values strictly above threshold are "present".
    pub fn threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold;
        self
    }

    /// Build the propagator, validating all configuration.
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - `input_field` is not set
    /// - `output_field` is not set
    /// - `radius` is 0
    pub fn build(self) -> Result<MorphologicalOp, String> {
        let input_field = self
            .input_field
            .ok_or_else(|| "input_field is required".to_string())?;
        let output_field = self
            .output_field
            .ok_or_else(|| "output_field is required".to_string())?;

        if self.radius == 0 {
            return Err("radius must be >= 1".to_string());
        }

        Ok(MorphologicalOp {
            input_field,
            output_field,
            op: self.op,
            radius: self.radius,
            threshold: self.threshold,
        })
    }
}

impl Propagator for MorphologicalOp {
    fn name(&self) -> &str {
        "MorphologicalOp"
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

    fn max_dt(&self) -> Option<f64> {
        None
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let ordering = ctx.space().canonical_ordering();
        let cell_count = ordering.len();

        // Precompute immediate neighbour ranks for BFS
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

        let prev = ctx
            .reads_previous()
            .read(self.input_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("input field {:?} not readable", self.input_field),
            })?
            .to_vec();

        // Binarize input
        let binary: Vec<bool> = prev.iter().map(|&v| v > self.threshold).collect();

        // Compute output into local buffer.
        // Pre-allocate BFS containers outside the loop; clear() between
        // iterations to amortise allocation cost (fixes #94 hotspot 1).
        let mut out_buf = vec![0.0f32; cell_count];
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        for i in 0..cell_count {
            visited.clear();
            queue.clear();
            visited.insert(i);
            queue.push_back((i, 0u32));

            let mut all_present = true;
            let mut any_present = false;

            while let Some((rank, depth)) = queue.pop_front() {
                if rank < binary.len() {
                    if binary[rank] {
                        any_present = true;
                    } else {
                        all_present = false;
                    }
                }

                if depth < self.radius {
                    for &nb_rank in &neighbour_ranks[rank] {
                        if visited.insert(nb_rank) {
                            queue.push_back((nb_rank, depth + 1));
                        }
                    }
                }
            }

            out_buf[i] = match self.op {
                MorphOp::Dilate => {
                    if any_present {
                        1.0
                    } else {
                        0.0
                    }
                }
                MorphOp::Erode => {
                    if all_present {
                        1.0
                    } else {
                        0.0
                    }
                }
            };
        }

        let out = ctx.writes().write(self.output_field).ok_or_else(|| {
            PropagatorError::ExecutionFailed {
                reason: format!("output field {:?} not writable", self.output_field),
            }
        })?;
        out.copy_from_slice(&out_buf);

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

    const F_IN: FieldId = FieldId(100);
    const F_OUT: FieldId = FieldId(101);

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
        let prop = MorphologicalOp::builder()
            .input_field(F_IN)
            .output_field(F_OUT)
            .build()
            .unwrap();

        assert_eq!(prop.name(), "MorphologicalOp");
        assert!(prop.reads().is_empty(), "reads() should be empty");
        assert!(prop.max_dt().is_none());

        let rp = prop.reads_previous();
        assert!(rp.contains(F_IN));

        let w = prop.writes();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0], (F_OUT, WriteMode::Full));
    }

    #[test]
    fn builder_rejects_missing_input() {
        let result = MorphologicalOp::builder().output_field(F_OUT).build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("input_field"));
    }

    #[test]
    fn builder_rejects_missing_output() {
        let result = MorphologicalOp::builder().input_field(F_IN).build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("output_field"));
    }

    #[test]
    fn builder_rejects_zero_radius() {
        let result = MorphologicalOp::builder()
            .input_field(F_IN)
            .output_field(F_OUT)
            .radius(0)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("radius"));
    }

    // ---------------------------------------------------------------
    // Step logic tests
    // ---------------------------------------------------------------

    #[test]
    fn dilate_expands_single_cell() {
        // 3x3 grid, single cell present at center (cell 4)
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let prop = MorphologicalOp::builder()
            .input_field(F_IN)
            .output_field(F_OUT)
            .op(MorphOp::Dilate)
            .radius(1)
            .threshold(0.5)
            .build()
            .unwrap();

        let mut input = vec![0.0f32; n];
        input[4] = 1.0; // center

        let mut reader = MockFieldReader::new();
        reader.set_field(F_IN, input);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_OUT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_OUT).unwrap();
        // Center and its 4 neighbors should be 1.0
        assert_eq!(out[4], 1.0, "center");
        assert_eq!(out[1], 1.0, "north");
        assert_eq!(out[7], 1.0, "south");
        assert_eq!(out[3], 1.0, "west");
        assert_eq!(out[5], 1.0, "east");
        // Corners should still be 0.0 (Manhattan distance 2 from center)
        assert_eq!(out[0], 0.0, "top-left corner");
        assert_eq!(out[8], 0.0, "bottom-right corner");
    }

    #[test]
    fn erode_shrinks_block() {
        // 3x3 grid, all cells present. With Absorb boundaries,
        // BFS only visits cells that exist -- all visited cells are
        // present, so erosion preserves all.
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let prop = MorphologicalOp::builder()
            .input_field(F_IN)
            .output_field(F_OUT)
            .op(MorphOp::Erode)
            .radius(1)
            .threshold(0.5)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_IN, vec![1.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_OUT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_OUT).unwrap();
        // All cells present with absorb boundaries -> all survive erosion
        assert_eq!(out[4], 1.0, "center should survive erosion");
        assert_eq!(out[0], 1.0, "all present -> erosion preserves all");
    }

    #[test]
    fn erode_removes_isolated_cell() {
        // 3x3 grid, only center present
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let prop = MorphologicalOp::builder()
            .input_field(F_IN)
            .output_field(F_OUT)
            .op(MorphOp::Erode)
            .radius(1)
            .threshold(0.5)
            .build()
            .unwrap();

        let mut input = vec![0.0f32; n];
        input[4] = 1.0;

        let mut reader = MockFieldReader::new();
        reader.set_field(F_IN, input);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_OUT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_OUT).unwrap();
        // Center has 4 neighbors, none present -> not all present -> eroded to 0
        assert_eq!(out[4], 0.0, "isolated cell should be eroded");
    }

    #[test]
    fn dilate_radius_2() {
        // 5x5 grid, single cell at center (2,2) = index 12
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let prop = MorphologicalOp::builder()
            .input_field(F_IN)
            .output_field(F_OUT)
            .op(MorphOp::Dilate)
            .radius(2)
            .threshold(0.5)
            .build()
            .unwrap();

        let mut input = vec![0.0f32; n];
        input[12] = 1.0;

        let mut reader = MockFieldReader::new();
        reader.set_field(F_IN, input);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_OUT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_OUT).unwrap();
        // Center should be dilated
        assert_eq!(out[12], 1.0, "center");
        // 2 hops away: (0,2)=2, (2,0)=10, (4,2)=22, (2,4)=14
        assert_eq!(out[2], 1.0, "2 hops north");
        assert_eq!(out[22], 1.0, "2 hops south");
        // Corners at Manhattan distance 4 should be 0.0
        assert_eq!(out[0], 0.0, "corner too far");
    }

    #[test]
    fn threshold_binarization() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let prop = MorphologicalOp::builder()
            .input_field(F_IN)
            .output_field(F_OUT)
            .op(MorphOp::Dilate)
            .radius(1)
            .threshold(0.7)
            .build()
            .unwrap();

        // Only cell 4 is above threshold 0.7
        let input = vec![0.0, 0.5, 0.0, 0.5, 0.8, 0.5, 0.0, 0.5, 0.0];

        let mut reader = MockFieldReader::new();
        reader.set_field(F_IN, input);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_OUT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_OUT).unwrap();
        assert_eq!(out[4], 1.0, "center above threshold -> dilated");
        assert_eq!(out[1], 1.0, "north neighbor of present cell");
        assert_eq!(out[0], 0.0, "corner: no neighbor above threshold");
    }
}
