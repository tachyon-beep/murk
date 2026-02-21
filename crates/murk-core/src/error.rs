//! Error types for the Murk simulation framework.
//!
//! Maps the error code table from HLD §9.7 to Rust enums, organized
//! by subsystem: step (tick engine), propagator, ingress, and observation.

use std::error::Error;
use std::fmt;

/// Errors from the tick engine during `step()`.
///
/// Corresponds to the TickEngine and Pipeline subsystem codes in HLD §9.7.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepError {
    /// A propagator returned an error during execution
    /// (`MURK_ERROR_PROPAGATOR_FAILED`).
    PropagatorFailed {
        /// Name of the failing propagator.
        name: String,
        /// The underlying propagator error.
        reason: PropagatorError,
    },
    /// Arena allocation failed — OOM during generation staging
    /// (`MURK_ERROR_ALLOCATION_FAILED`).
    AllocationFailed,
    /// The tick was rolled back due to a propagator failure
    /// (`MURK_ERROR_TICK_ROLLBACK`).
    TickRollback,
    /// Ticking is disabled after consecutive rollbacks
    /// (`MURK_ERROR_TICK_DISABLED`, Decision J).
    TickDisabled,
    /// The requested dt exceeds a propagator's `max_dt` constraint
    /// (`MURK_ERROR_DT_OUT_OF_RANGE`).
    DtOutOfRange,
    /// The world is shutting down
    /// (`MURK_ERROR_SHUTTING_DOWN`, Decision E).
    ShuttingDown,
}

impl fmt::Display for StepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PropagatorFailed { name, reason } => {
                write!(f, "propagator '{name}' failed: {reason}")
            }
            Self::AllocationFailed => write!(f, "arena allocation failed"),
            Self::TickRollback => write!(f, "tick rolled back"),
            Self::TickDisabled => write!(f, "ticking disabled after consecutive rollbacks"),
            Self::DtOutOfRange => write!(f, "dt exceeds propagator max_dt constraint"),
            Self::ShuttingDown => write!(f, "world is shutting down"),
        }
    }
}

impl Error for StepError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::PropagatorFailed { reason, .. } => Some(reason),
            _ => None,
        }
    }
}

/// Errors from individual propagator execution.
///
/// Returned by `Propagator::step()` and wrapped in
/// [`StepError::PropagatorFailed`] by the tick engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropagatorError {
    /// The propagator's step function failed
    /// (`MURK_ERROR_PROPAGATOR_FAILED`).
    ExecutionFailed {
        /// Human-readable description of the failure.
        reason: String,
    },
    /// NaN detected in propagator output (sentinel checking).
    NanDetected {
        /// The field containing the NaN.
        field_id: crate::FieldId,
        /// Index of the first NaN cell, if known.
        cell_index: Option<usize>,
    },
    /// A user-defined constraint was violated.
    ConstraintViolation {
        /// Description of the violated constraint.
        constraint: String,
    },
}

impl fmt::Display for PropagatorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExecutionFailed { reason } => write!(f, "execution failed: {reason}"),
            Self::NanDetected {
                field_id,
                cell_index,
            } => {
                write!(f, "NaN detected in field {field_id}")?;
                if let Some(idx) = cell_index {
                    write!(f, " at cell {idx}")?;
                }
                Ok(())
            }
            Self::ConstraintViolation { constraint } => {
                write!(f, "constraint violation: {constraint}")
            }
        }
    }
}

impl Error for PropagatorError {}

/// Errors from the ingress (command submission) pipeline.
///
/// Used in [`Receipt::reason_code`](crate::command::Receipt) to explain
/// why a command was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IngressError {
    /// The command queue is at capacity (`MURK_ERROR_QUEUE_FULL`).
    QueueFull,
    /// The command's `basis_tick_id` is too old (`MURK_ERROR_STALE`).
    Stale,
    /// The tick was rolled back; commands were dropped
    /// (`MURK_ERROR_TICK_ROLLBACK`).
    TickRollback,
    /// Ticking is disabled after consecutive rollbacks
    /// (`MURK_ERROR_TICK_DISABLED`).
    TickDisabled,
    /// The world is shutting down (`MURK_ERROR_SHUTTING_DOWN`).
    ShuttingDown,
    /// The command type is not supported by the current tick executor
    /// (`MURK_ERROR_UNSUPPORTED_COMMAND`).
    UnsupportedCommand,
}

impl fmt::Display for IngressError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueFull => write!(f, "command queue full"),
            Self::Stale => write!(f, "command basis_tick_id is stale"),
            Self::TickRollback => write!(f, "tick rolled back"),
            Self::TickDisabled => write!(f, "ticking disabled"),
            Self::ShuttingDown => write!(f, "world is shutting down"),
            Self::UnsupportedCommand => write!(f, "command type not supported"),
        }
    }
}

impl Error for IngressError {}

/// Errors from the observation (egress) pipeline.
///
/// Covers ObsPlan compilation, execution, and snapshot access failures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObsError {
    /// ObsPlan generation does not match the current snapshot
    /// (`MURK_ERROR_PLAN_INVALIDATED`).
    PlanInvalidated {
        /// Description of the generation mismatch.
        reason: String,
    },
    /// Exact-tick egress request timed out — RealtimeAsync only
    /// (`MURK_ERROR_TIMEOUT_WAITING_FOR_TICK`).
    TimeoutWaitingForTick,
    /// Requested tick has been evicted from the ring buffer
    /// (`MURK_ERROR_NOT_AVAILABLE`).
    NotAvailable,
    /// ObsPlan `valid_ratio` is below the 0.35 threshold
    /// (`MURK_ERROR_INVALID_COMPOSITION`).
    InvalidComposition {
        /// Description of the composition issue.
        reason: String,
    },
    /// ObsPlan execution failed mid-fill
    /// (`MURK_ERROR_EXECUTION_FAILED`).
    ExecutionFailed {
        /// Description of the execution failure.
        reason: String,
    },
    /// Malformed ObsSpec at compilation time
    /// (`MURK_ERROR_INVALID_OBSSPEC`).
    InvalidObsSpec {
        /// Description of the spec issue.
        reason: String,
    },
    /// Egress worker exceeded `max_epoch_hold`
    /// (`MURK_ERROR_WORKER_STALLED`).
    WorkerStalled,
}

impl fmt::Display for ObsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlanInvalidated { reason } => write!(f, "plan invalidated: {reason}"),
            Self::TimeoutWaitingForTick => write!(f, "timeout waiting for tick"),
            Self::NotAvailable => write!(f, "requested tick not available"),
            Self::InvalidComposition { reason } => write!(f, "invalid composition: {reason}"),
            Self::ExecutionFailed { reason } => write!(f, "execution failed: {reason}"),
            Self::InvalidObsSpec { reason } => write!(f, "invalid obsspec: {reason}"),
            Self::WorkerStalled => write!(f, "egress worker stalled"),
        }
    }
}

impl Error for ObsError {}
