//! Simulation engine orchestrating Murk environments.
//!
//! Provides the top-level `Engine` that manages the simulation loop,
//! coordinating arenas, spaces, propagators, and observation extraction.
//! Supports both lockstep (callable struct) and realtime-async modes.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]

pub mod config;
pub mod ingress;
pub mod metrics;

pub use config::{BackoffConfig, ConfigError, WorldConfig};
pub use ingress::{DrainResult, IngressQueue};
pub use metrics::StepMetrics;
