//! Spatial data structures for Murk simulations.
//!
//! This crate defines the [`Space`] trait — the central spatial abstraction
//! through which all propagators, observations, and region queries flow —
//! along with concrete lattice backends and region planning types.
//!
//! # Backends
//!
//! - [`Line1D`]: 1D line with configurable [`EdgeBehavior`] (absorb, clamp, wrap)
//! - [`Ring1D`]: 1D ring (always-wrap periodic boundary)
//!
//! # Region Planning
//!
//! Spatial queries are expressed as [`RegionSpec`] values and compiled to
//! [`RegionPlan`] for O(1) lookups during tick execution.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]

pub mod edge;
pub mod error;
pub mod line1d;
pub mod region;
pub mod ring1d;
pub mod space;

#[cfg(test)]
pub(crate) mod compliance;

pub use edge::EdgeBehavior;
pub use error::SpaceError;
pub use line1d::Line1D;
pub use region::{BoundingShape, RegionPlan, RegionSpec};
pub use ring1d::Ring1D;
pub use space::Space;
