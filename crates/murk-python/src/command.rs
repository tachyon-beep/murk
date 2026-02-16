//! PyCommand and PyReceipt: Python wrappers for Murk commands and receipts.

use pyo3::prelude::*;

use murk_ffi::{MurkCommand, MurkCommandType, MurkReceipt};

/// Write mode for propagators.
#[pyclass(eq, eq_int, from_py_object)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WriteMode {
    Full = 0,
    Incremental = 1,
}

/// Command type discriminator.
#[pyclass(eq, eq_int, from_py_object)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandType {
    SetParameter = 0,
    SetField = 1,
}

/// A command to submit to the simulation.
#[pyclass(from_py_object)]
#[derive(Clone)]
pub(crate) struct Command {
    pub(crate) inner: MurkCommand,
}

#[pymethods]
impl Command {
    /// Create a SetField command.
    ///
    /// Args:
    ///     field_id: Target field index.
    ///     coord: Cell coordinate (list of ints, 1-4 dimensions).
    ///     value: Float value to set.
    ///     expires_after_tick: Command expires if not applied by this tick (default 0 = never).
    #[staticmethod]
    #[pyo3(signature = (field_id, coord, value, expires_after_tick=u64::MAX))]
    fn set_field(
        field_id: u32,
        coord: Vec<i32>,
        value: f32,
        expires_after_tick: u64,
    ) -> PyResult<Self> {
        if coord.is_empty() || coord.len() > 4 {
            return Err(pyo3::exceptions::PyValueError::new_err(
                "coord must have 1-4 dimensions",
            ));
        }
        let mut c = [0i32; 4];
        for (i, &v) in coord.iter().enumerate() {
            c[i] = v;
        }
        Ok(Command {
            inner: MurkCommand {
                command_type: MurkCommandType::SetField as i32,
                expires_after_tick,
                source_id: 0,
                source_seq: 0,
                priority_class: 0,
                field_id,
                param_key: 0,
                float_value: value,
                double_value: 0.0,
                coord: c,
                coord_ndim: coord.len() as u32,
            },
        })
    }

    /// Create a SetParameter command.
    #[staticmethod]
    #[pyo3(signature = (param_key, value, expires_after_tick=u64::MAX))]
    fn set_parameter(param_key: u32, value: f64, expires_after_tick: u64) -> PyResult<Self> {
        Ok(Command {
            inner: MurkCommand {
                command_type: MurkCommandType::SetParameter as i32,
                expires_after_tick,
                source_id: 0,
                source_seq: 0,
                priority_class: 0,
                field_id: 0,
                param_key,
                float_value: 0.0,
                double_value: value,
                coord: [0; 4],
                coord_ndim: 0,
            },
        })
    }
}

/// Read-only receipt returned after command processing.
#[pyclass]
pub(crate) struct Receipt {
    inner: MurkReceipt,
}

#[pymethods]
impl Receipt {
    #[getter]
    fn accepted(&self) -> bool {
        self.inner.accepted != 0
    }

    #[getter]
    fn applied_tick_id(&self) -> u64 {
        self.inner.applied_tick_id
    }

    #[getter]
    fn reason_code(&self) -> i32 {
        self.inner.reason_code
    }

    #[getter]
    fn command_index(&self) -> u32 {
        self.inner.command_index
    }

    fn __repr__(&self) -> String {
        format!(
            "Receipt(accepted={}, applied_tick={}, reason={}, index={})",
            self.accepted(),
            self.inner.applied_tick_id,
            self.inner.reason_code,
            self.inner.command_index
        )
    }
}

impl Receipt {
    pub(crate) fn from_ffi(r: MurkReceipt) -> Self {
        Receipt { inner: r }
    }
}
