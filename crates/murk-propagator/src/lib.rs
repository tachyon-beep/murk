//! Propagator trait and pipeline validation for Murk simulations.
//!
//! A **propagator** is a modular, stateless operator that runs once per
//! tick, reading fields from the simulation state and writing computed
//! results. Propagators declare their field dependencies at registration,
//! enabling the engine to:
//!
//! - Detect write-write conflicts.
//! - Validate field reference existence.
//! - Enforce timestep (`dt`) constraints.
//! - Precompute overlay routing via [`ReadResolutionPlan`].
//!
//! The central abstraction is the [`Propagator`] trait with its
//! `step(&self, ctx: &mut StepContext)` method. [`StepContext`] provides
//! split-borrow field access: `reads()` for the in-tick overlay view
//! (Euler-style) and `reads_previous()` for the frozen tick-start view
//! (Jacobi-style).

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]

pub mod context;
pub mod guard;
pub mod pipeline;
pub mod propagator;
pub mod scratch;

pub use context::StepContext;
pub use guard::FullWriteGuard;
pub use pipeline::{
    validate_pipeline, PipelineError, ReadResolutionPlan, ReadSource, WriteConflict,
};
pub use propagator::{Propagator, WriteMode};
pub use scratch::ScratchRegion;
