//! Observation specification and extraction for Murk simulations.
//!
//! Defines the observation spec ([`ObsSpec`]) that describes how to
//! extract flat observation tensors from simulation state for
//! reinforcement learning agents, and the compiled [`ObsPlan`] that
//! executes the extraction against any [`SnapshotAccess`](murk_core::SnapshotAccess)
//! implementor.
//!
//! # Architecture
//!
//! ```text
//! ObsSpec ──compile()──► ObsPlan ──execute()──► &mut [f32] + ObsMetadata
//!    ↓                     ↓
//!  entries             gather_ops     (pre-computed field indices)
//!  regions             transforms     (applied at gather time)
//! ```
//!
//! The observation pipeline is decoupled from the arena (Decision N):
//! `ObsPlan` reads through `&dyn SnapshotAccess`, not `ReadArena` directly.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]

pub mod metadata;
pub mod plan;
pub mod spec;

pub use metadata::ObsMetadata;
pub use plan::{ObsPlan, ObsPlanResult};
pub use spec::{ObsDtype, ObsEntry, ObsSpec, ObsTransform};
