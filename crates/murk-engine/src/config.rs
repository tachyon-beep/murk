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

// ── AsyncConfig ───────────────────────────────────────────────────

/// Configuration for [`RealtimeAsyncWorld`](crate::realtime::RealtimeAsyncWorld).
///
/// Controls the egress worker pool size and epoch-hold budget that
/// governs the shutdown state machine and stalled-worker detection.
#[derive(Clone, Debug)]
pub struct AsyncConfig {
    /// Number of egress worker threads. `None` = auto-detect
    /// (`available_parallelism / 2`, clamped to `[2, 16]`).
    pub worker_count: Option<usize>,
    /// Maximum milliseconds a worker may hold an epoch pin before being
    /// considered stalled and forcibly unpinned. Default: 100.
    pub max_epoch_hold_ms: u64,
    /// Grace period (ms) after cancellation before the worker is
    /// forcibly unpinned. Default: 10.
    pub cancel_grace_ms: u64,
}

impl Default for AsyncConfig {
    fn default() -> Self {
        Self {
            worker_count: None,
            max_epoch_hold_ms: 100,
            cancel_grace_ms: 10,
        }
    }
}

impl AsyncConfig {
    /// Resolve the actual worker count, applying auto-detection if `None`.
    ///
    /// Explicit values are clamped to `[1, 64]`. Zero workers would
    /// create an unusable world (no egress threads to service observations).
    pub fn resolved_worker_count(&self) -> usize {
        match self.worker_count {
            Some(n) => n.clamp(1, 64),
            None => {
                let cpus = std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(4);
                (cpus / 2).clamp(2, 16)
            }
        }
    }
}

// ── ConfigError ────────────────────────────────────────────────────

/// Errors detected during [`WorldConfig::validate()`].
#[derive(Debug, PartialEq)]
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
    /// BackoffConfig invariant violated.
    InvalidBackoff {
        /// Description of which invariant was violated.
        reason: String,
    },
    /// Cell count or field count exceeds `u32::MAX`.
    CellCountOverflow {
        /// The value that overflowed.
        value: usize,
    },
    /// A field definition failed validation.
    InvalidField {
        /// Description of the validation failure.
        reason: String,
    },
    /// Engine could not be recovered from tick thread (e.g. thread panicked).
    EngineRecoveryFailed,
    /// A background thread could not be spawned.
    ThreadSpawnFailed {
        /// Description of which thread failed.
        reason: String,
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
            Self::InvalidBackoff { reason } => {
                write!(f, "invalid backoff config: {reason}")
            }
            Self::CellCountOverflow { value } => {
                write!(f, "cell count {value} exceeds u32::MAX")
            }
            Self::InvalidField { reason } => {
                write!(f, "invalid field: {reason}")
            }
            Self::EngineRecoveryFailed => {
                write!(f, "engine could not be recovered from tick thread")
            }
            Self::ThreadSpawnFailed { reason } => {
                write!(f, "thread spawn failed: {reason}")
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
        // 2a. Each field must pass structural validation.
        for field in &self.fields {
            field
                .validate()
                .map_err(|reason| ConfigError::InvalidField { reason })?;
        }
        // 2b. Cell count must fit in u32 (arena uses u32 internally).
        let cell_count = self.space.cell_count();
        if u32::try_from(cell_count).is_err() {
            return Err(ConfigError::CellCountOverflow { value: cell_count });
        }
        // 2c. Field count must fit in u32 (FieldId is u32).
        if u32::try_from(self.fields.len()).is_err() {
            return Err(ConfigError::CellCountOverflow {
                value: self.fields.len(),
            });
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
        // 5. tick_rate_hz, if present, must be finite and positive, and
        //    its reciprocal must also be finite (rejects subnormals where
        //    1.0/hz = inf, which would panic in Duration::from_secs_f64).
        if let Some(hz) = self.tick_rate_hz {
            if !hz.is_finite() || hz <= 0.0 || !(1.0 / hz).is_finite() {
                return Err(ConfigError::InvalidTickRate { value: hz });
            }
        }
        // 6. BackoffConfig invariants.
        let b = &self.backoff;
        if b.initial_max_skew > b.max_skew_cap {
            return Err(ConfigError::InvalidBackoff {
                reason: format!(
                    "initial_max_skew ({}) exceeds max_skew_cap ({})",
                    b.initial_max_skew, b.max_skew_cap,
                ),
            });
        }
        if !b.backoff_factor.is_finite() || b.backoff_factor < 1.0 {
            return Err(ConfigError::InvalidBackoff {
                reason: format!(
                    "backoff_factor must be finite and >= 1.0, got {}",
                    b.backoff_factor,
                ),
            });
        }
        if !b.rejection_rate_threshold.is_finite()
            || b.rejection_rate_threshold < 0.0
            || b.rejection_rate_threshold > 1.0
        {
            return Err(ConfigError::InvalidBackoff {
                reason: format!(
                    "rejection_rate_threshold must be in [0.0, 1.0], got {}",
                    b.rejection_rate_threshold,
                ),
            });
        }
        if b.decay_rate == 0 {
            return Err(ConfigError::InvalidBackoff {
                reason: "decay_rate must be at least 1".to_string(),
            });
        }

        // 7. Pipeline validation (delegates to murk-propagator).
        //    The plan is intentionally discarded here — the world constructor
        //    calls validate_pipeline() again to obtain it.
        let defined = self.defined_field_set();
        let _ = validate_pipeline(&self.propagators, &defined, self.dt, &*self.space)?;

        Ok(())
    }

    /// Build a [`FieldSet`] from the configured field definitions.
    ///
    /// # Panics
    ///
    /// Panics if the number of fields exceeds `u32::MAX`. This is
    /// unreachable in practice since `validate()` is called first.
    pub(crate) fn defined_field_set(&self) -> FieldSet {
        (0..self.fields.len())
            .map(|i| FieldId(u32::try_from(i).expect("field count validated")))
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
            propagators: vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))],
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
        cfg.propagators
            .push(Box::new(ConstPropagator::new("conflict", FieldId(0), 2.0)));
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
            fn max_dt(&self, _space: &dyn murk_space::Space) -> Option<f64> {
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
        struct EmptySpace(murk_core::SpaceInstanceId);
        impl Space for EmptySpace {
            fn ndim(&self) -> usize {
                1
            }
            fn cell_count(&self) -> usize {
                0
            }
            fn neighbours(
                &self,
                _: &murk_core::Coord,
            ) -> smallvec::SmallVec<[murk_core::Coord; 8]> {
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
            fn instance_id(&self) -> murk_core::SpaceInstanceId {
                self.0
            }
            fn topology_eq(&self, other: &dyn Space) -> bool {
                (other as &dyn std::any::Any)
                    .downcast_ref::<Self>()
                    .is_some()
            }
        }

        let mut cfg = valid_config();
        cfg.space = Box::new(EmptySpace(murk_core::SpaceInstanceId::next()));
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

    #[test]
    fn async_config_resolved_worker_count_clamps_zero() {
        let cfg = AsyncConfig {
            worker_count: Some(0),
            ..AsyncConfig::default()
        };
        assert_eq!(cfg.resolved_worker_count(), 1);
    }

    #[test]
    fn async_config_resolved_worker_count_clamps_large() {
        let cfg = AsyncConfig {
            worker_count: Some(200),
            ..AsyncConfig::default()
        };
        assert_eq!(cfg.resolved_worker_count(), 64);
    }

    #[test]
    fn async_config_resolved_worker_count_auto() {
        let cfg = AsyncConfig::default();
        let count = cfg.resolved_worker_count();
        assert!(
            (2..=16).contains(&count),
            "auto count {count} out of [2,16]"
        );
    }

    // ── BackoffConfig validation ─────────────────────────────

    #[test]
    fn validate_backoff_initial_exceeds_cap_fails() {
        let mut cfg = valid_config();
        cfg.backoff.initial_max_skew = 100;
        cfg.backoff.max_skew_cap = 5;
        match cfg.validate() {
            Err(ConfigError::InvalidBackoff { .. }) => {}
            other => panic!("expected InvalidBackoff, got {other:?}"),
        }
    }

    #[test]
    fn validate_backoff_nan_factor_fails() {
        let mut cfg = valid_config();
        cfg.backoff.backoff_factor = f64::NAN;
        match cfg.validate() {
            Err(ConfigError::InvalidBackoff { .. }) => {}
            other => panic!("expected InvalidBackoff, got {other:?}"),
        }
    }

    #[test]
    fn validate_backoff_factor_below_one_fails() {
        let mut cfg = valid_config();
        cfg.backoff.backoff_factor = 0.5;
        match cfg.validate() {
            Err(ConfigError::InvalidBackoff { .. }) => {}
            other => panic!("expected InvalidBackoff, got {other:?}"),
        }
    }

    #[test]
    fn validate_backoff_threshold_out_of_range_fails() {
        let mut cfg = valid_config();
        cfg.backoff.rejection_rate_threshold = 1.5;
        match cfg.validate() {
            Err(ConfigError::InvalidBackoff { .. }) => {}
            other => panic!("expected InvalidBackoff, got {other:?}"),
        }
    }

    #[test]
    fn validate_backoff_zero_decay_rate_fails() {
        let mut cfg = valid_config();
        cfg.backoff.decay_rate = 0;
        match cfg.validate() {
            Err(ConfigError::InvalidBackoff { .. }) => {}
            other => panic!("expected InvalidBackoff, got {other:?}"),
        }
    }

    /// BUG-103: ThreadSpawnFailed error variant exists and formats correctly.
    #[test]
    fn thread_spawn_failed_error_display() {
        let err = ConfigError::ThreadSpawnFailed {
            reason: "tick thread: resource limit".to_string(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("thread spawn failed"));
        assert!(msg.contains("tick thread"));
    }

    /// BUG-104: Subnormal tick_rate_hz passes validation but 1/hz = inf
    /// panics in Duration::from_secs_f64.
    #[test]
    fn validate_subnormal_tick_rate_hz_rejected() {
        let mut cfg = valid_config();
        cfg.tick_rate_hz = Some(f64::from_bits(1)); // smallest positive subnormal
        match cfg.validate() {
            Err(ConfigError::InvalidTickRate { .. }) => {}
            other => panic!("expected InvalidTickRate, got {other:?}"),
        }
    }

    #[test]
    fn validate_valid_backoff_succeeds() {
        let mut cfg = valid_config();
        cfg.backoff = BackoffConfig {
            initial_max_skew: 5,
            max_skew_cap: 10,
            backoff_factor: 1.5,
            decay_rate: 60,
            rejection_rate_threshold: 0.20,
        };
        assert!(cfg.validate().is_ok());
    }
}
