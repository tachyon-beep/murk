//! Propagator trait and step context for Murk simulations.
//!
//! The `Propagator` trait defines the `&self` step function with
//! split-borrow `StepContext` for reads/reads_previous/writes access.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]
