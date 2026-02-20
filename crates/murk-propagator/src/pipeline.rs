//! Pipeline validation and read resolution planning.
//!
//! [`validate_pipeline`] runs once at engine startup to check the propagator
//! list for structural errors and build the [`ReadResolutionPlan`] — a
//! precomputed routing table that eliminates runtime conditionals in the
//! per-tick hot path.

use indexmap::IndexMap;
use murk_core::{FieldId, FieldSet};

use crate::propagator::{Propagator, WriteMode};

use std::error::Error;
use std::fmt;

// ── Read resolution ────────────────────────────────────────────────

/// Where a propagator reads a field from during tick execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadSource {
    /// Read from the base generation (tick-start snapshot).
    BaseGen,
    /// Read from the staged write buffer of a prior propagator.
    Staged {
        /// Index of the writing propagator in the pipeline.
        writer_index: usize,
    },
}

/// Precomputed routing table mapping each `(propagator, field)` to its
/// [`ReadSource`], and each written field to its [`WriteMode`].
///
/// Built once by [`validate_pipeline`]. The engine consults this plan
/// to configure each propagator's `FieldReader` before calling `step()`
/// and to seed [`WriteMode::Incremental`] buffers from the previous generation.
#[derive(Debug)]
#[must_use]
pub struct ReadResolutionPlan {
    /// `routes[propagator_index]` maps `FieldId → ReadSource`.
    routes: Vec<IndexMap<FieldId, ReadSource>>,
    /// `write_modes[propagator_index]` maps `FieldId → WriteMode`.
    write_modes: Vec<IndexMap<FieldId, WriteMode>>,
}

impl ReadResolutionPlan {
    /// Number of propagators in the plan.
    pub fn len(&self) -> usize {
        self.routes.len()
    }

    /// Whether the plan covers zero propagators.
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    /// Look up the read source for a field in a given propagator's context.
    pub fn source(&self, propagator_index: usize, field: FieldId) -> Option<ReadSource> {
        self.routes.get(propagator_index)?.get(&field).copied()
    }

    /// All `(field, source)` pairs for a propagator.
    pub fn routes_for(&self, propagator_index: usize) -> Option<&IndexMap<FieldId, ReadSource>> {
        self.routes.get(propagator_index)
    }

    /// Look up the write mode for a field in a given propagator's context.
    pub fn write_mode(&self, propagator_index: usize, field: FieldId) -> Option<WriteMode> {
        self.write_modes.get(propagator_index)?.get(&field).copied()
    }

    /// All `(field, mode)` pairs for a propagator's writes.
    pub fn write_modes_for(
        &self,
        propagator_index: usize,
    ) -> Option<&IndexMap<FieldId, WriteMode>> {
        self.write_modes.get(propagator_index)
    }

    /// Fields declared as [`WriteMode::Incremental`] for a given propagator.
    ///
    /// The engine must copy previous-generation data into the write buffer
    /// for each of these fields before calling `step()`.
    pub fn incremental_fields_for(&self, propagator_index: usize) -> Vec<FieldId> {
        match self.write_modes.get(propagator_index) {
            Some(modes) => modes
                .iter()
                .filter(|(_, &mode)| mode == WriteMode::Incremental)
                .map(|(&field_id, _)| field_id)
                .collect(),
            None => Vec::new(),
        }
    }
}

// ── Errors ─────────────────────────────────────────────────────────

/// A detected write-write conflict between two propagators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteConflict {
    /// The contested field.
    pub field_id: FieldId,
    /// Name of the first writer (earlier in pipeline order).
    pub first_writer: String,
    /// Name of the second writer (later in pipeline order).
    pub second_writer: String,
}

/// Errors from pipeline validation (startup-time, not per-tick).
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineError {
    /// No propagators registered.
    EmptyPipeline,

    /// Two or more propagators write the same field.
    WriteConflict(Vec<WriteConflict>),

    /// A propagator references a field not defined in the world.
    UndefinedField {
        /// Which propagator.
        propagator: String,
        /// The missing field.
        field_id: FieldId,
    },

    /// The configured dt exceeds a propagator's `max_dt`.
    DtTooLarge {
        /// The dt that was requested.
        configured_dt: f64,
        /// The tightest `max_dt` constraint.
        max_supported: f64,
        /// Which propagator constrains it.
        constraining_propagator: String,
    },

    /// The configured dt is not a valid timestep (NaN, infinity, zero, or negative).
    InvalidDt {
        /// The invalid dt value.
        value: f64,
    },

    /// A propagator's `max_dt()` returned a non-finite or non-positive value.
    InvalidMaxDt {
        /// Which propagator.
        propagator: String,
        /// The invalid max_dt value.
        value: f64,
    },
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPipeline => write!(f, "pipeline has no propagators"),
            Self::WriteConflict(conflicts) => {
                write!(f, "write-write conflicts: ")?;
                for (i, c) in conflicts.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(
                        f,
                        "field {:?} written by '{}' and '{}'",
                        c.field_id, c.first_writer, c.second_writer,
                    )?;
                }
                Ok(())
            }
            Self::UndefinedField {
                propagator,
                field_id,
            } => {
                write!(
                    f,
                    "propagator '{propagator}' references undefined field {field_id:?}"
                )
            }
            Self::DtTooLarge {
                configured_dt,
                max_supported,
                constraining_propagator,
            } => {
                write!(
                    f,
                    "dt {configured_dt} exceeds max_dt {max_supported} \
                     (constrained by '{constraining_propagator}')"
                )
            }
            Self::InvalidDt { value } => {
                write!(f, "dt must be finite and positive, got {value}")
            }
            Self::InvalidMaxDt { propagator, value } => {
                write!(
                    f,
                    "propagator '{propagator}' returned invalid max_dt: {value} \
                     (must be finite and positive)"
                )
            }
        }
    }
}

impl Error for PipelineError {}

// ── Validation ─────────────────────────────────────────────────────

/// Validate a propagator pipeline and build the [`ReadResolutionPlan`].
///
/// Checks performed (all at startup, not per-tick):
///
/// 1. Pipeline is non-empty.
/// 2. No write-write conflicts (two propagators writing the same field).
/// 3. All referenced field IDs exist in `defined_fields`.
/// 4. `dt <= min(max_dt)` across all propagators.
///
/// Returns the precomputed routing plan on success.
pub fn validate_pipeline(
    propagators: &[Box<dyn Propagator>],
    defined_fields: &FieldSet,
    dt: f64,
) -> Result<ReadResolutionPlan, PipelineError> {
    // 0. dt must be finite and positive
    if !dt.is_finite() || dt <= 0.0 {
        return Err(PipelineError::InvalidDt { value: dt });
    }

    // 1. Non-empty
    if propagators.is_empty() {
        return Err(PipelineError::EmptyPipeline);
    }

    // 2. Write-write conflicts
    {
        let mut last_writer: IndexMap<FieldId, usize> = IndexMap::new();
        let mut conflicts: Vec<WriteConflict> = Vec::new();

        for (i, prop) in propagators.iter().enumerate() {
            for (field_id, _mode) in prop.writes() {
                if let Some(&j) = last_writer.get(&field_id) {
                    conflicts.push(WriteConflict {
                        field_id,
                        first_writer: propagators[j].name().to_string(),
                        second_writer: prop.name().to_string(),
                    });
                }
                last_writer.insert(field_id, i);
            }
        }
        if !conflicts.is_empty() {
            return Err(PipelineError::WriteConflict(conflicts));
        }
    }

    // 3. Field reference existence
    for prop in propagators {
        for field_id in prop.reads().iter() {
            if !defined_fields.contains(field_id) {
                return Err(PipelineError::UndefinedField {
                    propagator: prop.name().to_string(),
                    field_id,
                });
            }
        }
        for field_id in prop.reads_previous().iter() {
            if !defined_fields.contains(field_id) {
                return Err(PipelineError::UndefinedField {
                    propagator: prop.name().to_string(),
                    field_id,
                });
            }
        }
        for (field_id, _) in prop.writes() {
            if !defined_fields.contains(field_id) {
                return Err(PipelineError::UndefinedField {
                    propagator: prop.name().to_string(),
                    field_id,
                });
            }
        }
    }

    // 4. dt validation
    {
        let mut min_max_dt = f64::INFINITY;
        let mut constraining = String::new();
        for prop in propagators {
            if let Some(max) = prop.max_dt() {
                if !max.is_finite() || max <= 0.0 {
                    return Err(PipelineError::InvalidMaxDt {
                        propagator: prop.name().to_string(),
                        value: max,
                    });
                }
                if max < min_max_dt {
                    min_max_dt = max;
                    constraining = prop.name().to_string();
                }
            }
        }
        if dt > min_max_dt {
            return Err(PipelineError::DtTooLarge {
                configured_dt: dt,
                max_supported: min_max_dt,
                constraining_propagator: constraining,
            });
        }
    }

    // 5. Build ReadResolutionPlan
    let mut last_writer: IndexMap<FieldId, usize> = IndexMap::new();
    let mut routes: Vec<IndexMap<FieldId, ReadSource>> = Vec::with_capacity(propagators.len());
    let mut write_modes: Vec<IndexMap<FieldId, WriteMode>> =
        Vec::with_capacity(propagators.len());

    for (i, prop) in propagators.iter().enumerate() {
        let mut prop_routes = IndexMap::new();
        let mut prop_write_modes = IndexMap::new();

        // Route reads() through overlay
        for field_id in prop.reads().iter() {
            let source = if let Some(&j) = last_writer.get(&field_id) {
                ReadSource::Staged { writer_index: j }
            } else {
                ReadSource::BaseGen
            };
            prop_routes.insert(field_id, source);
        }

        // reads_previous() always routes to BaseGen implicitly — the engine
        // provides it as a separate reader. No entry needed in the plan.

        routes.push(prop_routes);

        // Record write modes and update last_writer for subsequent propagators
        for (field_id, mode) in prop.writes() {
            prop_write_modes.insert(field_id, mode);
            last_writer.insert(field_id, i);
        }

        write_modes.push(prop_write_modes);
    }

    Ok(ReadResolutionPlan {
        routes,
        write_modes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::StepContext;
    use crate::propagator::WriteMode;
    use murk_core::{FieldId, FieldSet, PropagatorError};

    // ── Test propagators ───────────────────────────────────────

    /// Reads field A, writes field B.
    struct PropAB;
    impl Propagator for PropAB {
        fn name(&self) -> &str {
            "PropAB"
        }
        fn reads(&self) -> FieldSet {
            [FieldId(0)].into_iter().collect()
        }
        fn writes(&self) -> Vec<(FieldId, WriteMode)> {
            vec![(FieldId(1), WriteMode::Full)]
        }
        fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
            Ok(())
        }
    }

    /// Reads field B, writes field C.
    struct PropBC;
    impl Propagator for PropBC {
        fn name(&self) -> &str {
            "PropBC"
        }
        fn reads(&self) -> FieldSet {
            [FieldId(1)].into_iter().collect()
        }
        fn writes(&self) -> Vec<(FieldId, WriteMode)> {
            vec![(FieldId(2), WriteMode::Full)]
        }
        fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
            Ok(())
        }
    }

    /// Also writes field B — causes a write conflict with PropAB.
    struct PropConflict;
    impl Propagator for PropConflict {
        fn name(&self) -> &str {
            "PropConflict"
        }
        fn reads(&self) -> FieldSet {
            FieldSet::empty()
        }
        fn writes(&self) -> Vec<(FieldId, WriteMode)> {
            vec![(FieldId(1), WriteMode::Incremental)]
        }
        fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
            Ok(())
        }
    }

    /// References a field that doesn't exist.
    struct PropBadRef;
    impl Propagator for PropBadRef {
        fn name(&self) -> &str {
            "PropBadRef"
        }
        fn reads(&self) -> FieldSet {
            [FieldId(99)].into_iter().collect()
        }
        fn writes(&self) -> Vec<(FieldId, WriteMode)> {
            vec![]
        }
        fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
            Ok(())
        }
    }

    /// Has a max_dt constraint.
    struct PropDtConstrained {
        max: f64,
    }
    impl Propagator for PropDtConstrained {
        fn name(&self) -> &str {
            "PropDtConstrained"
        }
        fn reads(&self) -> FieldSet {
            FieldSet::empty()
        }
        fn writes(&self) -> Vec<(FieldId, WriteMode)> {
            vec![(FieldId(0), WriteMode::Full)]
        }
        fn max_dt(&self) -> Option<f64> {
            Some(self.max)
        }
        fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
            Ok(())
        }
    }

    /// Reads field A via reads_previous (Jacobi-style).
    struct PropJacobi;
    impl Propagator for PropJacobi {
        fn name(&self) -> &str {
            "PropJacobi"
        }
        fn reads(&self) -> FieldSet {
            FieldSet::empty()
        }
        fn reads_previous(&self) -> FieldSet {
            [FieldId(0)].into_iter().collect()
        }
        fn writes(&self) -> Vec<(FieldId, WriteMode)> {
            vec![(FieldId(1), WriteMode::Full)]
        }
        fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
            Ok(())
        }
    }

    fn fields_0_1_2() -> FieldSet {
        [FieldId(0), FieldId(1), FieldId(2)].into_iter().collect()
    }

    // ── Valid pipeline ─────────────────────────────────────────

    #[test]
    fn valid_two_stage_pipeline() {
        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropAB), Box::new(PropBC)];
        let plan = validate_pipeline(&props, &fields_0_1_2(), 0.1).unwrap();
        assert_eq!(plan.len(), 2);

        // PropAB reads field 0 → BaseGen (no prior writer)
        assert_eq!(plan.source(0, FieldId(0)), Some(ReadSource::BaseGen));

        // PropBC reads field 1 → Staged from PropAB (index 0)
        assert_eq!(
            plan.source(1, FieldId(1)),
            Some(ReadSource::Staged { writer_index: 0 })
        );
    }

    #[test]
    fn reads_previous_always_base_gen() {
        // PropJacobi reads field 0 via reads_previous. Even if a prior
        // propagator wrote field 0, reads_previous always sees BaseGen.
        // The routing plan only tracks reads() routes; reads_previous is
        // always BaseGen implicitly.
        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropJacobi)];
        let fields = [FieldId(0), FieldId(1)].into_iter().collect();
        let plan = validate_pipeline(&props, &fields, 0.1).unwrap();
        // reads_previous is not stored in the plan — it always routes to BaseGen
        assert_eq!(plan.source(0, FieldId(0)), None);
    }

    // ── Empty pipeline ─────────────────────────────────────────

    #[test]
    fn empty_pipeline_rejected() {
        let props: Vec<Box<dyn Propagator>> = vec![];
        let result = validate_pipeline(&props, &FieldSet::empty(), 0.1);
        assert!(matches!(result, Err(PipelineError::EmptyPipeline)));
    }

    // ── Write conflicts ────────────────────────────────────────

    #[test]
    fn write_conflict_detected() {
        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropAB), Box::new(PropConflict)];
        let result = validate_pipeline(&props, &fields_0_1_2(), 0.1);
        match result {
            Err(PipelineError::WriteConflict(conflicts)) => {
                assert_eq!(conflicts.len(), 1);
                assert_eq!(conflicts[0].field_id, FieldId(1));
                assert_eq!(conflicts[0].first_writer, "PropAB");
                assert_eq!(conflicts[0].second_writer, "PropConflict");
            }
            other => panic!("expected WriteConflict, got {other:?}"),
        }
    }

    // ── Undefined field ────────────────────────────────────────

    #[test]
    fn undefined_read_field_rejected() {
        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropBadRef)];
        let result = validate_pipeline(&props, &FieldSet::empty(), 0.1);
        match result {
            Err(PipelineError::UndefinedField {
                propagator,
                field_id,
            }) => {
                assert_eq!(propagator, "PropBadRef");
                assert_eq!(field_id, FieldId(99));
            }
            other => panic!("expected UndefinedField, got {other:?}"),
        }
    }

    #[test]
    fn undefined_write_field_rejected() {
        // PropAB writes field 1 — but we only define field 0
        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropAB)];
        let fields = [FieldId(0)].into_iter().collect();
        let result = validate_pipeline(&props, &fields, 0.1);
        assert!(matches!(result, Err(PipelineError::UndefinedField { .. })));
    }

    #[test]
    fn undefined_reads_previous_field_rejected() {
        // PropJacobi reads_previous field 0 — define only field 1
        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropJacobi)];
        let fields = [FieldId(1)].into_iter().collect();
        let result = validate_pipeline(&props, &fields, 0.1);
        assert!(matches!(result, Err(PipelineError::UndefinedField { .. })));
    }

    // ── dt validation ──────────────────────────────────────────

    #[test]
    fn dt_within_bound_accepted() {
        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropDtConstrained { max: 0.5 })];
        let fields = [FieldId(0)].into_iter().collect();
        assert!(validate_pipeline(&props, &fields, 0.5).is_ok());
        assert!(validate_pipeline(&props, &fields, 0.1).is_ok());
    }

    #[test]
    fn dt_exceeds_max_dt_rejected() {
        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropDtConstrained { max: 0.5 })];
        let fields = [FieldId(0)].into_iter().collect();
        let result = validate_pipeline(&props, &fields, 1.0);
        match result {
            Err(PipelineError::DtTooLarge {
                configured_dt,
                max_supported,
                constraining_propagator,
            }) => {
                assert_eq!(configured_dt, 1.0);
                assert_eq!(max_supported, 0.5);
                assert_eq!(constraining_propagator, "PropDtConstrained");
            }
            other => panic!("expected DtTooLarge, got {other:?}"),
        }
    }

    #[test]
    fn dt_constrained_by_tightest() {
        // Two propagators: max_dt 0.5 and 0.2. dt=0.3 should fail.
        struct PropDt05;
        impl Propagator for PropDt05 {
            fn name(&self) -> &str {
                "PropDt05"
            }
            fn reads(&self) -> FieldSet {
                FieldSet::empty()
            }
            fn writes(&self) -> Vec<(FieldId, WriteMode)> {
                vec![(FieldId(0), WriteMode::Full)]
            }
            fn max_dt(&self) -> Option<f64> {
                Some(0.5)
            }
            fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
                Ok(())
            }
        }
        struct PropDt02;
        impl Propagator for PropDt02 {
            fn name(&self) -> &str {
                "PropDt02"
            }
            fn reads(&self) -> FieldSet {
                FieldSet::empty()
            }
            fn writes(&self) -> Vec<(FieldId, WriteMode)> {
                vec![(FieldId(1), WriteMode::Full)]
            }
            fn max_dt(&self) -> Option<f64> {
                Some(0.2)
            }
            fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
                Ok(())
            }
        }

        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropDt05), Box::new(PropDt02)];
        let fields = [FieldId(0), FieldId(1)].into_iter().collect();
        let result = validate_pipeline(&props, &fields, 0.3);
        match result {
            Err(PipelineError::DtTooLarge {
                constraining_propagator,
                ..
            }) => {
                assert_eq!(constraining_propagator, "PropDt02");
            }
            other => panic!("expected DtTooLarge, got {other:?}"),
        }
    }

    // ── Resolution plan routing ────────────────────────────────

    #[test]
    fn three_stage_overlay_routing() {
        // A → writes field 1
        // B → reads field 1 (overlay → Staged{0}), writes field 2
        // C → reads field 1 (overlay → Staged{0}), reads field 2 (overlay → Staged{1})
        struct PropA;
        impl Propagator for PropA {
            fn name(&self) -> &str {
                "A"
            }
            fn reads(&self) -> FieldSet {
                FieldSet::empty()
            }
            fn writes(&self) -> Vec<(FieldId, WriteMode)> {
                vec![(FieldId(1), WriteMode::Full)]
            }
            fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
                Ok(())
            }
        }
        struct PropB;
        impl Propagator for PropB {
            fn name(&self) -> &str {
                "B"
            }
            fn reads(&self) -> FieldSet {
                [FieldId(1)].into_iter().collect()
            }
            fn writes(&self) -> Vec<(FieldId, WriteMode)> {
                vec![(FieldId(2), WriteMode::Full)]
            }
            fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
                Ok(())
            }
        }
        struct PropC;
        impl Propagator for PropC {
            fn name(&self) -> &str {
                "C"
            }
            fn reads(&self) -> FieldSet {
                [FieldId(1), FieldId(2)].into_iter().collect()
            }
            fn writes(&self) -> Vec<(FieldId, WriteMode)> {
                vec![]
            }
            fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
                Ok(())
            }
        }

        let props: Vec<Box<dyn Propagator>> =
            vec![Box::new(PropA), Box::new(PropB), Box::new(PropC)];
        let fields = [FieldId(0), FieldId(1), FieldId(2)].into_iter().collect();
        let plan = validate_pipeline(&props, &fields, 0.1).unwrap();

        // B reads field 1 → Staged from A (index 0)
        assert_eq!(
            plan.source(1, FieldId(1)),
            Some(ReadSource::Staged { writer_index: 0 })
        );
        // C reads field 1 → Staged from A (index 0)
        assert_eq!(
            plan.source(2, FieldId(1)),
            Some(ReadSource::Staged { writer_index: 0 })
        );
        // C reads field 2 → Staged from B (index 1)
        assert_eq!(
            plan.source(2, FieldId(2)),
            Some(ReadSource::Staged { writer_index: 1 })
        );
    }

    #[test]
    fn unread_field_not_in_plan() {
        // PropAB reads field 0, writes field 1. Field 2 is never read by PropAB.
        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropAB)];
        let plan = validate_pipeline(&props, &fields_0_1_2(), 0.1).unwrap();
        // Field 2 not in PropAB's routes
        assert_eq!(plan.source(0, FieldId(2)), None);
    }

    // ── Write mode metadata ─────────────────────────────────────

    #[test]
    fn write_mode_full_recorded_in_plan() {
        // PropAB writes field 1 as Full
        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropAB)];
        let plan = validate_pipeline(&props, &fields_0_1_2(), 0.1).unwrap();

        assert_eq!(plan.write_mode(0, FieldId(1)), Some(WriteMode::Full));
        // Field 0 is read, not written — no write mode
        assert_eq!(plan.write_mode(0, FieldId(0)), None);
        // No incremental fields for PropAB
        assert!(plan.incremental_fields_for(0).is_empty());
    }

    #[test]
    fn write_mode_incremental_recorded_in_plan() {
        // A propagator that writes field 1 as Incremental
        struct PropIncremental;
        impl Propagator for PropIncremental {
            fn name(&self) -> &str {
                "PropIncremental"
            }
            fn reads(&self) -> FieldSet {
                FieldSet::empty()
            }
            fn writes(&self) -> Vec<(FieldId, WriteMode)> {
                vec![(FieldId(1), WriteMode::Incremental)]
            }
            fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
                Ok(())
            }
        }

        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropIncremental)];
        let plan = validate_pipeline(&props, &fields_0_1_2(), 0.1).unwrap();

        assert_eq!(
            plan.write_mode(0, FieldId(1)),
            Some(WriteMode::Incremental)
        );
        assert_eq!(plan.incremental_fields_for(0), vec![FieldId(1)]);
    }

    #[test]
    fn mixed_write_modes_in_multi_stage_pipeline() {
        // PropAB writes field 1 as Full; add another propagator writing
        // field 2 as Incremental
        struct PropIncrC;
        impl Propagator for PropIncrC {
            fn name(&self) -> &str {
                "PropIncrC"
            }
            fn reads(&self) -> FieldSet {
                FieldSet::empty()
            }
            fn writes(&self) -> Vec<(FieldId, WriteMode)> {
                vec![(FieldId(2), WriteMode::Incremental)]
            }
            fn step(&self, _ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
                Ok(())
            }
        }

        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropAB), Box::new(PropIncrC)];
        let plan = validate_pipeline(&props, &fields_0_1_2(), 0.1).unwrap();

        // PropAB (index 0): Full write on field 1
        assert_eq!(plan.write_mode(0, FieldId(1)), Some(WriteMode::Full));
        assert!(plan.incremental_fields_for(0).is_empty());

        // PropIncrC (index 1): Incremental write on field 2
        assert_eq!(
            plan.write_mode(1, FieldId(2)),
            Some(WriteMode::Incremental)
        );
        assert_eq!(plan.incremental_fields_for(1), vec![FieldId(2)]);
    }

    // ── Invalid dt ─────────────────────────────────────────────

    #[test]
    fn nan_dt_rejected() {
        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropAB)];
        let result = validate_pipeline(&props, &fields_0_1_2(), f64::NAN);
        assert!(matches!(result, Err(PipelineError::InvalidDt { .. })));
    }

    #[test]
    fn inf_dt_rejected() {
        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropAB)];
        let result = validate_pipeline(&props, &fields_0_1_2(), f64::INFINITY);
        assert!(matches!(result, Err(PipelineError::InvalidDt { .. })));
    }

    #[test]
    fn neg_inf_dt_rejected() {
        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropAB)];
        let result = validate_pipeline(&props, &fields_0_1_2(), f64::NEG_INFINITY);
        assert!(matches!(result, Err(PipelineError::InvalidDt { .. })));
    }

    #[test]
    fn zero_dt_rejected() {
        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropAB)];
        let result = validate_pipeline(&props, &fields_0_1_2(), 0.0);
        assert!(matches!(result, Err(PipelineError::InvalidDt { .. })));
    }

    #[test]
    fn negative_dt_rejected() {
        let props: Vec<Box<dyn Propagator>> = vec![Box::new(PropAB)];
        let result = validate_pipeline(&props, &fields_0_1_2(), -0.1);
        assert!(matches!(result, Err(PipelineError::InvalidDt { .. })));
    }

    // ── Invalid max_dt from propagator ────────────────────────

    #[test]
    fn nan_max_dt_rejected() {
        let props: Vec<Box<dyn Propagator>> =
            vec![Box::new(PropDtConstrained { max: f64::NAN })];
        let fields = [FieldId(0)].into_iter().collect();
        let result = validate_pipeline(&props, &fields, 0.1);
        match result {
            Err(PipelineError::InvalidMaxDt { propagator, value }) => {
                assert_eq!(propagator, "PropDtConstrained");
                assert!(value.is_nan());
            }
            other => panic!("expected InvalidMaxDt, got {other:?}"),
        }
    }

    #[test]
    fn inf_max_dt_rejected() {
        let props: Vec<Box<dyn Propagator>> =
            vec![Box::new(PropDtConstrained { max: f64::INFINITY })];
        let fields = [FieldId(0)].into_iter().collect();
        let result = validate_pipeline(&props, &fields, 0.1);
        match result {
            Err(PipelineError::InvalidMaxDt { propagator, .. }) => {
                assert_eq!(propagator, "PropDtConstrained");
            }
            other => panic!("expected InvalidMaxDt, got {other:?}"),
        }
    }

    #[test]
    fn neg_inf_max_dt_rejected() {
        let props: Vec<Box<dyn Propagator>> =
            vec![Box::new(PropDtConstrained { max: f64::NEG_INFINITY })];
        let fields = [FieldId(0)].into_iter().collect();
        let result = validate_pipeline(&props, &fields, 0.1);
        assert!(matches!(
            result,
            Err(PipelineError::InvalidMaxDt { .. })
        ));
    }

    #[test]
    fn zero_max_dt_rejected() {
        let props: Vec<Box<dyn Propagator>> =
            vec![Box::new(PropDtConstrained { max: 0.0 })];
        let fields = [FieldId(0)].into_iter().collect();
        let result = validate_pipeline(&props, &fields, 0.1);
        assert!(matches!(
            result,
            Err(PipelineError::InvalidMaxDt { .. })
        ));
    }

    #[test]
    fn negative_max_dt_rejected() {
        let props: Vec<Box<dyn Propagator>> =
            vec![Box::new(PropDtConstrained { max: -1.0 })];
        let fields = [FieldId(0)].into_iter().collect();
        let result = validate_pipeline(&props, &fields, 0.1);
        assert!(matches!(
            result,
            Err(PipelineError::InvalidMaxDt { .. })
        ));
    }
}
