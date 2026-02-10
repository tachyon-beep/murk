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
//! - [`Square4`]: 2D grid, 4-connected (N/S/E/W), Manhattan distance
//! - [`Square8`]: 2D grid, 8-connected (+ diagonals), Chebyshev distance
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
pub(crate) mod grid2d;
pub mod line1d;
pub mod region;
pub mod ring1d;
pub mod space;
pub mod square4;
pub mod square8;

#[cfg(test)]
pub(crate) mod compliance;

pub use edge::EdgeBehavior;
pub use error::SpaceError;
pub use line1d::Line1D;
pub use region::{BoundingShape, RegionPlan, RegionSpec};
pub use ring1d::Ring1D;
pub use space::Space;
pub use square4::Square4;
pub use square8::Square8;
