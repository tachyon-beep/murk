//! C-compatible status codes mapping HLD §9.7 error codes.
//!
//! [`MurkStatus`] is a `repr(i32)` enum covering all error conditions
//! from the Murk simulation framework. Conversions from Rust error types
//! (`StepError`, `ObsError`, `ConfigError`, `TickError`) are provided.

use murk_core::error::{IngressError, ObsError, StepError};
use murk_engine::config::ConfigError;
use murk_engine::tick::TickError;

/// C-compatible status code returned by all FFI functions.
///
/// `Ok` = 0, all errors are negative. Values are ABI-stable.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MurkStatus {
    /// Success.
    Ok = 0,
    /// Handle is invalid or was already destroyed.
    InvalidHandle = -1,
    /// Observation plan was compiled for a different generation.
    PlanInvalidated = -2,
    /// Exact-tick egress request timed out (RealtimeAsync only).
    TimeoutWaitingForTick = -3,
    /// Requested tick evicted from ring buffer.
    NotAvailable = -4,
    /// ObsPlan valid_ratio below threshold.
    InvalidComposition = -5,
    /// Command queue at capacity.
    QueueFull = -6,
    /// Command basis_tick_id is too old.
    Stale = -7,
    /// Tick was rolled back.
    TickRollback = -8,
    /// Arena allocation failed (OOM).
    AllocationFailed = -9,
    /// A propagator's step function failed.
    PropagatorFailed = -10,
    /// Observation plan execution failed mid-fill.
    ExecutionFailed = -11,
    /// Malformed ObsSpec at compilation time.
    InvalidObsSpec = -12,
    /// dt exceeds a propagator's max_dt constraint.
    DtOutOfRange = -13,
    /// Egress worker exceeded max_epoch_hold.
    WorkerStalled = -14,
    /// World is shutting down.
    ShuttingDown = -15,
    /// Ticking disabled after consecutive rollbacks.
    TickDisabled = -16,
    /// Configuration validation error.
    ConfigError = -17,
    /// An argument is null, out of range, or otherwise invalid.
    InvalidArgument = -18,
    /// Caller-provided buffer is too small.
    BufferTooSmall = -19,
    /// Internal error (e.g. poisoned mutex after a prior panic).
    InternalError = -20,
    /// Command type not supported by the tick executor.
    UnsupportedCommand = -21,
    /// Command was accepted but could not be applied (e.g. invalid coordinate
    /// or unknown field).
    NotApplied = -22,
    /// A Rust panic was caught at the FFI boundary.
    Panicked = -128,
}

impl From<&StepError> for MurkStatus {
    fn from(e: &StepError) -> Self {
        match e {
            StepError::PropagatorFailed { .. } => MurkStatus::PropagatorFailed,
            StepError::AllocationFailed => MurkStatus::AllocationFailed,
            StepError::TickRollback => MurkStatus::TickRollback,
            StepError::TickDisabled => MurkStatus::TickDisabled,
            StepError::DtOutOfRange => MurkStatus::DtOutOfRange,
            StepError::ShuttingDown => MurkStatus::ShuttingDown,
        }
    }
}

impl From<&TickError> for MurkStatus {
    fn from(e: &TickError) -> Self {
        MurkStatus::from(&e.kind)
    }
}

impl From<&ObsError> for MurkStatus {
    fn from(e: &ObsError) -> Self {
        match e {
            ObsError::PlanInvalidated { .. } => MurkStatus::PlanInvalidated,
            ObsError::TimeoutWaitingForTick => MurkStatus::TimeoutWaitingForTick,
            ObsError::NotAvailable => MurkStatus::NotAvailable,
            ObsError::InvalidComposition { .. } => MurkStatus::InvalidComposition,
            ObsError::ExecutionFailed { .. } => MurkStatus::ExecutionFailed,
            ObsError::InvalidObsSpec { .. } => MurkStatus::InvalidObsSpec,
            ObsError::WorkerStalled => MurkStatus::WorkerStalled,
        }
    }
}

impl From<&ConfigError> for MurkStatus {
    fn from(_e: &ConfigError) -> Self {
        MurkStatus::ConfigError
    }
}

impl From<&IngressError> for MurkStatus {
    fn from(e: &IngressError) -> Self {
        match e {
            IngressError::QueueFull => MurkStatus::QueueFull,
            IngressError::Stale => MurkStatus::Stale,
            IngressError::TickRollback => MurkStatus::TickRollback,
            IngressError::TickDisabled => MurkStatus::TickDisabled,
            IngressError::ShuttingDown => MurkStatus::ShuttingDown,
            IngressError::UnsupportedCommand => MurkStatus::UnsupportedCommand,
            IngressError::NotApplied => MurkStatus::NotApplied,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::error::PropagatorError;

    #[test]
    fn status_code_values_are_stable() {
        assert_eq!(MurkStatus::Ok as i32, 0);
        assert_eq!(MurkStatus::InvalidHandle as i32, -1);
        assert_eq!(MurkStatus::PlanInvalidated as i32, -2);
        assert_eq!(MurkStatus::TimeoutWaitingForTick as i32, -3);
        assert_eq!(MurkStatus::NotAvailable as i32, -4);
        assert_eq!(MurkStatus::InvalidComposition as i32, -5);
        assert_eq!(MurkStatus::QueueFull as i32, -6);
        assert_eq!(MurkStatus::Stale as i32, -7);
        assert_eq!(MurkStatus::TickRollback as i32, -8);
        assert_eq!(MurkStatus::AllocationFailed as i32, -9);
        assert_eq!(MurkStatus::PropagatorFailed as i32, -10);
        assert_eq!(MurkStatus::ExecutionFailed as i32, -11);
        assert_eq!(MurkStatus::InvalidObsSpec as i32, -12);
        assert_eq!(MurkStatus::DtOutOfRange as i32, -13);
        assert_eq!(MurkStatus::WorkerStalled as i32, -14);
        assert_eq!(MurkStatus::ShuttingDown as i32, -15);
        assert_eq!(MurkStatus::TickDisabled as i32, -16);
        assert_eq!(MurkStatus::ConfigError as i32, -17);
        assert_eq!(MurkStatus::InvalidArgument as i32, -18);
        assert_eq!(MurkStatus::BufferTooSmall as i32, -19);
        assert_eq!(MurkStatus::InternalError as i32, -20);
        assert_eq!(MurkStatus::UnsupportedCommand as i32, -21);
        assert_eq!(MurkStatus::NotApplied as i32, -22);
    }

    #[test]
    fn step_error_to_status() {
        assert_eq!(
            MurkStatus::from(&StepError::AllocationFailed),
            MurkStatus::AllocationFailed
        );
        assert_eq!(
            MurkStatus::from(&StepError::TickDisabled),
            MurkStatus::TickDisabled
        );
        assert_eq!(
            MurkStatus::from(&StepError::PropagatorFailed {
                name: "test".into(),
                reason: PropagatorError::ExecutionFailed {
                    reason: "boom".into()
                },
            }),
            MurkStatus::PropagatorFailed
        );
        assert_eq!(
            MurkStatus::from(&StepError::TickRollback),
            MurkStatus::TickRollback
        );
        assert_eq!(
            MurkStatus::from(&StepError::DtOutOfRange),
            MurkStatus::DtOutOfRange
        );
        assert_eq!(
            MurkStatus::from(&StepError::ShuttingDown),
            MurkStatus::ShuttingDown
        );
    }

    #[test]
    fn obs_error_to_status() {
        assert_eq!(
            MurkStatus::from(&ObsError::PlanInvalidated { reason: "x".into() }),
            MurkStatus::PlanInvalidated
        );
        assert_eq!(
            MurkStatus::from(&ObsError::TimeoutWaitingForTick),
            MurkStatus::TimeoutWaitingForTick
        );
        assert_eq!(
            MurkStatus::from(&ObsError::NotAvailable),
            MurkStatus::NotAvailable
        );
        assert_eq!(
            MurkStatus::from(&ObsError::InvalidComposition { reason: "x".into() }),
            MurkStatus::InvalidComposition
        );
        assert_eq!(
            MurkStatus::from(&ObsError::ExecutionFailed { reason: "x".into() }),
            MurkStatus::ExecutionFailed
        );
        assert_eq!(
            MurkStatus::from(&ObsError::InvalidObsSpec { reason: "x".into() }),
            MurkStatus::InvalidObsSpec
        );
        assert_eq!(
            MurkStatus::from(&ObsError::WorkerStalled),
            MurkStatus::WorkerStalled
        );
    }

    #[test]
    fn ingress_error_to_status() {
        assert_eq!(
            MurkStatus::from(&IngressError::QueueFull),
            MurkStatus::QueueFull
        );
        assert_eq!(MurkStatus::from(&IngressError::Stale), MurkStatus::Stale);
        assert_eq!(
            MurkStatus::from(&IngressError::TickRollback),
            MurkStatus::TickRollback
        );
        assert_eq!(
            MurkStatus::from(&IngressError::TickDisabled),
            MurkStatus::TickDisabled
        );
        assert_eq!(
            MurkStatus::from(&IngressError::ShuttingDown),
            MurkStatus::ShuttingDown
        );
        assert_eq!(
            MurkStatus::from(&IngressError::UnsupportedCommand),
            MurkStatus::UnsupportedCommand
        );
        assert_eq!(
            MurkStatus::from(&IngressError::NotApplied),
            MurkStatus::NotApplied
        );
    }

    #[test]
    fn panicked_status_is_negative_128() {
        assert_eq!(MurkStatus::Panicked as i32, -128);
    }

    #[test]
    fn tick_error_to_status() {
        let te = TickError {
            kind: StepError::TickDisabled,
            receipts: vec![],
        };
        assert_eq!(MurkStatus::from(&te), MurkStatus::TickDisabled);
    }
}
