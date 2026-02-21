//! Configurable noise injection propagator.
//!
//! Adds deterministic noise to a field each tick. Useful for stochastic
//! environments, partial observability, and robustness training.
//!
//! Respects the determinism contract: uses a seeded ChaCha8 RNG
//! derived from `seed_offset XOR tick_id`, producing identical noise
//! sequences for identical seeds.
//!
//! Three noise types:
//! - **Gaussian**: `v += scale * N(0,1)` (Box-Muller transform)
//! - **Uniform**: `v += scale * U(-1,1)`
//! - **SaltPepper**: with probability `scale`, set to 0.0 or 1.0
//!
//! Constructed via the builder pattern: [`NoiseInjection::builder`].

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;

/// Noise type for [`NoiseInjection`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseType {
    /// Additive Gaussian noise: `v += scale * N(0,1)`.
    Gaussian,
    /// Additive uniform noise: `v += scale * U(-1,1)`.
    Uniform,
    /// Salt-and-pepper noise: with probability `scale`, replace value
    /// with 0.0 or 1.0 (equal chance).
    SaltPepper,
}

/// A deterministic noise injection propagator.
///
/// Copies previous-tick field values and adds noise. The RNG is seeded
/// from `seed_offset XOR tick_id` each tick, ensuring deterministic
/// replay with the same configuration.
#[derive(Debug)]
pub struct NoiseInjection {
    field: FieldId,
    noise_type: NoiseType,
    scale: f64,
    seed_offset: u64,
}

/// Builder for [`NoiseInjection`].
///
/// Required field: `field`.
pub struct NoiseInjectionBuilder {
    field: Option<FieldId>,
    noise_type: NoiseType,
    scale: f64,
    seed_offset: u64,
}

impl NoiseInjection {
    /// Create a new builder for configuring a `NoiseInjection` propagator.
    pub fn builder() -> NoiseInjectionBuilder {
        NoiseInjectionBuilder {
            field: None,
            noise_type: NoiseType::Gaussian,
            scale: 0.1,
            seed_offset: 0,
        }
    }

    /// Generate a Gaussian sample using Box-Muller transform.
    /// Avoids the `rand_distr` dependency.
    fn box_muller(rng: &mut ChaCha8Rng) -> f64 {
        let u1: f64 = rng.gen::<f64>().max(1e-300); // avoid ln(0)
        let u2: f64 = rng.gen();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

impl NoiseInjectionBuilder {
    /// Set the field to inject noise into.
    pub fn field(mut self, field: FieldId) -> Self {
        self.field = Some(field);
        self
    }

    /// Set the noise type (default: Gaussian).
    pub fn noise_type(mut self, noise_type: NoiseType) -> Self {
        self.noise_type = noise_type;
        self
    }

    /// Set the noise scale (default: 0.1). Must be >= 0.
    ///
    /// For Gaussian: standard deviation. For Uniform: half-range.
    /// For SaltPepper: probability of replacement per cell (must be <= 1.0).
    pub fn scale(mut self, scale: f64) -> Self {
        self.scale = scale;
        self
    }

    /// Set the seed offset for deterministic RNG (default: 0).
    pub fn seed_offset(mut self, offset: u64) -> Self {
        self.seed_offset = offset;
        self
    }

    /// Build the propagator, validating all configuration.
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// - `field` is not set
    /// - `scale` is negative or NaN
    /// - `noise_type` is SaltPepper and `scale` > 1.0
    pub fn build(self) -> Result<NoiseInjection, String> {
        let field = self.field.ok_or_else(|| "field is required".to_string())?;

        if !self.scale.is_finite() || self.scale < 0.0 {
            return Err(format!("scale must be finite and >= 0, got {}", self.scale));
        }

        if self.noise_type == NoiseType::SaltPepper && self.scale > 1.0 {
            return Err(format!(
                "SaltPepper scale is a probability and must be <= 1.0, got {}",
                self.scale
            ));
        }

        Ok(NoiseInjection {
            field,
            noise_type: self.noise_type,
            scale: self.scale,
            seed_offset: self.seed_offset,
        })
    }
}

impl Propagator for NoiseInjection {
    fn name(&self) -> &str {
        "NoiseInjection"
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
        let tick = ctx.tick_id().0;

        let prev = ctx
            .reads_previous()
            .read(self.field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("field {:?} not readable", self.field),
            })?
            .to_vec();

        let out =
            ctx.writes()
                .write(self.field)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: format!("field {:?} not writable", self.field),
                })?;

        // Length mismatch defense
        if prev.len() != out.len() {
            return Err(PropagatorError::ExecutionFailed {
                reason: format!(
                    "field previous length ({}) != output length ({})",
                    prev.len(),
                    out.len()
                ),
            });
        }

        out.copy_from_slice(&prev);

        // Deterministic RNG seeded from seed_offset XOR tick_id
        let mut rng = ChaCha8Rng::seed_from_u64(self.seed_offset ^ tick);

        match self.noise_type {
            NoiseType::Gaussian => {
                for v in out.iter_mut() {
                    *v += (self.scale * Self::box_muller(&mut rng)) as f32;
                }
            }
            NoiseType::Uniform => {
                for v in out.iter_mut() {
                    let u: f64 = rng.gen::<f64>() * 2.0 - 1.0;
                    *v += (self.scale * u) as f32;
                }
            }
            NoiseType::SaltPepper => {
                for v in out.iter_mut() {
                    let p: f64 = rng.gen();
                    if p < self.scale {
                        // Coin flip for salt (1.0) or pepper (0.0)
                        *v = if rng.gen::<bool>() { 1.0 } else { 0.0 };
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

    const F_DATA: FieldId = FieldId(100);

    fn make_ctx<'a>(
        reader: &'a MockFieldReader,
        writer: &'a mut MockFieldWriter,
        scratch: &'a mut ScratchRegion,
        space: &'a Square4,
        tick: u64,
    ) -> StepContext<'a> {
        StepContext::new(reader, reader, writer, scratch, space, TickId(tick), 0.1)
    }

    // ---------------------------------------------------------------
    // Builder tests
    // ---------------------------------------------------------------

    #[test]
    fn builder_minimal() {
        let prop = NoiseInjection::builder().field(F_DATA).build().unwrap();

        assert_eq!(prop.name(), "NoiseInjection");
        assert!(prop.reads().is_empty(), "reads() should be empty");
        assert!(prop.max_dt().is_none());

        let rp = prop.reads_previous();
        assert!(rp.contains(F_DATA));

        let w = prop.writes();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0], (F_DATA, WriteMode::Full));
    }

    #[test]
    fn builder_rejects_missing_field() {
        let result = NoiseInjection::builder().build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("field"));
    }

    #[test]
    fn builder_rejects_negative_scale() {
        let result = NoiseInjection::builder().field(F_DATA).scale(-1.0).build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("scale"));
    }

    #[test]
    fn builder_rejects_nan_scale() {
        let result = NoiseInjection::builder()
            .field(F_DATA)
            .scale(f64::NAN)
            .build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("scale"));
    }

    #[test]
    fn builder_rejects_salt_pepper_scale_over_one() {
        let result = NoiseInjection::builder()
            .field(F_DATA)
            .noise_type(NoiseType::SaltPepper)
            .scale(1.5)
            .build();
        assert!(result.is_err());
    }

    // ---------------------------------------------------------------
    // Step logic tests
    // ---------------------------------------------------------------

    #[test]
    fn determinism_same_seed_same_output() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = NoiseInjection::builder()
            .field(F_DATA)
            .noise_type(NoiseType::Gaussian)
            .scale(1.0)
            .seed_offset(42)
            .build()
            .unwrap();

        let run = |tick: u64| -> Vec<f32> {
            let mut reader = MockFieldReader::new();
            reader.set_field(F_DATA, vec![10.0; n]);
            let mut writer = MockFieldWriter::new();
            writer.add_field(F_DATA, n);
            let mut scratch = ScratchRegion::new(0);
            let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, tick);
            prop.step(&mut ctx).unwrap();
            writer.get_field(F_DATA).unwrap().to_vec()
        };

        let a = run(5);
        let b = run(5);
        assert_eq!(a, b, "same tick + same seed -> bit-identical output");
    }

    #[test]
    fn different_ticks_different_output() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = NoiseInjection::builder()
            .field(F_DATA)
            .noise_type(NoiseType::Gaussian)
            .scale(1.0)
            .seed_offset(42)
            .build()
            .unwrap();

        let run = |tick: u64| -> Vec<f32> {
            let mut reader = MockFieldReader::new();
            reader.set_field(F_DATA, vec![10.0; n]);
            let mut writer = MockFieldWriter::new();
            writer.add_field(F_DATA, n);
            let mut scratch = ScratchRegion::new(0);
            let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, tick);
            prop.step(&mut ctx).unwrap();
            writer.get_field(F_DATA).unwrap().to_vec()
        };

        let a = run(1);
        let b = run(2);
        assert_ne!(a, b, "different ticks should produce different noise");
    }

    #[test]
    fn gaussian_noise_changes_values() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = NoiseInjection::builder()
            .field(F_DATA)
            .noise_type(NoiseType::Gaussian)
            .scale(1.0)
            .seed_offset(0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_DATA, vec![0.0; n]);
        let mut writer = MockFieldWriter::new();
        writer.add_field(F_DATA, n);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_DATA).unwrap();
        let non_zero = out.iter().filter(|&&v| v.abs() > 1e-6).count();
        assert!(
            non_zero > 0,
            "Gaussian noise should produce non-zero values"
        );
    }

    #[test]
    fn uniform_noise_bounded() {
        let grid = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let scale = 0.5;
        let prop = NoiseInjection::builder()
            .field(F_DATA)
            .noise_type(NoiseType::Uniform)
            .scale(scale)
            .seed_offset(0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_DATA, vec![10.0; n]);
        let mut writer = MockFieldWriter::new();
        writer.add_field(F_DATA, n);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_DATA).unwrap();
        for &v in out {
            assert!(
                v >= 10.0 - scale as f32 && v <= 10.0 + scale as f32,
                "uniform noise should be bounded: 10 +/- {scale}, got {v}"
            );
        }
    }

    #[test]
    fn salt_pepper_only_zero_or_one() {
        let grid = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = NoiseInjection::builder()
            .field(F_DATA)
            .noise_type(NoiseType::SaltPepper)
            .scale(1.0) // replace all cells
            .seed_offset(0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_DATA, vec![0.5; n]);
        let mut writer = MockFieldWriter::new();
        writer.add_field(F_DATA, n);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_DATA).unwrap();
        for &v in out {
            assert!(
                v == 0.0 || v == 1.0,
                "SaltPepper with scale=1.0 should produce only 0 or 1, got {v}"
            );
        }
    }
}
