//! The [`Propagator`] trait and [`WriteMode`] enum.
//!
//! Propagators are modular, stateless operators executed in sequence each
//! tick. They declare field dependencies at registration, enabling the
//! engine to validate the pipeline and precompute overlay routing.

use crate::context::StepContext;
use murk_core::{FieldId, FieldSet, PropagatorError};

/// Write initialization strategy for a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteMode {
    /// Fresh buffer. Propagator MUST fill every cell.
    ///
    /// The engine allocates a new buffer; in debug builds a
    /// [`FullWriteGuard`](crate::FullWriteGuard) tracks coverage.
    Full,

    /// Buffer seeded from the previous generation via memcpy.
    /// Propagator modifies only the cells it needs to update.
    ///
    /// Used for sparse incremental updates (e.g., agent movement).
    Incremental,
}

/// A modular, stateless operator in the TickEngine's per-tick pipeline.
///
/// # Contract
///
/// - `step()` MUST be deterministic: same inputs produce identical outputs.
/// - `&self` — propagators are stateless; mutable state goes through fields.
/// - `reads()` and `writes()` are called once at startup, not per-tick.
///
/// # Object safety
///
/// This trait is object-safe; the engine stores propagators as
/// `Vec<Box<dyn Propagator>>`.
///
/// # Examples
///
/// A minimal propagator that fills a field with a constant value:
///
/// ```
/// use murk_propagator::{Propagator, StepContext, WriteMode};
/// use murk_core::{FieldId, FieldSet, PropagatorError};
///
/// struct ConstantFill {
///     field: FieldId,
///     value: f32,
/// }
///
/// impl Propagator for ConstantFill {
///     fn name(&self) -> &str { "constant_fill" }
///
///     fn reads(&self) -> FieldSet { FieldSet::empty() }
///
///     fn writes(&self) -> Vec<(FieldId, WriteMode)> {
///         vec![(self.field, WriteMode::Full)]
///     }
///
///     fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
///         let buf = ctx.writes().write(self.field).unwrap();
///         buf.fill(self.value);
///         Ok(())
///     }
/// }
///
/// let prop = ConstantFill { field: FieldId(0), value: 42.0 };
/// assert_eq!(prop.name(), "constant_fill");
/// ```
pub trait Propagator: Send + 'static {
    /// Human-readable name for error reporting and telemetry.
    fn name(&self) -> &str;

    /// Fields this propagator reads via the in-tick overlay view.
    ///
    /// Reading through `ctx.reads()` sees values from prior propagators
    /// in the current tick (Euler-style sequential integration).
    fn reads(&self) -> FieldSet;

    /// Fields this propagator reads from the frozen tick-start view.
    ///
    /// Reading through `ctx.reads_previous()` always sees the base
    /// generation regardless of prior writes (Jacobi-style).
    ///
    /// Default: empty set.
    fn reads_previous(&self) -> FieldSet {
        FieldSet::empty()
    }

    /// Fields this propagator writes, with their initialization mode.
    ///
    /// Called once at pipeline construction, not per-tick.
    fn writes(&self) -> Vec<(FieldId, WriteMode)>;

    /// Maximum stable timestep for this propagator (e.g., CFL).
    ///
    /// The pipeline validates `dt <= min(max_dt)` across all propagators.
    /// Return `None` to impose no constraint.
    fn max_dt(&self) -> Option<f64> {
        None
    }

    /// Scratch memory required **in bytes** (not f32 slots).
    ///
    /// The engine allocates `max(scratch_bytes())` across all propagators
    /// and converts to f32 slots via [`ScratchRegion::with_byte_capacity()`].
    /// The bump pointer is reset between each `step()` call.
    ///
    /// **Important:** Do not pass the return value directly to
    /// [`ScratchRegion::new()`], which expects f32 slot counts.
    /// Use [`ScratchRegion::with_byte_capacity()`] instead.
    fn scratch_bytes(&self) -> usize {
        0
    }

    /// Execute the propagator for one tick.
    ///
    /// Called once per tick in dependency order. The [`StepContext`]
    /// provides read access (overlay and frozen views), write access
    /// to declared fields, scratch memory, and the spatial topology.
    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError>;
}
