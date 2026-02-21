//! Python bindings for library propagators (native Rust speed, no GIL).
//!
//! Exposes [`ScalarDiffusion`], [`GradientCompute`], [`IdentityCopy`],
//! [`FlowField`], [`AgentEmission`], [`ResourceField`], [`MorphologicalOp`],
//! [`WavePropagation`], and [`NoiseInjection`] as Python classes that
//! register **native Rust propagators** directly into the FFI config. Unlike [`PropagatorDef`](crate::propagator::PropagatorDef),
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
///     max_degree: Maximum neighbor degree for CFL stability. Default 12 (Fcc12).
///         Set to 4 for Square4, 6 for Hex2D to allow larger timesteps.
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
    max_degree: u32,
}

#[pymethods]
impl PyScalarDiffusion {
    /// Create a new ScalarDiffusion propagator.
    #[new]
    #[pyo3(signature = (input_field, output_field, coefficient=0.0, decay=0.0, sources=vec![], clamp_min=None, clamp_max=None, gradient_field=None, max_degree=12))]
    fn new(
        input_field: u32,
        output_field: u32,
        coefficient: f64,
        decay: f64,
        sources: Vec<(usize, f32)>,
        clamp_min: Option<f32>,
        clamp_max: Option<f32>,
        gradient_field: Option<u32>,
        max_degree: u32,
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
            max_degree,
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
            .sources(self.sources.clone())
            .max_degree(self.max_degree);

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
                pyo3::exceptions::PyValueError::new_err(format!("GradientCompute build error: {e}"))
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

// ── FlowField ─────────────────────────────────────────────

/// A native flow field propagator (normalized negative gradient).
///
/// Computes steepest-descent direction from a scalar potential field
/// into a 2-component vector flow field. Runs entirely in Rust.
///
/// Args:
///     potential_field: Scalar potential field ID (read from previous tick).
///     flow_field: 2-component vector field ID to write flow into.
///     normalize: Whether to normalize to unit vectors (default True).
#[pyclass(name = "FlowField")]
pub(crate) struct PyFlowField {
    potential_field: u32,
    flow_field: u32,
    normalize: bool,
}

#[pymethods]
impl PyFlowField {
    #[new]
    #[pyo3(signature = (potential_field, flow_field, normalize=true))]
    fn new(potential_field: u32, flow_field: u32, normalize: bool) -> Self {
        PyFlowField {
            potential_field,
            flow_field,
            normalize,
        }
    }

    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?;
        let prop = murk_propagators::FlowField::builder()
            .potential_field(FieldId(self.potential_field))
            .flow_field(FieldId(self.flow_field))
            .normalize(self.normalize)
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("FlowField build error: {e}"))
            })?;
        let handle = box_propagator_to_handle(Box::new(prop));
        config.add_propagator_handle(py, handle)
    }

    fn __repr__(&self) -> String {
        format!(
            "FlowField(potential_field={}, flow_field={}, normalize={})",
            self.potential_field, self.flow_field, self.normalize
        )
    }
}

// ── AgentEmission ─────────────────────────────────────────

/// A native agent emission propagator.
///
/// Emits a scalar value at each cell where an agent is present.
/// Runs entirely in Rust.
///
/// Args:
///     presence_field: Field ID encoding agent positions.
///     emission_field: Field ID to write emissions into.
///     intensity: Emission strength per agent (default 1.0).
///     additive: If True, add to previous emission; if False, set from zero (default True).
#[pyclass(name = "AgentEmission")]
pub(crate) struct PyAgentEmission {
    presence_field: u32,
    emission_field: u32,
    intensity: f32,
    additive: bool,
}

#[pymethods]
impl PyAgentEmission {
    #[new]
    #[pyo3(signature = (presence_field, emission_field, intensity=1.0, additive=true))]
    fn new(presence_field: u32, emission_field: u32, intensity: f32, additive: bool) -> Self {
        PyAgentEmission {
            presence_field,
            emission_field,
            intensity,
            additive,
        }
    }

    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?;
        let mode = if self.additive {
            murk_propagators::EmissionMode::Additive
        } else {
            murk_propagators::EmissionMode::Set
        };
        let prop = murk_propagators::AgentEmission::builder()
            .presence_field(FieldId(self.presence_field))
            .emission_field(FieldId(self.emission_field))
            .intensity(self.intensity)
            .mode(mode)
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("AgentEmission build error: {e}"))
            })?;
        let handle = box_propagator_to_handle(Box::new(prop));
        config.add_propagator_handle(py, handle)
    }

    fn __repr__(&self) -> String {
        format!(
            "AgentEmission(presence_field={}, emission_field={}, intensity={}, additive={})",
            self.presence_field, self.emission_field, self.intensity, self.additive
        )
    }
}

// ── ResourceField ─────────────────────────────────────────

/// A native consumable resource field propagator.
///
/// Resources are consumed by agent presence and regrow over time.
/// Runs entirely in Rust.
///
/// Args:
///     field: Resource field ID (read previous, write current).
///     presence_field: Agent presence field ID.
///     consumption_rate: Consumption per agent per tick (default 1.0).
///     regrowth_rate: Regrowth rate (default 0.1).
///     capacity: Carrying capacity (default 1.0).
///     logistic: If True, use logistic regrowth; if False, linear (default False).
#[pyclass(name = "ResourceField")]
pub(crate) struct PyResourceField {
    field: u32,
    presence_field: u32,
    consumption_rate: f32,
    regrowth_rate: f32,
    capacity: f32,
    logistic: bool,
}

#[pymethods]
impl PyResourceField {
    #[new]
    #[pyo3(signature = (field, presence_field, consumption_rate=1.0, regrowth_rate=0.1, capacity=1.0, logistic=false))]
    fn new(
        field: u32,
        presence_field: u32,
        consumption_rate: f32,
        regrowth_rate: f32,
        capacity: f32,
        logistic: bool,
    ) -> Self {
        PyResourceField {
            field,
            presence_field,
            consumption_rate,
            regrowth_rate,
            capacity,
            logistic,
        }
    }

    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?;
        let model = if self.logistic {
            murk_propagators::RegrowthModel::Logistic
        } else {
            murk_propagators::RegrowthModel::Linear
        };
        let prop = murk_propagators::ResourceField::builder()
            .field(FieldId(self.field))
            .presence_field(FieldId(self.presence_field))
            .consumption_rate(self.consumption_rate)
            .regrowth_rate(self.regrowth_rate)
            .capacity(self.capacity)
            .regrowth_model(model)
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("ResourceField build error: {e}"))
            })?;
        let handle = box_propagator_to_handle(Box::new(prop));
        config.add_propagator_handle(py, handle)
    }

    fn __repr__(&self) -> String {
        format!(
            "ResourceField(field={}, presence_field={}, capacity={})",
            self.field, self.presence_field, self.capacity
        )
    }
}

// ── MorphologicalOp ───────────────────────────────────────

/// A native morphological erosion/dilation propagator.
///
/// Binarizes a scalar field by threshold and applies morphological
/// erosion or dilation within a BFS radius. Runs entirely in Rust.
///
/// Args:
///     input_field: Input scalar field ID.
///     output_field: Output binary field ID.
///     dilate: If True, dilate; if False, erode (default True).
///     radius: BFS radius in hops (default 1).
///     threshold: Binarization threshold (default 0.5).
#[pyclass(name = "MorphologicalOp")]
pub(crate) struct PyMorphologicalOp {
    input_field: u32,
    output_field: u32,
    dilate: bool,
    radius: u32,
    threshold: f32,
}

#[pymethods]
impl PyMorphologicalOp {
    #[new]
    #[pyo3(signature = (input_field, output_field, dilate=true, radius=1, threshold=0.5))]
    fn new(input_field: u32, output_field: u32, dilate: bool, radius: u32, threshold: f32) -> Self {
        PyMorphologicalOp {
            input_field,
            output_field,
            dilate,
            radius,
            threshold,
        }
    }

    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?;
        let op = if self.dilate {
            murk_propagators::MorphOp::Dilate
        } else {
            murk_propagators::MorphOp::Erode
        };
        let prop = murk_propagators::MorphologicalOp::builder()
            .input_field(FieldId(self.input_field))
            .output_field(FieldId(self.output_field))
            .op(op)
            .radius(self.radius)
            .threshold(self.threshold)
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("MorphologicalOp build error: {e}"))
            })?;
        let handle = box_propagator_to_handle(Box::new(prop));
        config.add_propagator_handle(py, handle)
    }

    fn __repr__(&self) -> String {
        format!(
            "MorphologicalOp(input_field={}, output_field={}, dilate={}, radius={})",
            self.input_field, self.output_field, self.dilate, self.radius
        )
    }
}

// ── WavePropagation ───────────────────────────────────────

/// A native second-order wave equation propagator.
///
/// Models wave dynamics with propagating wavefronts, reflection, and
/// interference. Runs entirely in Rust.
///
/// Args:
///     displacement_field: Displacement scalar field ID.
///     velocity_field: Velocity scalar field ID.
///     wave_speed: Wave propagation speed (default 1.0).
///     damping: Energy damping coefficient (default 0.0).
#[pyclass(name = "WavePropagation")]
pub(crate) struct PyWavePropagation {
    displacement_field: u32,
    velocity_field: u32,
    wave_speed: f64,
    damping: f64,
}

#[pymethods]
impl PyWavePropagation {
    #[new]
    #[pyo3(signature = (displacement_field, velocity_field, wave_speed=1.0, damping=0.0))]
    fn new(displacement_field: u32, velocity_field: u32, wave_speed: f64, damping: f64) -> Self {
        PyWavePropagation {
            displacement_field,
            velocity_field,
            wave_speed,
            damping,
        }
    }

    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?;
        let prop = murk_propagators::WavePropagation::builder()
            .displacement_field(FieldId(self.displacement_field))
            .velocity_field(FieldId(self.velocity_field))
            .wave_speed(self.wave_speed)
            .damping(self.damping)
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("WavePropagation build error: {e}"))
            })?;
        let handle = box_propagator_to_handle(Box::new(prop));
        config.add_propagator_handle(py, handle)
    }

    fn __repr__(&self) -> String {
        format!(
            "WavePropagation(displacement_field={}, velocity_field={}, wave_speed={}, damping={})",
            self.displacement_field, self.velocity_field, self.wave_speed, self.damping
        )
    }
}

// ── NoiseInjection ────────────────────────────────────────

/// A native deterministic noise injection propagator.
///
/// Adds deterministic noise (Gaussian, Uniform, or SaltPepper) to a
/// field each tick. Same seed -> same noise. Runs entirely in Rust.
///
/// Args:
///     field: Field ID to inject noise into.
///     noise_type: One of "gaussian", "uniform", "salt_pepper" (default "gaussian").
///     scale: Noise scale (default 0.1).
///     seed_offset: Seed offset for deterministic RNG (default 0).
#[pyclass(name = "NoiseInjection")]
pub(crate) struct PyNoiseInjection {
    field: u32,
    noise_type: String,
    scale: f64,
    seed_offset: u64,
}

#[pymethods]
impl PyNoiseInjection {
    #[new]
    #[pyo3(signature = (field, noise_type="gaussian".to_string(), scale=0.1, seed_offset=0))]
    fn new(field: u32, noise_type: String, scale: f64, seed_offset: u64) -> Self {
        PyNoiseInjection {
            field,
            noise_type,
            scale,
            seed_offset,
        }
    }

    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?;
        let noise = match self.noise_type.as_str() {
            "gaussian" => murk_propagators::NoiseType::Gaussian,
            "uniform" => murk_propagators::NoiseType::Uniform,
            "salt_pepper" => murk_propagators::NoiseType::SaltPepper,
            other => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "unknown noise_type '{other}', expected 'gaussian', 'uniform', or 'salt_pepper'"
            )))
            }
        };
        let prop = murk_propagators::NoiseInjection::builder()
            .field(FieldId(self.field))
            .noise_type(noise)
            .scale(self.scale)
            .seed_offset(self.seed_offset)
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("NoiseInjection build error: {e}"))
            })?;
        let handle = box_propagator_to_handle(Box::new(prop));
        config.add_propagator_handle(py, handle)
    }

    fn __repr__(&self) -> String {
        format!(
            "NoiseInjection(field={}, noise_type='{}', scale={}, seed_offset={})",
            self.field, self.noise_type, self.scale, self.seed_offset
        )
    }
}
