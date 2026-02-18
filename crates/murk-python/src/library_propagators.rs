//! Python bindings for library propagators (native Rust speed, no GIL).
//!
//! Exposes [`ScalarDiffusion`], [`GradientCompute`], and [`IdentityCopy`]
//! as Python classes that register **native Rust propagators** directly
//! into the FFI config. Unlike [`PropagatorDef`](crate::propagator::PropagatorDef),
//! these never touch the GIL at runtime: no Python trampoline, no numpy
//! copies, no function pointers.
//!
//! # How it works
//!
//! 1. Python creates a wrapper (e.g., `ScalarDiffusion(...)`) storing params.
//! 2. `register(config)` builds the Rust propagator via its builder API.
//! 3. The propagator is double-boxed (`Box<Box<dyn Propagator>>`) into a
//!    `u64` handle, matching the protocol of `murk_config_add_propagator`.
//! 4. The handle is passed to `Config.add_propagator_handle()`.
//!
//! The engine then runs the propagator's `step()` method directly in Rust.

use pyo3::prelude::*;

use murk_core::FieldId;
use murk_propagator::Propagator;

use crate::config::Config;

/// Double-box a `Box<dyn Propagator>` into a `u64` handle.
///
/// This matches the encoding used by `murk_propagator_create` / consumed
/// by `murk_config_add_propagator`: a thin pointer to a heap-allocated
/// fat pointer (`Box<dyn Propagator>`).
#[allow(unsafe_code)]
fn box_propagator_to_handle(prop: Box<dyn Propagator>) -> u64 {
    Box::into_raw(Box::new(prop)) as u64
}

// ── ScalarDiffusion ─────────────────────────────────────────

/// A native Jacobi-style scalar diffusion propagator.
///
/// Runs entirely in Rust at native speed. No GIL, no numpy, no Python
/// trampoline overhead.
///
/// Args:
///     input_field: Field ID to read from the previous tick.
///     output_field: Field ID to write diffused values into.
///     coefficient: Diffusion coefficient (>= 0). Default 0.0.
///     decay: Exponential decay rate per tick (>= 0). Default 0.0.
///     sources: List of (cell_index, value) fixed-source tuples. Default [].
///     clamp_min: Minimum clamp value. Default None.
///     clamp_max: Maximum clamp value. Default None.
///     gradient_field: Optional field ID for 2-component gradient output. Default None.
#[pyclass(name = "ScalarDiffusion")]
pub(crate) struct PyScalarDiffusion {
    input_field: u32,
    output_field: u32,
    coefficient: f64,
    decay: f64,
    sources: Vec<(usize, f32)>,
    clamp_min: Option<f32>,
    clamp_max: Option<f32>,
    gradient_field: Option<u32>,
}

#[pymethods]
impl PyScalarDiffusion {
    /// Create a new ScalarDiffusion propagator.
    #[new]
    #[pyo3(signature = (input_field, output_field, coefficient=0.0, decay=0.0, sources=vec![], clamp_min=None, clamp_max=None, gradient_field=None))]
    fn new(
        input_field: u32,
        output_field: u32,
        coefficient: f64,
        decay: f64,
        sources: Vec<(usize, f32)>,
        clamp_min: Option<f32>,
        clamp_max: Option<f32>,
        gradient_field: Option<u32>,
    ) -> Self {
        PyScalarDiffusion {
            input_field,
            output_field,
            coefficient,
            decay,
            sources,
            clamp_min,
            clamp_max,
            gradient_field,
        }
    }

    /// Register this propagator with a Config.
    ///
    /// Builds the native Rust propagator and inserts it into the config's
    /// propagator list. The propagator will run at native speed with no
    /// Python overhead.
    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?;

        let mut builder = murk_propagators::ScalarDiffusion::builder()
            .input_field(FieldId(self.input_field))
            .output_field(FieldId(self.output_field))
            .coefficient(self.coefficient)
            .decay(self.decay)
            .sources(self.sources.clone());

        if let Some(lo) = self.clamp_min {
            builder = builder.clamp_min(lo);
        }
        if let Some(hi) = self.clamp_max {
            builder = builder.clamp_max(hi);
        }
        if let Some(gf) = self.gradient_field {
            builder = builder.gradient_field(FieldId(gf));
        }

        let prop = builder.build().map_err(|e| {
            pyo3::exceptions::PyValueError::new_err(format!("ScalarDiffusion build error: {e}"))
        })?;

        let handle = box_propagator_to_handle(Box::new(prop));
        config.add_propagator_handle(py, handle)
    }

    fn __repr__(&self) -> String {
        format!(
            "ScalarDiffusion(input_field={}, output_field={}, coefficient={}, decay={})",
            self.input_field, self.output_field, self.coefficient, self.decay
        )
    }
}

// ── GradientCompute ─────────────────────────────────────────

/// A native finite-difference gradient propagator.
///
/// Computes central-difference gradient of a scalar field into a
/// 2-component vector field. Runs entirely in Rust.
///
/// Args:
///     input_field: Scalar field ID to compute gradient of (read from previous tick).
///     output_field: 2-component vector field ID to write gradient into.
#[pyclass(name = "GradientCompute")]
pub(crate) struct PyGradientCompute {
    input_field: u32,
    output_field: u32,
}

#[pymethods]
impl PyGradientCompute {
    /// Create a new GradientCompute propagator.
    #[new]
    fn new(input_field: u32, output_field: u32) -> Self {
        PyGradientCompute {
            input_field,
            output_field,
        }
    }

    /// Register this propagator with a Config.
    ///
    /// Builds the native Rust propagator and inserts it into the config's
    /// propagator list.
    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?;

        let prop = murk_propagators::GradientCompute::builder()
            .input_field(FieldId(self.input_field))
            .output_field(FieldId(self.output_field))
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "GradientCompute build error: {e}"
                ))
            })?;

        let handle = box_propagator_to_handle(Box::new(prop));
        config.add_propagator_handle(py, handle)
    }

    fn __repr__(&self) -> String {
        format!(
            "GradientCompute(input_field={}, output_field={})",
            self.input_field, self.output_field
        )
    }
}

// ── IdentityCopy ────────────────────────────────────────────

/// A native identity-copy propagator.
///
/// Copies a field verbatim from the previous tick to the current tick.
/// Used to carry forward state that no other propagator writes.
/// Runs entirely in Rust.
///
/// Args:
///     field: Field ID to copy.
#[pyclass(name = "IdentityCopy")]
pub(crate) struct PyIdentityCopy {
    field: u32,
}

#[pymethods]
impl PyIdentityCopy {
    /// Create a new IdentityCopy propagator.
    #[new]
    fn new(field: u32) -> Self {
        PyIdentityCopy { field }
    }

    /// Register this propagator with a Config.
    ///
    /// Builds the native Rust propagator and inserts it into the config's
    /// propagator list.
    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?;

        let prop = murk_propagators::IdentityCopy::new(FieldId(self.field));

        let handle = box_propagator_to_handle(Box::new(prop));
        config.add_propagator_handle(py, handle)
    }

    fn __repr__(&self) -> String {
        format!("IdentityCopy(field={})", self.field)
    }
}
