//! Simulation engine orchestrating Murk environments.
//!
//! Provides [`LockstepWorld`] for synchronous simulation and
//! [`RealtimeAsyncWorld`] for background-threaded simulation with
//! concurrent observation extraction.
//!
//! Both modes are backed by the internal [`TickEngine`] that manages the
//! simulation loop, coordinating arenas, spaces, propagators, and
//! observation extraction.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]

pub mod config;
pub mod egress;
pub mod epoch;
pub mod ingress;
pub mod lockstep;
pub mod metrics;
mod overlay;
pub mod realtime;
pub mod ring;
pub mod tick;
pub(crate) mod tick_thread;

pub use config::{AsyncConfig, BackoffConfig, ConfigError, WorldConfig};
pub use epoch::{EpochCounter, WorkerEpoch, EPOCH_UNPINNED};
pub use ingress::{DrainResult, DrainedCommand, IngressQueue};
pub use lockstep::{LockstepWorld, StepResult};
pub use metrics::StepMetrics;
pub use realtime::{RealtimeAsyncWorld, ShutdownReport, SubmitError};
pub use ring::SnapshotRing;
pub use tick::{TickEngine, TickError, TickResult};
