//! Simulation engine orchestrating Murk environments.
//!
//! Provides [`LockstepWorld`] as the primary user-facing API for synchronous
//! simulation, backed by the internal [`TickEngine`] that manages the
//! simulation loop, coordinating arenas, spaces, propagators, and
//! observation extraction.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]

pub mod config;
pub mod ingress;
pub mod lockstep;
pub mod metrics;
mod overlay;
pub mod tick;

pub use config::{BackoffConfig, ConfigError, WorldConfig};
pub use ingress::{DrainResult, DrainedCommand, IngressQueue};
pub use lockstep::{LockstepWorld, StepResult};
pub use metrics::StepMetrics;
pub use tick::{TickEngine, TickError, TickResult};
