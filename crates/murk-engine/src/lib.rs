//! Simulation engine orchestrating Murk environments.
//!
//! Provides the top-level [`TickEngine`] that manages the simulation loop,
//! coordinating arenas, spaces, propagators, and observation extraction.
//! Supports both lockstep (callable struct) and realtime-async modes.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]

pub mod config;
pub mod ingress;
pub mod metrics;
mod overlay;
pub mod tick;

pub use config::{BackoffConfig, ConfigError, WorldConfig};
pub use ingress::{DrainResult, DrainedCommand, IngressQueue};
pub use metrics::StepMetrics;
pub use tick::{TickEngine, TickError, TickResult};
