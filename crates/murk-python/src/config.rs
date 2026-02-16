//! PyConfig: Python wrapper around the murk config builder FFI.

use std::ffi::CString;

use pyo3::prelude::*;

use murk_ffi::{
    murk_config_add_field, murk_config_add_propagator, murk_config_create, murk_config_destroy,
    murk_config_set_dt, murk_config_set_seed, murk_config_set_space,
};

use crate::error::check_status;

/// Spatial topology type.
#[pyclass(eq, eq_int, from_py_object)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SpaceType {
    /// 1D line with configurable edge behavior.
    Line1D = 0,
    /// 1D ring (always-wrap periodic boundary).
    Ring1D = 1,
    /// 2D grid, 4-connected (N/S/E/W).
    Square4 = 2,
    /// 2D grid, 8-connected (+ diagonals).
    Square8 = 3,
    /// 2D hexagonal lattice, 6-connected (pointy-top).
    Hex2D = 4,
    /// Cartesian product of arbitrary spaces.
    ProductSpace = 5,
    /// 3D FCC lattice, 12-connected (isotropic).
    Fcc12 = 6,
}

/// Field data type.
#[pyclass(eq, eq_int, from_py_object)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldType {
    /// Single f32 per cell.
    Scalar = 0,
    /// Fixed-size f32 vector per cell.
    Vector = 1,
    /// Categorical (discrete) value per cell.
    Categorical = 2,
}

/// Field allocation strategy.
#[pyclass(eq, eq_int, from_py_object)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FieldMutability {
    /// Generation 0 forever.
    Static = 0,
    /// New allocation each tick if modified.
    PerTick = 1,
    /// New allocation only when modified.
    Sparse = 2,
}

/// Boundary behavior when field values exceed bounds.
#[pyclass(eq, eq_int, from_py_object)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BoundaryBehavior {
    /// Clamp to nearest bound.
    Clamp = 0,
    /// Reflect off the bound.
    Reflect = 1,
    /// Absorb at the boundary.
    Absorb = 2,
    /// Wrap around to opposite bound.
    Wrap = 3,
}

/// Edge behavior for lattice spaces.
#[pyclass(eq, eq_int, from_py_object)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EdgeBehavior {
    /// Absorb: cells at edge have no neighbor beyond.
    Absorb = 0,
    /// Clamp: beyond-edge neighbors map to edge cell.
    Clamp = 1,
    /// Wrap: periodic boundary.
    Wrap = 2,
}

/// Observation region type.
#[pyclass(eq, eq_int, from_py_object)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RegionType {
    /// Full grid — observe every cell.
    All = 0,
    /// Circular patch around agent center.
    AgentDisk = 5,
    /// Rectangular patch around agent center.
    AgentRect = 6,
}

#[pymethods]
impl RegionType {
    /// Integer discriminant, compatible with Python enum `.value`.
    #[getter]
    fn value(&self) -> i32 {
        *self as i32
    }
}

/// Observation transform applied at extraction time.
#[pyclass(eq, eq_int, from_py_object)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransformType {
    /// Raw field values, no transform.
    Identity = 0,
    /// Scale to [normalize_min, normalize_max] range.
    Normalize = 1,
}

#[pymethods]
impl TransformType {
    /// Integer discriminant, compatible with Python enum `.value`.
    #[getter]
    fn value(&self) -> i32 {
        *self as i32
    }
}

/// Pooling kernel for observation downsampling.
#[pyclass(eq, eq_int, from_py_object)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PoolKernel {
    /// No pooling.
    NoPool = 0,
    /// Mean pooling.
    Mean = 1,
    /// Max pooling.
    Max = 2,
    /// Min pooling.
    Min = 3,
    /// Sum pooling.
    Sum = 4,
}

#[pymethods]
impl PoolKernel {
    /// Integer discriminant, compatible with Python enum `.value`.
    #[getter]
    fn value(&self) -> i32 {
        *self as i32
    }
}

/// Observation data type.
#[pyclass(eq, eq_int, from_py_object)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DType {
    /// 32-bit float.
    F32 = 0,
}

#[pymethods]
impl DType {
    /// Integer discriminant, compatible with Python enum `.value`.
    #[getter]
    fn value(&self) -> i32 {
        *self as i32
    }
}

/// Config builder for constructing a Murk world.
///
/// Wraps the C FFI config handle. Must be consumed by `World(config)` or
/// explicitly destroyed. Supports context manager protocol.
#[pyclass]
pub(crate) struct Config {
    handle: Option<u64>,
    /// Trampoline data addresses for Python propagators. Moved to World on consumption.
    pub(crate) trampoline_data: Vec<usize>,
}

#[pymethods]
impl Config {
    #[new]
    fn new(py: Python<'_>) -> PyResult<Self> {
        // Release GIL: murk_config_create locks CONFIGS.
        let (status, h) = py.detach(|| {
            let mut h: u64 = 0;
            let s = murk_config_create(&mut h);
            (s, h)
        });
        check_status(status)?;
        Ok(Config {
            handle: Some(h),
            trampoline_data: Vec::new(),
        })
    }

    /// Set the spatial topology (low-level).
    ///
    /// Prefer the typed methods (set_space_square4, set_space_hex2d, etc.)
    /// for a self-documenting API. This method is retained for ProductSpace
    /// and advanced use cases.
    fn set_space(&self, py: Python<'_>, space_type: SpaceType, params: Vec<f64>) -> PyResult<()> {
        self._set_space_raw(py, space_type as i32, &params)
    }

    /// Set space to Line1D.
    ///
    /// Args:
    ///     length: Number of cells.
    ///     edge: Edge behavior (Absorb, Clamp, or Wrap).
    fn set_space_line1d(&self, py: Python<'_>, length: u32, edge: EdgeBehavior) -> PyResult<()> {
        let params = [length as f64, edge as i32 as f64];
        self._set_space_raw(py, SpaceType::Line1D as i32, &params)
    }

    /// Set space to Ring1D (periodic 1D).
    ///
    /// Args:
    ///     length: Number of cells.
    fn set_space_ring1d(&self, py: Python<'_>, length: u32) -> PyResult<()> {
        let params = [length as f64];
        self._set_space_raw(py, SpaceType::Ring1D as i32, &params)
    }

    /// Set space to Square4 (2D grid, 4-connected).
    ///
    /// Args:
    ///     width: Grid width.
    ///     height: Grid height.
    ///     edge: Edge behavior (Absorb, Clamp, or Wrap).
    fn set_space_square4(
        &self,
        py: Python<'_>,
        width: u32,
        height: u32,
        edge: EdgeBehavior,
    ) -> PyResult<()> {
        let params = [width as f64, height as f64, edge as i32 as f64];
        self._set_space_raw(py, SpaceType::Square4 as i32, &params)
    }

    /// Set space to Square8 (2D grid, 8-connected).
    ///
    /// Args:
    ///     width: Grid width.
    ///     height: Grid height.
    ///     edge: Edge behavior (Absorb, Clamp, or Wrap).
    fn set_space_square8(
        &self,
        py: Python<'_>,
        width: u32,
        height: u32,
        edge: EdgeBehavior,
    ) -> PyResult<()> {
        let params = [width as f64, height as f64, edge as i32 as f64];
        self._set_space_raw(py, SpaceType::Square8 as i32, &params)
    }

    /// Set space to Hex2D (hexagonal lattice, 6-connected).
    ///
    /// Args:
    ///     cols: Number of columns.
    ///     rows: Number of rows.
    fn set_space_hex2d(&self, py: Python<'_>, cols: u32, rows: u32) -> PyResult<()> {
        let params = [cols as f64, rows as f64];
        self._set_space_raw(py, SpaceType::Hex2D as i32, &params)
    }

    /// Set space to Fcc12 (3D FCC lattice, 12-connected).
    ///
    /// Args:
    ///     width: Grid width.
    ///     height: Grid height.
    ///     depth: Grid depth.
    ///     edge: Edge behavior (Absorb, Clamp, or Wrap).
    fn set_space_fcc12(
        &self,
        py: Python<'_>,
        width: u32,
        height: u32,
        depth: u32,
        edge: EdgeBehavior,
    ) -> PyResult<()> {
        let params = [
            width as f64,
            height as f64,
            depth as f64,
            edge as i32 as f64,
        ];
        self._set_space_raw(py, SpaceType::Fcc12 as i32, &params)
    }

    /// Add a field definition to the config.
    #[pyo3(signature = (name, field_type=FieldType::Scalar, mutability=FieldMutability::PerTick, dims=0, boundary=BoundaryBehavior::Clamp))]
    fn add_field(
        &self,
        py: Python<'_>,
        name: &str,
        field_type: FieldType,
        mutability: FieldMutability,
        dims: u32,
        boundary: BoundaryBehavior,
    ) -> PyResult<()> {
        let h = self.require_handle()?;
        let cname = CString::new(name)
            .map_err(|_| pyo3::exceptions::PyValueError::new_err("invalid field name"))?;
        let name_addr = cname.as_ptr() as usize;
        let ft = field_type as i32;
        let mt = mutability as i32;
        let bb = boundary as i32;
        // Release GIL: murk_config_add_field locks CONFIGS.
        let status =
            py.detach(|| murk_config_add_field(h, name_addr as *const i8, ft, mt, dims, bb));
        check_status(status)
    }

    /// Set the simulation timestep in seconds.
    fn set_dt(&self, py: Python<'_>, dt: f64) -> PyResult<()> {
        let h = self.require_handle()?;
        // Release GIL: murk_config_set_dt locks CONFIGS.
        let status = py.detach(|| murk_config_set_dt(h, dt));
        check_status(status)
    }

    /// Set the RNG seed.
    fn set_seed(&self, py: Python<'_>, seed: u64) -> PyResult<()> {
        let h = self.require_handle()?;
        // Release GIL: murk_config_set_seed locks CONFIGS.
        let status = py.detach(|| murk_config_set_seed(h, seed));
        check_status(status)
    }

    fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    #[pyo3(signature = (_exc_type=None, _exc_val=None, _exc_tb=None))]
    fn __exit__(
        &mut self,
        py: Python<'_>,
        _exc_type: Option<&Bound<'_, PyAny>>,
        _exc_val: Option<&Bound<'_, PyAny>>,
        _exc_tb: Option<&Bound<'_, PyAny>>,
    ) {
        self.destroy_with_gil(py);
    }
}

impl Config {
    pub(crate) fn require_handle(&self) -> PyResult<u64> {
        self.handle.ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("Config already consumed or destroyed")
        })
    }

    fn _set_space_raw(&self, py: Python<'_>, space_type: i32, params: &[f64]) -> PyResult<()> {
        let h = self.require_handle()?;
        let params_addr = params.as_ptr() as usize;
        let params_len = params.len();
        let status = py
            .detach(|| murk_config_set_space(h, space_type, params_addr as *const f64, params_len));
        check_status(status)
    }

    /// Add a propagator handle to the config (FFI call).
    pub(crate) fn add_propagator_handle(&self, py: Python<'_>, prop_handle: u64) -> PyResult<()> {
        let h = self.require_handle()?;
        // Release GIL: murk_config_add_propagator locks CONFIGS.
        let status = py.detach(|| murk_config_add_propagator(h, prop_handle));
        check_status(status)
    }

    /// Take ownership of the handle (consume it). Called by World::new.
    /// Also returns any trampoline data that needs to be transferred to the World.
    pub(crate) fn take_handle(&mut self) -> PyResult<(u64, Vec<usize>)> {
        let h = self.handle.take().ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("Config already consumed or destroyed")
        })?;
        let data = std::mem::take(&mut self.trampoline_data);
        Ok((h, data))
    }

    /// Free trampoline data (plain heap dealloc, no mutex).
    #[allow(unsafe_code)]
    fn free_trampolines(&mut self) {
        for addr in self.trampoline_data.drain(..) {
            if addr != 0 {
                unsafe {
                    drop(Box::from_raw(
                        addr as *mut crate::propagator::TrampolineData,
                    ));
                }
            }
        }
    }

    /// Destroy with GIL token available.
    fn destroy_with_gil(&mut self, py: Python<'_>) {
        self.free_trampolines();
        if let Some(h) = self.handle.take() {
            // Release GIL: murk_config_destroy locks CONFIGS.
            py.detach(|| murk_config_destroy(h));
        }
    }
}

impl Drop for Config {
    fn drop(&mut self) {
        self.free_trampolines();
        if let Some(h) = self.handle.take() {
            // Release GIL: murk_config_destroy locks CONFIGS.
            Python::attach(|py| {
                py.detach(|| murk_config_destroy(h));
            });
        }
    }
}
