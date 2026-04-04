//! World configuration, validation, and error types.
//!
//! [`WorldConfig`] holds validated simulation configuration. Construct
//! it via [`WorldConfig::builder()`] → [`WorldConfigBuilder::build()`].
//! The builder runs all validation, so a `WorldConfig` value is always
//! structurally valid.
//!
//! Crate-internal code (e.g., `realtime.rs`) retains `pub(crate)` field
//! access for reconstruction patterns where the space is replaced with
//! an `Arc`-wrapped variant. See [`crate::realtime::RealtimeAsyncWorld::new()`] for details.

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
    /// Builder: `space` was not set.
    MissingSpace,
    /// Builder: `dt` was not set.
    MissingDt,
    /// tick_rate_hz is NaN, infinite, zero, or negative.
    InvalidTickRate {
        /// The invalid value.
        value: f64,
    },
    /// `initial_max_skew` exceeds `max_skew_cap`.
    BackoffSkewExceedsCap {
        /// The configured initial max skew.
        initial: u64,
        /// The configured cap.
        cap: u64,
    },
    /// `backoff_factor` is not finite or is less than 1.0.
    BackoffInvalidFactor {
        /// The invalid value.
        value: f64,
    },
    /// `rejection_rate_threshold` is outside `[0.0, 1.0]` or not finite.
    BackoffInvalidThreshold {
        /// The invalid value.
        value: f64,
    },
    /// `decay_rate` is zero.
    BackoffZeroDecayRate,
    /// Cell count exceeds `u32::MAX`.
    CellCountOverflow {
        /// The value that overflowed.
        value: usize,
    },
    /// Field count exceeds `u32::MAX`.
    FieldCountOverflow {
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
            Self::MissingSpace => {
                write!(f, "builder: space not set — call .space() before .build()")
            }
            Self::MissingDt => {
                write!(f, "builder: dt not set — call .dt() before .build()")
            }
            Self::InvalidTickRate { value } => {
                write!(f, "tick_rate_hz must be finite and positive, got {value}")
            }
            Self::BackoffSkewExceedsCap { initial, cap } => {
                write!(f, "invalid backoff config: initial_max_skew ({initial}) exceeds max_skew_cap ({cap})")
            }
            Self::BackoffInvalidFactor { value } => {
                write!(
                    f,
                    "invalid backoff config: backoff_factor must be finite and >= 1.0, got {value}"
                )
            }
            Self::BackoffInvalidThreshold { value } => {
                write!(f, "invalid backoff config: rejection_rate_threshold must be in [0.0, 1.0], got {value}")
            }
            Self::BackoffZeroDecayRate => {
                write!(f, "invalid backoff config: decay_rate must be at least 1")
            }
            Self::CellCountOverflow { value } => {
                write!(f, "cell count {value} exceeds u32::MAX")
            }
            Self::FieldCountOverflow { value } => {
                write!(f, "field count {value} exceeds u32::MAX")
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
    pub(crate) space: Box<dyn Space>,
    /// Field definitions. `FieldId(n)` corresponds to `fields[n]`.
    pub(crate) fields: Vec<FieldDef>,
    /// Propagators executed in pipeline order each tick.
    pub(crate) propagators: Vec<Box<dyn Propagator>>,
    /// Simulation timestep in seconds.
    pub(crate) dt: f64,
    /// RNG seed for deterministic simulation.
    pub(crate) seed: u64,
    /// Number of snapshots retained in the ring buffer. Default: 8. Minimum: 2.
    pub(crate) ring_buffer_size: usize,
    /// Maximum commands buffered in the ingress queue. Default: 1024.
    pub(crate) max_ingress_queue: usize,
    /// Optional target tick rate for realtime-async mode.
    pub(crate) tick_rate_hz: Option<f64>,
    /// Adaptive backoff configuration.
    pub(crate) backoff: BackoffConfig,
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
            return Err(ConfigError::FieldCountOverflow {
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
            return Err(ConfigError::BackoffSkewExceedsCap {
                initial: b.initial_max_skew,
                cap: b.max_skew_cap,
            });
        }
        if !b.backoff_factor.is_finite() || b.backoff_factor < 1.0 {
            return Err(ConfigError::BackoffInvalidFactor {
                value: b.backoff_factor,
            });
        }
        if !b.rejection_rate_threshold.is_finite()
            || b.rejection_rate_threshold < 0.0
            || b.rejection_rate_threshold > 1.0
        {
            return Err(ConfigError::BackoffInvalidThreshold {
                value: b.rejection_rate_threshold,
            });
        }
        if b.decay_rate == 0 {
            return Err(ConfigError::BackoffZeroDecayRate);
        }

        // 7. Pipeline validation (delegates to murk-propagator).
        //    The plan is intentionally discarded here — the world constructor
        //    calls validate_pipeline() again to obtain it.
        let defined = self.defined_field_set()?;
        let _ = validate_pipeline(&self.propagators, &defined, self.dt, &*self.space)?;

        Ok(())
    }

    // ── Public accessors ──────────────────────────────────────

    /// The spatial topology for the simulation.
    pub fn space(&self) -> &dyn Space {
        &*self.space
    }

    /// The field definitions. `FieldId(n)` corresponds to `fields[n]`.
    pub fn fields(&self) -> &[FieldDef] {
        &self.fields
    }

    /// The propagators executed in pipeline order each tick.
    pub fn propagators(&self) -> &[Box<dyn Propagator>] {
        &self.propagators
    }

    /// The simulation timestep in seconds.
    pub fn dt(&self) -> f64 {
        self.dt
    }

    /// The RNG seed for deterministic simulation.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Number of snapshots retained in the ring buffer.
    pub fn ring_buffer_size(&self) -> usize {
        self.ring_buffer_size
    }

    /// Maximum commands buffered in the ingress queue.
    pub fn max_ingress_queue(&self) -> usize {
        self.max_ingress_queue
    }

    /// Optional target tick rate for realtime-async mode.
    pub fn tick_rate_hz(&self) -> Option<f64> {
        self.tick_rate_hz
    }

    /// The adaptive backoff configuration.
    pub fn backoff(&self) -> &BackoffConfig {
        &self.backoff
    }

    // ── Builder ──────────────────────────────────────────────

    /// Create a new [`WorldConfigBuilder`] with sensible defaults.
    pub fn builder() -> WorldConfigBuilder {
        WorldConfigBuilder {
            space: None,
            fields: Vec::new(),
            propagators: Vec::new(),
            dt: None,
            seed: 0,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: BackoffConfig::default(),
        }
    }

    /// Build a [`FieldSet`] from the configured field definitions.
    ///
    /// Returns [`ConfigError::FieldCountOverflow`] if the number of
    /// fields exceeds `u32::MAX`.
    pub(crate) fn defined_field_set(&self) -> Result<FieldSet, ConfigError> {
        (0..self.fields.len())
            .map(|i| {
                u32::try_from(i)
                    .map(FieldId)
                    .map_err(|_| ConfigError::FieldCountOverflow {
                        value: self.fields.len(),
                    })
            })
            .collect()
    }
}

/// Fluent builder for [`WorldConfig`].
///
/// `space` and `dt` are required — calling [`build()`](WorldConfigBuilder::build)
/// without them returns [`ConfigError::MissingSpace`] or [`ConfigError::MissingDt`].
/// `fields` and `propagators` default to empty `Vec`s; the existing pipeline
/// validation in [`WorldConfig::validate()`] catches those cases with
/// [`ConfigError::NoFields`] and [`ConfigError::Pipeline`] respectively,
/// so the builder does not duplicate those checks.
pub struct WorldConfigBuilder {
    space: Option<Box<dyn Space>>,
    fields: Vec<FieldDef>,
    propagators: Vec<Box<dyn Propagator>>,
    dt: Option<f64>,
    seed: u64,
    ring_buffer_size: usize,
    max_ingress_queue: usize,
    tick_rate_hz: Option<f64>,
    backoff: BackoffConfig,
}

impl WorldConfigBuilder {
    /// Set the spatial topology. If called multiple times, the last value wins.
    pub fn space(mut self, space: Box<dyn Space>) -> Self {
        self.space = Some(space);
        self
    }

    /// Set all field definitions at once. If called multiple times, the last value wins.
    pub fn fields(mut self, fields: Vec<FieldDef>) -> Self {
        self.fields = fields;
        self
    }

    /// Append a single field definition.
    pub fn field(mut self, field: FieldDef) -> Self {
        self.fields.push(field);
        self
    }

    /// Set all propagators at once. If called multiple times, the last value wins.
    pub fn propagators(mut self, propagators: Vec<Box<dyn Propagator>>) -> Self {
        self.propagators = propagators;
        self
    }

    /// Append a single propagator to the pipeline.
    pub fn propagator(mut self, propagator: Box<dyn Propagator>) -> Self {
        self.propagators.push(propagator);
        self
    }

    /// Set the simulation timestep in seconds. If called multiple times, the last value wins.
    pub fn dt(mut self, dt: f64) -> Self {
        self.dt = Some(dt);
        self
    }

    /// Set the RNG seed. If called multiple times, the last value wins.
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Set the ring buffer size. If called multiple times, the last value wins.
    pub fn ring_buffer_size(mut self, ring_buffer_size: usize) -> Self {
        self.ring_buffer_size = ring_buffer_size;
        self
    }

    /// Set the maximum ingress queue capacity. If called multiple times, the last value wins.
    pub fn max_ingress_queue(mut self, max_ingress_queue: usize) -> Self {
        self.max_ingress_queue = max_ingress_queue;
        self
    }

    /// Set the target tick rate for realtime-async mode. If called multiple times, the last value wins.
    ///
    /// The default is `None` (no autonomous ticking / lockstep mode).
    /// Calling this method sets the rate to `Some(hz)`.
    pub fn tick_rate_hz(mut self, hz: f64) -> Self {
        self.tick_rate_hz = Some(hz);
        self
    }

    /// Set the adaptive backoff configuration. If called multiple times, the last value wins.
    pub fn backoff(mut self, backoff: BackoffConfig) -> Self {
        self.backoff = backoff;
        self
    }

    /// Consume the builder and produce a validated [`WorldConfig`].
    ///
    /// Returns [`ConfigError::MissingSpace`] if `space` was never set,
    /// [`ConfigError::MissingDt`] if `dt` was never set, or any error
    /// from [`WorldConfig::validate()`] if structural invariants fail.
    pub fn build(self) -> Result<WorldConfig, ConfigError> {
        let space = self.space.ok_or(ConfigError::MissingSpace)?;
        let dt = self.dt.ok_or(ConfigError::MissingDt)?;

        let config = WorldConfig {
            space,
            fields: self.fields,
            propagators: self.propagators,
            dt,
            seed: self.seed,
            ring_buffer_size: self.ring_buffer_size,
            max_ingress_queue: self.max_ingress_queue,
            tick_rate_hz: self.tick_rate_hz,
            backoff: self.backoff,
        };

        config.validate()?;
        Ok(config)
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
        WorldConfig::builder()
            .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
            .fields(vec![scalar_field("energy")])
            .propagators(vec![Box::new(ConstPropagator::new(
                "const",
                FieldId(0),
                1.0,
            ))])
            .dt(0.1)
            .seed(42)
            .build()
            .unwrap()
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
    fn cell_count_overflow_display_says_cell_count() {
        let err = ConfigError::CellCountOverflow {
            value: u32::MAX as usize + 1,
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("cell count"),
            "CellCountOverflow Display should say 'cell count', got: {msg}"
        );
    }

    #[test]
    fn field_count_overflow_display_says_field_count() {
        let err = ConfigError::FieldCountOverflow {
            value: u32::MAX as usize + 1,
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("field count"),
            "FieldCountOverflow Display should say 'field count', got: {msg}"
        );
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
            Err(ConfigError::BackoffSkewExceedsCap {
                initial: 100,
                cap: 5,
            }) => {}
            other => panic!("expected BackoffSkewExceedsCap, got {other:?}"),
        }
    }

    #[test]
    fn validate_backoff_nan_factor_fails() {
        let mut cfg = valid_config();
        cfg.backoff.backoff_factor = f64::NAN;
        match cfg.validate() {
            Err(ConfigError::BackoffInvalidFactor { .. }) => {}
            other => panic!("expected BackoffInvalidFactor, got {other:?}"),
        }
    }

    #[test]
    fn validate_backoff_factor_below_one_fails() {
        let mut cfg = valid_config();
        cfg.backoff.backoff_factor = 0.5;
        match cfg.validate() {
            Err(ConfigError::BackoffInvalidFactor { value: 0.5 }) => {}
            other => panic!("expected BackoffInvalidFactor(0.5), got {other:?}"),
        }
    }

    #[test]
    fn validate_backoff_threshold_out_of_range_fails() {
        let mut cfg = valid_config();
        cfg.backoff.rejection_rate_threshold = 1.5;
        match cfg.validate() {
            Err(ConfigError::BackoffInvalidThreshold { value: 1.5 }) => {}
            other => panic!("expected BackoffInvalidThreshold(1.5), got {other:?}"),
        }
    }

    #[test]
    fn validate_backoff_zero_decay_rate_fails() {
        let mut cfg = valid_config();
        cfg.backoff.decay_rate = 0;
        match cfg.validate() {
            Err(ConfigError::BackoffZeroDecayRate) => {}
            other => panic!("expected BackoffZeroDecayRate, got {other:?}"),
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

    #[test]
    fn thread_spawn_failed_error_source_is_none() {
        use std::error::Error;
        let err = ConfigError::ThreadSpawnFailed {
            reason: "egress worker 2: resource limit".into(),
        };
        assert!(err.source().is_none());
    }

    #[test]
    fn thread_spawn_failed_reason_preserved() {
        let err = ConfigError::ThreadSpawnFailed {
            reason: "egress worker 2: os error 11".into(),
        };
        match &err {
            ConfigError::ThreadSpawnFailed { reason } => {
                assert_eq!(reason, "egress worker 2: os error 11");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn thread_spawn_failed_debug_contains_reason() {
        let err = ConfigError::ThreadSpawnFailed {
            reason: "tick thread: RLIMIT_NPROC".into(),
        };
        let dbg = format!("{err:?}");
        assert!(dbg.contains("RLIMIT_NPROC"), "Debug output: {dbg}");
    }

    // ── WorldConfigBuilder tests ────────────────────────────────

    #[test]
    fn builder_missing_space_fails() {
        let result = WorldConfig::builder()
            .fields(vec![scalar_field("energy")])
            .propagators(vec![Box::new(ConstPropagator::new(
                "const",
                FieldId(0),
                1.0,
            ))])
            .dt(0.1)
            .build();
        match result {
            Err(ConfigError::MissingSpace) => {}
            other => panic!("expected MissingSpace, got {other:?}"),
        }
    }

    #[test]
    fn builder_missing_dt_fails() {
        let result = WorldConfig::builder()
            .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
            .fields(vec![scalar_field("energy")])
            .propagators(vec![Box::new(ConstPropagator::new(
                "const",
                FieldId(0),
                1.0,
            ))])
            .build();
        match result {
            Err(ConfigError::MissingDt) => {}
            other => panic!("expected MissingDt, got {other:?}"),
        }
    }

    #[test]
    fn builder_with_defaults_succeeds() {
        let config = WorldConfig::builder()
            .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
            .fields(vec![scalar_field("energy")])
            .propagators(vec![Box::new(ConstPropagator::new(
                "const",
                FieldId(0),
                1.0,
            ))])
            .dt(0.1)
            .build()
            .expect("builder with defaults should succeed");
        assert_eq!(config.seed, 0);
        assert_eq!(config.ring_buffer_size, 8);
        assert_eq!(config.max_ingress_queue, 1024);
        assert_eq!(config.tick_rate_hz, None);
    }

    #[test]
    fn builder_with_all_options_succeeds() {
        let config = WorldConfig::builder()
            .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
            .fields(vec![scalar_field("energy")])
            .propagators(vec![Box::new(ConstPropagator::new(
                "const",
                FieldId(0),
                1.0,
            ))])
            .dt(0.05)
            .seed(99)
            .ring_buffer_size(16)
            .max_ingress_queue(2048)
            .tick_rate_hz(60.0)
            .backoff(BackoffConfig {
                initial_max_skew: 3,
                backoff_factor: 2.0,
                max_skew_cap: 20,
                decay_rate: 120,
                rejection_rate_threshold: 0.10,
            })
            .build()
            .expect("builder with all options should succeed");
        assert_eq!(config.dt, 0.05);
        assert_eq!(config.seed, 99);
        assert_eq!(config.ring_buffer_size, 16);
        assert_eq!(config.max_ingress_queue, 2048);
        assert_eq!(config.tick_rate_hz, Some(60.0));
        assert_eq!(config.backoff.initial_max_skew, 3);
        assert_eq!(config.backoff.backoff_factor, 2.0);
        assert_eq!(config.backoff.max_skew_cap, 20);
        assert_eq!(config.backoff.decay_rate, 120);
        assert!((config.backoff.rejection_rate_threshold - 0.10).abs() < f64::EPSILON);
    }

    #[test]
    fn builder_validates_ring_buffer_too_small() {
        let result = WorldConfig::builder()
            .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
            .fields(vec![scalar_field("energy")])
            .propagators(vec![Box::new(ConstPropagator::new(
                "const",
                FieldId(0),
                1.0,
            ))])
            .dt(0.1)
            .ring_buffer_size(1)
            .build();
        match result {
            Err(ConfigError::RingBufferTooSmall { configured: 1 }) => {}
            other => panic!("expected RingBufferTooSmall{{configured:1}}, got {other:?}"),
        }
    }

    // ── Display coverage for remaining error variants ──────

    #[test]
    fn engine_recovery_failed_display() {
        let err = ConfigError::EngineRecoveryFailed;
        let msg = format!("{err}");
        assert!(
            msg.contains("engine"),
            "EngineRecoveryFailed Display should mention 'engine', got: {msg}"
        );
    }

    #[test]
    fn invalid_tick_rate_display() {
        let err = ConfigError::InvalidTickRate { value: -1.0 };
        let msg = format!("{err}");
        assert!(
            msg.contains("tick_rate_hz") && msg.contains("-1"),
            "InvalidTickRate Display should mention tick_rate_hz and the value, got: {msg}"
        );
    }

    #[test]
    fn ring_buffer_too_small_display() {
        let err = ConfigError::RingBufferTooSmall { configured: 1 };
        let msg = format!("{err}");
        assert!(
            msg.contains("ring_buffer_size") && msg.contains("1"),
            "RingBufferTooSmall Display should mention ring_buffer_size and the value, got: {msg}"
        );
    }

    #[test]
    fn ingress_queue_zero_display() {
        let err = ConfigError::IngressQueueZero;
        let msg = format!("{err}");
        assert!(
            msg.contains("ingress"),
            "IngressQueueZero Display should mention ingress, got: {msg}"
        );
    }

    #[test]
    fn invalid_field_display() {
        let err = ConfigError::InvalidField {
            reason: "bounds min > max".to_string(),
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("bounds min > max"),
            "InvalidField Display should include the reason, got: {msg}"
        );
    }

    #[test]
    fn backoff_skew_exceeds_cap_display() {
        let err = ConfigError::BackoffSkewExceedsCap {
            initial: 20,
            cap: 5,
        };
        let msg = format!("{err}");
        assert!(
            msg.contains("20") && msg.contains("5"),
            "BackoffSkewExceedsCap Display should include both values, got: {msg}"
        );
    }

    #[test]
    fn backoff_invalid_factor_display() {
        let err = ConfigError::BackoffInvalidFactor { value: 0.5 };
        let msg = format!("{err}");
        assert!(
            msg.contains("0.5"),
            "BackoffInvalidFactor Display should include the value, got: {msg}"
        );
    }

    #[test]
    fn backoff_invalid_threshold_display() {
        let err = ConfigError::BackoffInvalidThreshold { value: 2.0 };
        let msg = format!("{err}");
        assert!(
            msg.contains("2"),
            "BackoffInvalidThreshold Display should include the value, got: {msg}"
        );
    }

    #[test]
    fn backoff_zero_decay_rate_display() {
        let err = ConfigError::BackoffZeroDecayRate;
        let msg = format!("{err}");
        assert!(
            msg.contains("decay_rate"),
            "BackoffZeroDecayRate Display should mention decay_rate, got: {msg}"
        );
    }

    #[test]
    fn engine_recovery_failed_source_is_none() {
        use std::error::Error;
        let err = ConfigError::EngineRecoveryFailed;
        assert!(err.source().is_none());
    }

    // ── tick_rate_hz edge cases ──────────────────────────────

    #[test]
    fn validate_negative_tick_rate_hz_rejected() {
        let mut cfg = valid_config();
        cfg.tick_rate_hz = Some(-60.0);
        match cfg.validate() {
            Err(ConfigError::InvalidTickRate { .. }) => {}
            other => panic!("expected InvalidTickRate, got {other:?}"),
        }
    }

    #[test]
    fn validate_infinite_tick_rate_hz_rejected() {
        let mut cfg = valid_config();
        cfg.tick_rate_hz = Some(f64::INFINITY);
        match cfg.validate() {
            Err(ConfigError::InvalidTickRate { .. }) => {}
            other => panic!("expected InvalidTickRate, got {other:?}"),
        }
    }

    #[test]
    fn validate_nan_tick_rate_hz_rejected() {
        let mut cfg = valid_config();
        cfg.tick_rate_hz = Some(f64::NAN);
        match cfg.validate() {
            Err(ConfigError::InvalidTickRate { .. }) => {}
            other => panic!("expected InvalidTickRate, got {other:?}"),
        }
    }

    #[test]
    fn validate_zero_tick_rate_hz_rejected() {
        let mut cfg = valid_config();
        cfg.tick_rate_hz = Some(0.0);
        match cfg.validate() {
            Err(ConfigError::InvalidTickRate { .. }) => {}
            other => panic!("expected InvalidTickRate, got {other:?}"),
        }
    }

    #[test]
    fn validate_negative_backoff_threshold_rejected() {
        let mut cfg = valid_config();
        cfg.backoff.rejection_rate_threshold = -0.1;
        match cfg.validate() {
            Err(ConfigError::BackoffInvalidThreshold { .. }) => {}
            other => panic!("expected BackoffInvalidThreshold, got {other:?}"),
        }
    }

    #[test]
    fn validate_nan_backoff_threshold_rejected() {
        let mut cfg = valid_config();
        cfg.backoff.rejection_rate_threshold = f64::NAN;
        match cfg.validate() {
            Err(ConfigError::BackoffInvalidThreshold { .. }) => {}
            other => panic!("expected BackoffInvalidThreshold, got {other:?}"),
        }
    }

    // ── Builder singular methods ─────────────────────────────

    #[test]
    fn builder_singular_field_method() {
        let config = WorldConfig::builder()
            .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
            .field(scalar_field("energy"))
            .propagators(vec![Box::new(ConstPropagator::new(
                "const",
                FieldId(0),
                1.0,
            ))])
            .dt(0.1)
            .build()
            .unwrap();
        assert_eq!(config.fields().len(), 1);
        assert_eq!(config.fields()[0].name, "energy");
    }

    #[test]
    fn builder_singular_propagator_method() {
        let config = WorldConfig::builder()
            .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
            .fields(vec![scalar_field("energy")])
            .propagator(Box::new(ConstPropagator::new("const", FieldId(0), 1.0)))
            .dt(0.1)
            .build()
            .unwrap();
        assert_eq!(config.propagators().len(), 1);
    }

    // ── WorldConfig accessor coverage ────────────────────────

    #[test]
    fn worldconfig_accessors_return_configured_values() {
        let config = WorldConfig::builder()
            .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
            .fields(vec![scalar_field("energy")])
            .propagators(vec![Box::new(ConstPropagator::new(
                "const",
                FieldId(0),
                1.0,
            ))])
            .dt(0.05)
            .seed(77)
            .ring_buffer_size(4)
            .max_ingress_queue(512)
            .tick_rate_hz(30.0)
            .build()
            .unwrap();

        assert_eq!(config.space().cell_count(), 10);
        assert_eq!(config.fields().len(), 1);
        assert_eq!(config.propagators().len(), 1);
        assert_eq!(config.dt(), 0.05);
        assert_eq!(config.seed(), 77);
        assert_eq!(config.ring_buffer_size(), 4);
        assert_eq!(config.max_ingress_queue(), 512);
        assert_eq!(config.tick_rate_hz(), Some(30.0));
        assert_eq!(config.backoff().initial_max_skew, 2); // default
    }

    #[test]
    fn worldconfig_debug_does_not_panic() {
        let config = valid_config();
        let dbg = format!("{config:?}");
        assert!(dbg.contains("WorldConfig"));
    }

    #[test]
    fn builder_invalid_dt_zero_rejected() {
        let result = WorldConfig::builder()
            .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
            .fields(vec![scalar_field("energy")])
            .propagators(vec![Box::new(ConstPropagator::new(
                "const",
                FieldId(0),
                1.0,
            ))])
            .dt(0.0)
            .build();
        match result {
            Err(ConfigError::Pipeline(PipelineError::InvalidDt { .. })) => {}
            other => panic!("expected Pipeline(InvalidDt), got {other:?}"),
        }
    }

    #[test]
    fn missing_space_display() {
        let err = ConfigError::MissingSpace;
        let msg = format!("{err}");
        assert!(
            msg.contains("space"),
            "MissingSpace Display should contain 'space', got: {msg}"
        );
    }

    #[test]
    fn missing_dt_display() {
        let err = ConfigError::MissingDt;
        let msg = format!("{err}");
        assert!(
            msg.contains("dt"),
            "MissingDt Display should contain 'dt', got: {msg}"
        );
    }

    #[test]
    fn builder_space_last_value_wins() {
        let config = WorldConfig::builder()
            .space(Box::new(Line1D::new(5, EdgeBehavior::Absorb).unwrap()))
            .space(Box::new(Line1D::new(20, EdgeBehavior::Absorb).unwrap()))
            .fields(vec![scalar_field("energy")])
            .propagators(vec![Box::new(ConstPropagator::new(
                "const",
                FieldId(0),
                1.0,
            ))])
            .dt(0.1)
            .build()
            .unwrap();
        assert_eq!(config.space.cell_count(), 20);
    }

    #[test]
    fn builder_ingress_queue_zero_rejected() {
        let result = WorldConfig::builder()
            .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
            .fields(vec![scalar_field("energy")])
            .propagators(vec![Box::new(ConstPropagator::new(
                "const",
                FieldId(0),
                1.0,
            ))])
            .dt(0.1)
            .max_ingress_queue(0)
            .build();
        match result {
            Err(ConfigError::IngressQueueZero) => {}
            other => panic!("expected IngressQueueZero, got {other:?}"),
        }
    }

    #[test]
    fn builder_nan_dt_rejected() {
        let result = WorldConfig::builder()
            .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
            .fields(vec![scalar_field("energy")])
            .propagators(vec![Box::new(ConstPropagator::new(
                "const",
                FieldId(0),
                1.0,
            ))])
            .dt(f64::NAN)
            .build();
        match result {
            Err(ConfigError::Pipeline(PipelineError::InvalidDt { .. })) => {}
            other => panic!("expected Pipeline(InvalidDt), got {other:?}"),
        }
    }
}
