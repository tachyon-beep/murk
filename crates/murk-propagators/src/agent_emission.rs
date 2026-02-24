//! Agent emission propagator.
//!
//! Reads a presence field and writes an emission field. Supports two modes:
//!
//! - **Set**: fills emission with zero, then sets `intensity` at each cell
//!   where presence is nonzero.
//! - **Additive**: copies the previous emission forward, then adds `intensity`
//!   at each cell where presence is nonzero.
//!
//! # Borrow contract
//!
//! In Additive mode the propagator both reads and writes the emission field.
//! Because `reads_previous()` and `writes()` yield separate borrows through
//! [`StepContext`], the previous emission data is copied into a `Vec` before
//! taking the mutable write borrow.
//!
//! # Construction
//!
//! ```
//! use murk_core::FieldId;
//! use murk_propagators::{AgentEmission, EmissionMode};
//!
//! let prop = AgentEmission::builder()
//!     .presence_field(FieldId(0))
//!     .emission_field(FieldId(1))
//!     .intensity(2.5)
//!     .mode(EmissionMode::Additive)
//!     .build()
//!     .unwrap();
//! ```

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};

/// Emission mode controlling how the propagator combines new emissions with
/// existing values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmissionMode {
    /// Add `intensity` to the previous emission at cells where presence is
    /// nonzero.
    Additive,
    /// Set emission to `intensity` at cells where presence is nonzero; zero
    /// everywhere else.
    Set,
}

/// A propagator that emits a scalar value at cells where agents are present.
///
/// Reads a presence field from the previous tick and writes an emission field.
/// In [`EmissionMode::Additive`] mode, the previous emission is carried
/// forward and intensity is accumulated. In [`EmissionMode::Set`] mode, the
/// emission field is zeroed first and intensity is placed only at occupied
/// cells.
#[derive(Debug)]
pub struct AgentEmission {
    presence_field: FieldId,
    emission_field: FieldId,
    intensity: f32,
    mode: EmissionMode,
}

/// Builder for [`AgentEmission`].
///
/// Required fields: `presence_field`, `emission_field`.
/// Defaults: `intensity = 1.0`, `mode = Additive`.
pub struct AgentEmissionBuilder {
    presence_field: Option<FieldId>,
    emission_field: Option<FieldId>,
    intensity: f32,
    mode: EmissionMode,
}

impl AgentEmission {
    /// Create a new builder for configuring an `AgentEmission` propagator.
    pub fn builder() -> AgentEmissionBuilder {
        AgentEmissionBuilder {
            presence_field: None,
            emission_field: None,
            intensity: 1.0,
            mode: EmissionMode::Additive,
        }
    }
}

impl AgentEmissionBuilder {
    /// Set the input presence field (read from previous tick).
    pub fn presence_field(mut self, field: FieldId) -> Self {
        self.presence_field = Some(field);
        self
    }

    /// Set the output emission field to write.
    pub fn emission_field(mut self, field: FieldId) -> Self {
        self.emission_field = Some(field);
        self
    }

    /// Set the emission intensity. Must be > 0.
    /// Default: `1.0`.
    pub fn intensity(mut self, intensity: f32) -> Self {
        self.intensity = intensity;
        self
    }

    /// Set the emission mode.
    /// Default: [`EmissionMode::Additive`].
    pub fn mode(mut self, mode: EmissionMode) -> Self {
        self.mode = mode;
        self
    }

    /// Build the propagator, validating all configuration.
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - `presence_field` is not set
    /// - `emission_field` is not set
    /// - `intensity` is not > 0
    pub fn build(self) -> Result<AgentEmission, String> {
        let presence_field = self
            .presence_field
            .ok_or_else(|| "presence_field is required".to_string())?;
        let emission_field = self
            .emission_field
            .ok_or_else(|| "emission_field is required".to_string())?;

        if !self.intensity.is_finite() || self.intensity <= 0.0 {
            return Err(format!(
                "intensity must be finite and > 0, got {}",
                self.intensity
            ));
        }

        Ok(AgentEmission {
            presence_field,
            emission_field,
            intensity: self.intensity,
            mode: self.mode,
        })
    }
}

impl Propagator for AgentEmission {
    fn name(&self) -> &str {
        "AgentEmission"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        match self.mode {
            EmissionMode::Set => [self.presence_field].into_iter().collect(),
            EmissionMode::Additive => [self.presence_field, self.emission_field]
                .into_iter()
                .collect(),
        }
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.emission_field, WriteMode::Full)]
    }

    fn max_dt(&self, _space: &dyn murk_space::Space) -> Option<f64> {
        None
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        // Read presence from the previous tick and copy to a local Vec.
        let presence = ctx
            .reads_previous()
            .read(self.presence_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("presence field {:?} not readable", self.presence_field),
            })?
            .to_vec();

        match self.mode {
            EmissionMode::Additive => {
                // Read previous emission and copy to a local Vec BEFORE
                // taking the mutable write borrow on the same field.
                let prev_emission = ctx
                    .reads_previous()
                    .read(self.emission_field)
                    .ok_or_else(|| PropagatorError::ExecutionFailed {
                        reason: format!("emission field {:?} not readable", self.emission_field),
                    })?
                    .to_vec();

                // Now take the write borrow.
                let out = ctx.writes().write(self.emission_field).ok_or_else(|| {
                    PropagatorError::ExecutionFailed {
                        reason: format!("emission field {:?} not writable", self.emission_field),
                    }
                })?;

                // Seed from previous emission.
                out.copy_from_slice(&prev_emission);

                // Verify length match to prevent out-of-bounds panic.
                if presence.len() != out.len() {
                    return Err(PropagatorError::ExecutionFailed {
                        reason: format!(
                            "presence field length ({}) != emission field length ({})",
                            presence.len(),
                            out.len()
                        ),
                    });
                }

                // Add intensity where agents are present.
                for (i, &p) in presence.iter().enumerate() {
                    if p != 0.0 {
                        out[i] += self.intensity;
                    }
                }
            }
            EmissionMode::Set => {
                // Take the write borrow and zero-fill.
                let out = ctx.writes().write(self.emission_field).ok_or_else(|| {
                    PropagatorError::ExecutionFailed {
                        reason: format!("emission field {:?} not writable", self.emission_field),
                    }
                })?;

                // Verify length match to prevent out-of-bounds panic.
                if presence.len() != out.len() {
                    return Err(PropagatorError::ExecutionFailed {
                        reason: format!(
                            "presence field length ({}) != emission field length ({})",
                            presence.len(),
                            out.len()
                        ),
                    });
                }

                out.fill(0.0);

                // Set intensity where agents are present.
                for (i, &p) in presence.iter().enumerate() {
                    if p != 0.0 {
                        out[i] = self.intensity;
                    }
                }
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

    const F_PRES: FieldId = FieldId(100);
    const F_EMIT: FieldId = FieldId(101);

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
        // Default mode is Additive, so reads_previous includes both fields.
        let prop = AgentEmission::builder()
            .presence_field(F_PRES)
            .emission_field(F_EMIT)
            .build()
            .unwrap();

        assert_eq!(prop.name(), "AgentEmission");
        assert!(prop.reads().is_empty(), "reads() should be empty");
        let space = crate::test_helpers::test_space();
        assert!(prop.max_dt(&space).is_none());

        // Additive mode reads_previous includes both presence and emission.
        let rp = prop.reads_previous();
        assert!(rp.contains(F_PRES));
        assert!(rp.contains(F_EMIT));

        // Writes the emission field with Full mode.
        let w = prop.writes();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0], (F_EMIT, WriteMode::Full));
    }

    #[test]
    fn builder_rejects_missing_presence() {
        let result = AgentEmission::builder().emission_field(F_EMIT).build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("presence_field"));
    }

    #[test]
    fn builder_rejects_missing_emission() {
        let result = AgentEmission::builder().presence_field(F_PRES).build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("emission_field"));
    }

    #[test]
    fn builder_rejects_zero_intensity() {
        let result = AgentEmission::builder()
            .presence_field(F_PRES)
            .emission_field(F_EMIT)
            .intensity(0.0)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("intensity"));
    }

    // ---------------------------------------------------------------
    // Step logic tests
    // ---------------------------------------------------------------

    #[test]
    fn set_mode_emits_at_agent_positions() {
        // 3x3 grid = 9 cells. Agent at cell 4, intensity=5.0, Set mode.
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let prop = AgentEmission::builder()
            .presence_field(F_PRES)
            .emission_field(F_EMIT)
            .intensity(5.0)
            .mode(EmissionMode::Set)
            .build()
            .unwrap();

        let mut presence = vec![0.0f32; n];
        presence[4] = 1.0;

        let mut reader = MockFieldReader::new();
        reader.set_field(F_PRES, presence);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_EMIT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);

        prop.step(&mut ctx).unwrap();

        let emit = writer.get_field(F_EMIT).unwrap();
        assert_eq!(emit[4], 5.0, "cell 4 should have emission 5.0");
        assert_eq!(emit[0], 0.0, "cell 0 should have emission 0.0");
    }

    #[test]
    fn additive_mode_accumulates() {
        // 3x3 grid = 9 cells. Agent at cell 4, prev_emission[4]=10.0,
        // intensity=3.0, Additive mode => emit[4]=13.0.
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let prop = AgentEmission::builder()
            .presence_field(F_PRES)
            .emission_field(F_EMIT)
            .intensity(3.0)
            .mode(EmissionMode::Additive)
            .build()
            .unwrap();

        let mut presence = vec![0.0f32; n];
        presence[4] = 1.0;

        let mut prev_emission = vec![0.0f32; n];
        prev_emission[4] = 10.0;

        let mut reader = MockFieldReader::new();
        reader.set_field(F_PRES, presence);
        reader.set_field(F_EMIT, prev_emission);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_EMIT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);

        prop.step(&mut ctx).unwrap();

        let emit = writer.get_field(F_EMIT).unwrap();
        assert_eq!(emit[4], 13.0, "cell 4 should have 10.0 + 3.0 = 13.0");
    }

    #[test]
    fn no_agents_no_emission() {
        // 3x3 grid = 9 cells. All presence=0.0, Set mode => all emission=0.0.
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let prop = AgentEmission::builder()
            .presence_field(F_PRES)
            .emission_field(F_EMIT)
            .intensity(5.0)
            .mode(EmissionMode::Set)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_PRES, vec![0.0f32; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_EMIT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);

        prop.step(&mut ctx).unwrap();

        let emit = writer.get_field(F_EMIT).unwrap();
        for (i, &val) in emit.iter().enumerate() {
            assert_eq!(val, 0.0, "cell {i} should be 0.0 with no agents");
        }
    }

    #[test]
    fn builder_rejects_nan_intensity() {
        let result = AgentEmission::builder()
            .presence_field(F_PRES)
            .emission_field(F_EMIT)
            .intensity(f32::NAN)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("intensity"));
    }

    #[test]
    fn additive_no_agents_carries_forward() {
        // Additive mode with no agents should carry previous emission unchanged.
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let prop = AgentEmission::builder()
            .presence_field(F_PRES)
            .emission_field(F_EMIT)
            .intensity(5.0)
            .mode(EmissionMode::Additive)
            .build()
            .unwrap();

        // Previous emission has non-zero values but no agents are present.
        let mut prev_emission = vec![0.0f32; n];
        prev_emission[0] = 7.0;
        prev_emission[4] = 3.0;
        prev_emission[8] = 1.5;

        let mut reader = MockFieldReader::new();
        reader.set_field(F_PRES, vec![0.0f32; n]);
        reader.set_field(F_EMIT, prev_emission.clone());

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_EMIT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);

        prop.step(&mut ctx).unwrap();

        let emit = writer.get_field(F_EMIT).unwrap();
        for (i, &val) in emit.iter().enumerate() {
            assert_eq!(
                val, prev_emission[i],
                "cell {i}: additive with no agents should preserve previous emission"
            );
        }
    }

    #[test]
    fn set_mode_reads_previous_excludes_emission() {
        // In Set mode, reads_previous should only contain presence — not emission.
        let prop = AgentEmission::builder()
            .presence_field(F_PRES)
            .emission_field(F_EMIT)
            .intensity(1.0)
            .mode(EmissionMode::Set)
            .build()
            .unwrap();

        let rp = prop.reads_previous();
        assert!(rp.contains(F_PRES));
        assert!(
            !rp.contains(F_EMIT),
            "Set mode should not read previous emission"
        );
    }
}
