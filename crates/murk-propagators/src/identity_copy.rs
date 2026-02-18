//! Trivial propagator that copies a field's previous-tick values into the
//! current tick unchanged.
//!
//! Used when a field must persist across ticks without any transformation
//! (e.g., carrying agent positions forward in `hex_pursuit`).
//!
//! # Semantics
//!
//! - Reads from the **previous tick** (`reads_previous`) via the frozen
//!   tick-start view (Jacobi-style).
//! - Writes the **same field** (`WriteMode::Full`) in the current tick.
//! - No math, no parameters beyond the field ID.
//!
//! This differs from the test-utils `IdentityPropagator`, which reads from
//! the current tick (`reads()`) and copies to a *different* field.
//!
//! # Construction
//!
//! ```
//! use murk_core::FieldId;
//! use murk_propagators::IdentityCopy;
//!
//! let prop = IdentityCopy::new(FieldId(5));
//! ```

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};

/// A propagator that copies a single field verbatim from the previous tick
/// to the current tick.
///
/// This is the simplest production propagator: no arithmetic, no spatial
/// queries, no scratch memory. It exists to carry forward state that no
/// other propagator writes (e.g., agent positions between movement ticks).
#[derive(Debug)]
pub struct IdentityCopy {
    field: FieldId,
}

impl IdentityCopy {
    /// Create a new `IdentityCopy` for the given field.
    ///
    /// The propagator will read `field` from the previous tick and write
    /// it unchanged to the current tick.
    pub fn new(field: FieldId) -> Self {
        Self { field }
    }
}

impl Propagator for IdentityCopy {
    fn name(&self) -> &str {
        "IdentityCopy"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        [self.field].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.field, WriteMode::Full)]
    }

    fn max_dt(&self) -> Option<f64> {
        None
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let prev = ctx
            .reads_previous()
            .read(self.field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("field {:?} not readable", self.field),
            })?
            .to_vec();
        let out = ctx
            .writes()
            .write(self.field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("field {:?} not writable", self.field),
            })?;
        out.copy_from_slice(&prev);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::TickId;
    use murk_propagator::scratch::ScratchRegion;
    use murk_space::{EdgeBehavior, Line1D};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    const F_FIELD: FieldId = FieldId(100);

    fn make_ctx<'a>(
        prev_reader: &'a MockFieldReader,
        writer: &'a mut MockFieldWriter,
        scratch: &'a mut ScratchRegion,
        space: &'a Line1D,
    ) -> StepContext<'a> {
        let empty_reader = prev_reader; // overlay reader unused by IdentityCopy
        StepContext::new(
            empty_reader,
            prev_reader,
            writer,
            scratch,
            space,
            TickId(1),
            0.1,
        )
    }

    #[test]
    fn copies_previous_to_current() {
        let space = Line1D::new(4, EdgeBehavior::Absorb).unwrap();
        let prop = IdentityCopy::new(F_FIELD);

        let input = vec![1.0, 2.0, 3.0, 4.0];
        let mut reader = MockFieldReader::new();
        reader.set_field(F_FIELD, input.clone());

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_FIELD, 4);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &space);

        prop.step(&mut ctx).unwrap();

        let output = writer.get_field(F_FIELD).unwrap();
        assert_eq!(output, &input[..], "output must exactly match previous-tick input");
    }

    #[test]
    fn declares_correct_fields() {
        let prop = IdentityCopy::new(F_FIELD);

        // name
        assert_eq!(prop.name(), "IdentityCopy");

        // reads() is empty — IdentityCopy does not use the in-tick overlay
        assert_eq!(prop.reads(), FieldSet::empty());

        // reads_previous() contains exactly {F_FIELD}
        let rp = prop.reads_previous();
        assert!(rp.contains(F_FIELD));
        assert_eq!(rp.len(), 1);

        // writes() is [(F_FIELD, Full)]
        let w = prop.writes();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0], (F_FIELD, WriteMode::Full));

        // max_dt() is None — no stability constraint
        assert!(prop.max_dt().is_none());
    }

    #[test]
    fn works_with_multi_component_field() {
        // Simulate a 2-component vector field on a 3-cell line:
        // total buffer size = 3 cells * 2 components = 6 floats.
        let n_cells: usize = 3;
        let components: usize = 2;
        let buf_len = n_cells * components;

        let space = Line1D::new(n_cells as u32, EdgeBehavior::Absorb).unwrap();
        let prop = IdentityCopy::new(F_FIELD);

        let input: Vec<f32> = (0..buf_len).map(|i| (i as f32) * 10.0).collect();
        let mut reader = MockFieldReader::new();
        reader.set_field(F_FIELD, input.clone());

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_FIELD, buf_len);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &space);

        prop.step(&mut ctx).unwrap();

        let output = writer.get_field(F_FIELD).unwrap();
        assert_eq!(
            output,
            &input[..],
            "multi-component vector field must be copied verbatim"
        );
    }
}
