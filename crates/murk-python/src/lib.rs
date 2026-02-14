//! Python bindings for the Murk simulation framework.
//!
//! This crate provides PyO3 bindings wrapping the C FFI layer (`murk-ffi`).
//! The native extension is named `_murk` and is imported by the pure-Python
//! `murk` package which adds Gymnasium adapters.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![allow(unsafe_code)]

use pyo3::prelude::*;

mod command;
mod config;
mod error;
mod metrics;
mod obs;
pub(crate) mod propagator;
mod world;

/// The native `_murk` extension module.
#[pymodule]
fn _murk(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Enums
    m.add_class::<config::SpaceType>()?;
    m.add_class::<config::FieldType>()?;
    m.add_class::<config::FieldMutability>()?;
    m.add_class::<config::BoundaryBehavior>()?;
    m.add_class::<config::EdgeBehavior>()?;
    m.add_class::<command::WriteMode>()?;
    m.add_class::<command::CommandType>()?;
    m.add_class::<config::RegionType>()?;
    m.add_class::<config::TransformType>()?;
    m.add_class::<config::PoolKernel>()?;
    m.add_class::<config::DType>()?;

    // Core classes
    m.add_class::<config::Config>()?;
    m.add_class::<command::Command>()?;
    m.add_class::<command::Receipt>()?;
    m.add_class::<world::World>()?;
    m.add_class::<obs::ObsEntry>()?;
    m.add_class::<obs::ObsPlan>()?;
    m.add_class::<metrics::StepMetrics>()?;
    m.add_class::<propagator::PropagatorDef>()?;

    // Functions
    m.add_function(wrap_pyfunction!(propagator::add_propagator, m)?)?;

    Ok(())
}
