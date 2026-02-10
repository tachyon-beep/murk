//! World configuration, validation, and error types.
//!
//! [`WorldConfig`] is the builder-input for constructing a simulation world.
//! [`validate()`](WorldConfig::validate) checks structural invariants at
//! startup; the actual world constructor (WP-5b) calls `validate_pipeline()`
//! directly to obtain the [`ReadResolutionPlan`](murk_propagator::ReadResolutionPlan).

use std::error::Error;
use std::fmt;

use murk_arena::ArenaError;
use murk_core::{FieldDef, FieldId, FieldSet};
use murk_propagator::{validate_pipeline, PipelineError, Propagator};
use murk_space::Space;

// ── BackoffConfig ──────────────────────────────────────────────────

/// Configuration for the adaptive command rejection backoff (§6.11).
///
/// When consecutive tick rollbacks occur, the engine increases the
/// allowed skew between the command's basis tick and the current tick.
/// This struct controls the shape of that backoff curve.
#[derive(Clone, Debug)]
pub struct BackoffConfig {
    /// Initial maximum skew tolerance (ticks). Default: 2.
    pub initial_max_skew: u64,
    /// Multiplicative factor applied on each consecutive rollback. Default: 1.5.
    pub backoff_factor: f64,
    /// Upper bound on the skew tolerance. Default: 10.
    pub max_skew_cap: u64,
    /// Number of ticks after last rollback before skew resets. Default: 60.
    pub decay_rate: u64,
    /// Fraction of rejected commands that triggers proactive backoff. Default: 0.20.
    pub rejection_rate_threshold: f64,
}

impl Default for BackoffConfig {
    fn default() -> Self {
        Self {
            initial_max_skew: 2,
            backoff_factor: 1.5,
            max_skew_cap: 10,
            decay_rate: 60,
            rejection_rate_threshold: 0.20,
        }
    }
}

// ── ConfigError ────────────────────────────────────────────────────

/// Errors detected during [`WorldConfig::validate()`].
#[derive(Debug)]
pub enum ConfigError {
    /// Propagator pipeline validation failed.
    Pipeline(PipelineError),
    /// Arena configuration is invalid.
    Arena(ArenaError),
    /// Space has zero cells.
    EmptySpace,
    /// No fields registered.
    NoFields,
    /// Ring buffer size is below the minimum of 2.
    RingBufferTooSmall {
        /// The configured size that was too small.
        configured: usize,
    },
    /// Ingress queue capacity is zero.
    IngressQueueZero,
    /// tick_rate_hz is NaN, infinite, zero, or negative.
    InvalidTickRate {
        /// The invalid value.
        value: f64,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pipeline(e) => write!(f, "pipeline: {e}"),
            Self::Arena(e) => write!(f, "arena: {e}"),
            Self::EmptySpace => write!(f, "space has zero cells"),
            Self::NoFields => write!(f, "no fields registered"),
            Self::RingBufferTooSmall { configured } => {
                write!(f, "ring_buffer_size {configured} is below minimum of 2")
            }
            Self::IngressQueueZero => write!(f, "max_ingress_queue must be at least 1"),
            Self::InvalidTickRate { value } => {
                write!(f, "tick_rate_hz must be finite and positive, got {value}")
            }
        }
    }
}

impl Error for ConfigError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Pipeline(e) => Some(e),
            Self::Arena(e) => Some(e),
            _ => None,
        }
    }
}

impl From<PipelineError> for ConfigError {
    fn from(e: PipelineError) -> Self {
        Self::Pipeline(e)
    }
}

impl From<ArenaError> for ConfigError {
    fn from(e: ArenaError) -> Self {
        Self::Arena(e)
    }
}

// ── WorldConfig ────────────────────────────────────────────────────

/// Complete configuration for constructing a simulation world.
///
/// Passed to the world constructor (WP-5b). `validate()` checks all
/// structural invariants without producing intermediate artifacts.
pub struct WorldConfig {
    /// Spatial topology for the simulation.
    pub space: Box<dyn Space>,
    /// Field definitions. `FieldId(n)` corresponds to `fields[n]`.
    pub fields: Vec<FieldDef>,
    /// Propagators executed in pipeline order each tick.
    pub propagators: Vec<Box<dyn Propagator>>,
    /// Simulation timestep in seconds.
    pub dt: f64,
    /// RNG seed for deterministic simulation.
    pub seed: u64,
    /// Number of snapshots retained in the ring buffer. Default: 8. Minimum: 2.
    pub ring_buffer_size: usize,
    /// Maximum commands buffered in the ingress queue. Default: 1024.
    pub max_ingress_queue: usize,
    /// Optional target tick rate for realtime-async mode.
    pub tick_rate_hz: Option<f64>,
    /// Adaptive backoff configuration.
    pub backoff: BackoffConfig,
}

impl WorldConfig {
    /// Validate all structural invariants.
    ///
    /// This is a pure validation pass — it does not return a
    /// `ReadResolutionPlan`. The world constructor calls
    /// `validate_pipeline()` directly to obtain the plan.
    pub fn validate(&self) -> Result<(), ConfigError> {
        // 1. Space must have at least one cell.
        if self.space.cell_count() == 0 {
            return Err(ConfigError::EmptySpace);
        }
        // 2. Must have at least one field.
        if self.fields.is_empty() {
            return Err(ConfigError::NoFields);
        }
        // 3. Ring buffer >= 2.
        if self.ring_buffer_size < 2 {
            return Err(ConfigError::RingBufferTooSmall {
                configured: self.ring_buffer_size,
            });
        }
        // 4. Ingress queue >= 1.
        if self.max_ingress_queue == 0 {
            return Err(ConfigError::IngressQueueZero);
        }
        // 5. tick_rate_hz, if present, must be finite and positive.
        if let Some(hz) = self.tick_rate_hz {
            if !hz.is_finite() || hz <= 0.0 {
                return Err(ConfigError::InvalidTickRate { value: hz });
            }
        }
        // 6. Pipeline validation (delegates to murk-propagator).
        let defined = self.defined_field_set();
        validate_pipeline(&self.propagators, &defined, self.dt)?;

        Ok(())
    }

    /// Build a [`FieldSet`] from the configured field definitions.
    pub(crate) fn defined_field_set(&self) -> FieldSet {
        (0..self.fields.len())
            .map(|i| FieldId(i as u32))
            .collect()
    }
}

impl fmt::Debug for WorldConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorldConfig")
            .field("space_ndim", &self.space.ndim())
            .field("space_cell_count", &self.space.cell_count())
            .field("fields", &self.fields.len())
            .field("propagators", &self.propagators.len())
            .field("dt", &self.dt)
            .field("seed", &self.seed)
            .field("ring_buffer_size", &self.ring_buffer_size)
            .field("max_ingress_queue", &self.max_ingress_queue)
            .field("tick_rate_hz", &self.tick_rate_hz)
            .field("backoff", &self.backoff)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::{BoundaryBehavior, FieldMutability, FieldType};
    use murk_space::{EdgeBehavior, Line1D};
    use murk_test_utils::ConstPropagator;

    fn scalar_field(name: &str) -> FieldDef {
        FieldDef {
            name: name.to_string(),
            field_type: FieldType::Scalar,
            mutability: FieldMutability::PerTick,
            units: None,
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        }
    }

    fn valid_config() -> WorldConfig {
        WorldConfig {
            space: Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()),
            fields: vec![scalar_field("energy")],
            propagators: vec![Box::new(ConstPropagator::new(
                "const",
                FieldId(0),
                1.0,
            ))],
            dt: 0.1,
            seed: 42,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: BackoffConfig::default(),
        }
    }

    #[test]
    fn validate_valid_config_succeeds() {
        assert!(valid_config().validate().is_ok());
    }

    #[test]
    fn validate_empty_propagators_fails() {
        let mut cfg = valid_config();
        cfg.propagators.clear();
        match cfg.validate() {
            Err(ConfigError::Pipeline(PipelineError::EmptyPipeline)) => {}
            other => panic!("expected Pipeline(EmptyPipeline), got {other:?}"),
        }
    }

    #[test]
    fn validate_invalid_dt_fails() {
        let mut cfg = valid_config();
        cfg.dt = f64::NAN;
        match cfg.validate() {
            Err(ConfigError::Pipeline(PipelineError::InvalidDt { .. })) => {}
            other => panic!("expected Pipeline(InvalidDt), got {other:?}"),
        }
    }

    #[test]
    fn validate_write_conflict_fails() {
        let mut cfg = valid_config();
        // Two propagators writing the same field.
        cfg.propagators.push(Box::new(ConstPropagator::new(
            "conflict",
            FieldId(0),
            2.0,
        )));
        match cfg.validate() {
            Err(ConfigError::Pipeline(PipelineError::WriteConflict(_))) => {}
            other => panic!("expected Pipeline(WriteConflict), got {other:?}"),
        }
    }

    #[test]
    fn validate_dt_exceeds_max_dt_fails() {
        use murk_core::PropagatorError;
        use murk_propagator::context::StepContext;
        use murk_propagator::propagator::WriteMode;

        struct CflProp;
        impl Propagator for CflProp {
            fn name(&self) -> &str {
                "cfl"
            }
            fn reads(&self) -> FieldSet {
                FieldSet::empty()
            }
            fn writes(&self) -> Vec<(FieldId, WriteMode)> {
                vec![(FieldId(0), WriteMode::Full)]
            }
            fn max_dt(&self) -> Option<f64> {
                Some(0.01)
            }
            fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
                Ok(())
            }
        }

        let mut cfg = valid_config();
        cfg.propagators = vec![Box::new(CflProp)];
        cfg.dt = 0.1;
        match cfg.validate() {
            Err(ConfigError::Pipeline(PipelineError::DtTooLarge { .. })) => {}
            other => panic!("expected Pipeline(DtTooLarge), got {other:?}"),
        }
    }

    #[test]
    fn validate_empty_space_fails() {
        use murk_space::error::SpaceError;
        // Line1D::new(0, ...) returns Err, so we need a custom space with 0 cells.
        struct EmptySpace;
        impl Space for EmptySpace {
            fn ndim(&self) -> usize {
                1
            }
            fn cell_count(&self) -> usize {
                0
            }
            fn neighbours(&self, _: &murk_core::Coord) -> smallvec::SmallVec<[murk_core::Coord; 8]> {
                smallvec::smallvec![]
            }
            fn distance(&self, _: &murk_core::Coord, _: &murk_core::Coord) -> f64 {
                0.0
            }
            fn compile_region(
                &self,
                _: &murk_space::RegionSpec,
            ) -> Result<murk_space::RegionPlan, SpaceError> {
                Err(SpaceError::EmptySpace)
            }
            fn canonical_ordering(&self) -> Vec<murk_core::Coord> {
                vec![]
            }
        }

        let mut cfg = valid_config();
        cfg.space = Box::new(EmptySpace);
        match cfg.validate() {
            Err(ConfigError::EmptySpace) => {}
            other => panic!("expected EmptySpace, got {other:?}"),
        }
    }

    #[test]
    fn validate_no_fields_fails() {
        let mut cfg = valid_config();
        cfg.fields.clear();
        match cfg.validate() {
            Err(ConfigError::NoFields) => {}
            other => panic!("expected NoFields, got {other:?}"),
        }
    }
}
