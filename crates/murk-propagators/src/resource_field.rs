//! Consumable resource field propagator.
//!
//! Models a scalar field that agents consume by their presence and that
//! regenerates over time. Supports linear and logistic regrowth models.
//!
//! Common in foraging/harvesting environments: agents deplete local
//! resources, which then regrow toward a carrying capacity.
//!
//! Constructed via the builder pattern: [`ResourceField::builder`].

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};

/// Regrowth model for [`ResourceField`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegrowthModel {
    /// Linear: `v += regrowth_rate * dt` (constant rate, capped at capacity).
    Linear,
    /// Logistic: `v += regrowth_rate * v * (1 - v/capacity) * dt`
    /// (density-dependent, S-curve toward capacity).
    Logistic,
}

/// A consumable resource field propagator.
///
/// Each tick:
/// 1. Copies previous-tick resource values.
/// 2. Subtracts `consumption_rate * dt` at cells where agents are present.
/// 3. Applies regrowth (linear or logistic).
/// 4. Clamps to `[0, capacity]`.
#[derive(Debug)]
pub struct ResourceField {
    field: FieldId,
    presence_field: FieldId,
    consumption_rate: f32,
    regrowth_rate: f32,
    capacity: f32,
    regrowth_model: RegrowthModel,
}

/// Builder for [`ResourceField`].
///
/// Required fields: `field` and `presence_field`.
pub struct ResourceFieldBuilder {
    field: Option<FieldId>,
    presence_field: Option<FieldId>,
    consumption_rate: f32,
    regrowth_rate: f32,
    capacity: f32,
    regrowth_model: RegrowthModel,
}

impl ResourceField {
    /// Create a new builder for configuring a `ResourceField` propagator.
    pub fn builder() -> ResourceFieldBuilder {
        ResourceFieldBuilder {
            field: None,
            presence_field: None,
            consumption_rate: 1.0,
            regrowth_rate: 0.1,
            capacity: 1.0,
            regrowth_model: RegrowthModel::Linear,
        }
    }
}

impl ResourceFieldBuilder {
    /// Set the resource field (read previous, write current).
    pub fn field(mut self, field: FieldId) -> Self {
        self.field = Some(field);
        self
    }

    /// Set the agent presence field (read previous).
    pub fn presence_field(mut self, field: FieldId) -> Self {
        self.presence_field = Some(field);
        self
    }

    /// Set the consumption rate per tick per agent (default: 1.0). Must be >= 0.
    pub fn consumption_rate(mut self, rate: f32) -> Self {
        self.consumption_rate = rate;
        self
    }

    /// Set the regrowth rate (default: 0.1). Must be >= 0.
    pub fn regrowth_rate(mut self, rate: f32) -> Self {
        self.regrowth_rate = rate;
        self
    }

    /// Set the carrying capacity (default: 1.0). Must be > 0.
    pub fn capacity(mut self, cap: f32) -> Self {
        self.capacity = cap;
        self
    }

    /// Set the regrowth model (default: Linear).
    pub fn regrowth_model(mut self, model: RegrowthModel) -> Self {
        self.regrowth_model = model;
        self
    }

    /// Build the propagator, validating all configuration.
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - `field` is not set
    /// - `presence_field` is not set
    /// - `consumption_rate` is negative or NaN
    /// - `regrowth_rate` is negative or NaN
    /// - `capacity` is not > 0 or is NaN
    pub fn build(self) -> Result<ResourceField, String> {
        let field = self.field.ok_or_else(|| "field is required".to_string())?;
        let presence_field = self
            .presence_field
            .ok_or_else(|| "presence_field is required".to_string())?;

        if !self.consumption_rate.is_finite() || self.consumption_rate < 0.0 {
            return Err(format!(
                "consumption_rate must be finite and >= 0, got {}",
                self.consumption_rate
            ));
        }
        if !self.regrowth_rate.is_finite() || self.regrowth_rate < 0.0 {
            return Err(format!(
                "regrowth_rate must be finite and >= 0, got {}",
                self.regrowth_rate
            ));
        }
        if !self.capacity.is_finite() || self.capacity <= 0.0 {
            return Err(format!(
                "capacity must be finite and > 0, got {}",
                self.capacity
            ));
        }

        Ok(ResourceField {
            field,
            presence_field,
            consumption_rate: self.consumption_rate,
            regrowth_rate: self.regrowth_rate,
            capacity: self.capacity,
            regrowth_model: self.regrowth_model,
        })
    }
}

impl Propagator for ResourceField {
    fn name(&self) -> &str {
        "ResourceField"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        [self.field, self.presence_field].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.field, WriteMode::Full)]
    }

    fn max_dt(&self, _space: &dyn murk_space::Space) -> Option<f64> {
        None
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let dt = ctx.dt() as f32;

        let prev = ctx
            .reads_previous()
            .read(self.field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("resource field {:?} not readable", self.field),
            })?
            .to_vec();

        let presence = ctx
            .reads_previous()
            .read(self.presence_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("presence field {:?} not readable", self.presence_field),
            })?
            .to_vec();

        let out =
            ctx.writes()
                .write(self.field)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: format!("resource field {:?} not writable", self.field),
                })?;

        // Length mismatch defense
        if prev.len() != out.len() {
            return Err(PropagatorError::ExecutionFailed {
                reason: format!(
                    "resource field previous length ({}) != output length ({})",
                    prev.len(),
                    out.len()
                ),
            });
        }
        if presence.len() != out.len() {
            return Err(PropagatorError::ExecutionFailed {
                reason: format!(
                    "presence field length ({}) != resource field length ({})",
                    presence.len(),
                    out.len()
                ),
            });
        }

        for i in 0..out.len() {
            let mut v = prev[i];

            // Consumption: subtract where agents are present
            if presence[i] != 0.0 {
                v -= self.consumption_rate * dt;
            }

            // Regrowth
            match self.regrowth_model {
                RegrowthModel::Linear => {
                    v += self.regrowth_rate * dt;
                }
                RegrowthModel::Logistic => {
                    // Logistic growth: rate * v * (1 - v/cap)
                    // Only applies when v > 0 (dead resources don't regrow logistically)
                    if v > 0.0 {
                        v += self.regrowth_rate * v * (1.0 - v / self.capacity) * dt;
                    }
                }
            }

            // Clamp to [0, capacity]
            out[i] = v.clamp(0.0, self.capacity);
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

    const F_RES: FieldId = FieldId(100);
    const F_PRES: FieldId = FieldId(101);

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
        let prop = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .build()
            .unwrap();

        assert_eq!(prop.name(), "ResourceField");
        assert!(prop.reads().is_empty(), "reads() should be empty");
        let space = crate::test_helpers::test_space();
        assert!(prop.max_dt(&space).is_none());

        let rp = prop.reads_previous();
        assert!(rp.contains(F_RES));
        assert!(rp.contains(F_PRES));

        let w = prop.writes();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0], (F_RES, WriteMode::Full));
    }

    #[test]
    fn builder_rejects_missing_field() {
        let result = ResourceField::builder().presence_field(F_PRES).build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("field"));
    }

    #[test]
    fn builder_rejects_missing_presence() {
        let result = ResourceField::builder().field(F_RES).build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("presence_field"));
    }

    #[test]
    fn builder_rejects_negative_consumption() {
        let result = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .consumption_rate(-1.0)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("consumption_rate"));
    }

    #[test]
    fn builder_rejects_nan_consumption() {
        let result = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .consumption_rate(f32::NAN)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("consumption_rate"));
    }

    #[test]
    fn builder_rejects_zero_capacity() {
        let result = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .capacity(0.0)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("capacity"));
    }

    #[test]
    fn builder_rejects_nan_capacity() {
        let result = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .capacity(f32::NAN)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("capacity"));
    }

    // ---------------------------------------------------------------
    // Step logic tests
    // ---------------------------------------------------------------

    #[test]
    fn consumption_reduces_resource() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let prop = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .consumption_rate(10.0)
            .regrowth_rate(0.0) // no regrowth
            .capacity(100.0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_RES, vec![50.0; n]);
        let mut presence = vec![0.0f32; n];
        presence[4] = 1.0; // agent at center
        reader.set_field(F_PRES, presence);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_RES, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1.0);
        prop.step(&mut ctx).unwrap();

        let res = writer.get_field(F_RES).unwrap();
        assert!(
            (res[4] - 40.0).abs() < 1e-6,
            "consumption: 50 - 10*1.0 = 40, got {}",
            res[4]
        );
        assert!(
            (res[0] - 50.0).abs() < 1e-6,
            "no agent: resource unchanged at {}",
            res[0]
        );
    }

    #[test]
    fn linear_regrowth() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let prop = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .consumption_rate(0.0)
            .regrowth_rate(5.0)
            .capacity(100.0)
            .regrowth_model(RegrowthModel::Linear)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_RES, vec![10.0; n]);
        reader.set_field(F_PRES, vec![0.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_RES, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1.0);
        prop.step(&mut ctx).unwrap();

        let res = writer.get_field(F_RES).unwrap();
        assert!(
            (res[0] - 15.0).abs() < 1e-6,
            "linear: 10 + 5*1.0 = 15, got {}",
            res[0]
        );
    }

    #[test]
    fn logistic_regrowth_slows_near_capacity() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let prop = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .consumption_rate(0.0)
            .regrowth_rate(1.0)
            .capacity(100.0)
            .regrowth_model(RegrowthModel::Logistic)
            .build()
            .unwrap();

        // Low resource: should regrow fast
        let mut reader_low = MockFieldReader::new();
        reader_low.set_field(F_RES, vec![20.0; n]);
        reader_low.set_field(F_PRES, vec![0.0; n]);

        let mut writer_low = MockFieldWriter::new();
        writer_low.add_field(F_RES, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader_low, &mut writer_low, &mut scratch, &grid, 1.0);
        prop.step(&mut ctx).unwrap();
        let low_growth = writer_low.get_field(F_RES).unwrap()[0] - 20.0;

        // High resource: should regrow slower
        let mut reader_high = MockFieldReader::new();
        reader_high.set_field(F_RES, vec![90.0; n]);
        reader_high.set_field(F_PRES, vec![0.0; n]);

        let mut writer_high = MockFieldWriter::new();
        writer_high.add_field(F_RES, n);

        let mut scratch2 = ScratchRegion::new(0);
        let mut ctx2 = make_ctx(&reader_high, &mut writer_high, &mut scratch2, &grid, 1.0);
        prop.step(&mut ctx2).unwrap();
        let high_growth = writer_high.get_field(F_RES).unwrap()[0] - 90.0;

        assert!(
            low_growth > high_growth,
            "logistic: low resource ({low_growth}) should grow faster than high ({high_growth})"
        );
    }

    #[test]
    fn capacity_clamp() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let prop = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .consumption_rate(0.0)
            .regrowth_rate(100.0)
            .capacity(50.0)
            .regrowth_model(RegrowthModel::Linear)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_RES, vec![40.0; n]);
        reader.set_field(F_PRES, vec![0.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_RES, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1.0);
        prop.step(&mut ctx).unwrap();

        let res = writer.get_field(F_RES).unwrap();
        assert!(
            (res[0] - 50.0).abs() < 1e-6,
            "should clamp to capacity 50, got {}",
            res[0]
        );
    }

    #[test]
    fn floor_clamp_at_zero() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let prop = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .consumption_rate(100.0)
            .regrowth_rate(0.0)
            .capacity(50.0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_RES, vec![5.0; n]);
        let mut presence = vec![0.0f32; n];
        presence[0] = 1.0;
        reader.set_field(F_PRES, presence);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_RES, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1.0);
        prop.step(&mut ctx).unwrap();

        let res = writer.get_field(F_RES).unwrap();
        assert!(res[0].abs() < 1e-6, "should clamp to 0, got {}", res[0]);
    }
}
