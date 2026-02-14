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
//! - [`Hex2D`]: 2D hexagonal lattice, 6-connected, cube distance
//! - [`Fcc12`]: 3D face-centred cubic lattice, 12-connected, isotropic
//! - [`ProductSpace`]: Cartesian product of arbitrary spaces
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
pub mod fcc12;
pub(crate) mod grid2d;
pub mod hex2d;
pub mod line1d;
pub mod product;
pub mod region;
pub mod ring1d;
pub mod space;
pub mod square4;
pub mod square8;

#[cfg(test)]
pub(crate) mod compliance;

pub use edge::EdgeBehavior;
pub use error::SpaceError;
pub use fcc12::Fcc12;
pub use hex2d::Hex2D;
pub use line1d::Line1D;
pub use product::{ProductMetric, ProductSpace};
pub use region::{BoundingShape, RegionPlan, RegionSpec};
pub use ring1d::Ring1D;
pub use space::Space;
pub use square4::Square4;
pub use square8::Square8;
