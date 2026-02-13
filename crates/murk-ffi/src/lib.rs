//! C FFI bindings for the Murk simulation framework.
//!
//! Exposes a C-compatible API for language bindings. This crate is one
//! of two that may contain `unsafe` code (along with `murk-arena`).
//!
//! # Handle Model
//!
//! All Rust objects exposed to C are managed through slot+generation handle
//! tables. Destroyed handles return `MURK_ERROR_INVALID_HANDLE` instead of
//! causing UB. Double-destroy is a safe no-op.
//!
//! # ABI Versioning
//!
//! Call [`murk_abi_version()`] to retrieve the ABI version as a packed u32:
//! major in upper 16 bits, minor in lower 16.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(unsafe_code)]
// FFI functions inherently dereference raw pointers; safety is documented per-block.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

pub mod command;
pub mod config;
mod handle;
pub mod metrics;
pub mod obs;
pub mod propagator;
pub mod status;
pub mod types;
pub mod world;

pub use command::{MurkCommand, MurkCommandType, MurkReceipt};
pub use config::{
    murk_config_add_field, murk_config_add_propagator, murk_config_create, murk_config_destroy,
    murk_config_set_dt, murk_config_set_max_ingress_queue, murk_config_set_ring_buffer_size,
    murk_config_set_seed, murk_config_set_space,
};
pub use metrics::{murk_step_metrics, murk_step_metrics_propagator, MurkStepMetrics};
pub use obs::{
    murk_obsplan_compile, murk_obsplan_destroy, murk_obsplan_execute,
    murk_obsplan_execute_agents, murk_obsplan_mask_len, murk_obsplan_output_len, MurkObsEntry,
    MurkObsResult,
};
pub use propagator::{
    murk_propagator_create, MurkPropagatorDef, MurkStepContext, MurkWriteDecl,
};
pub use status::MurkStatus;
pub use types::{
    MurkBoundaryBehavior, MurkEdgeBehavior, MurkFieldMutability, MurkFieldType, MurkSpaceType,
    MurkWriteMode,
};
pub use world::{
    murk_consecutive_rollbacks, murk_current_tick, murk_is_tick_disabled,
    murk_lockstep_create, murk_lockstep_destroy, murk_lockstep_reset, murk_lockstep_step,
    murk_lockstep_step_vec, murk_seed, murk_snapshot_read_field,
};

/// ABI version: major in upper 16 bits, minor in lower 16.
///
/// Bump major on breaking changes, minor on additions.
/// Current: v1.0.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_abi_version() -> u32 {
    1 << 16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_version_returns_v1_0() {
        let v = murk_abi_version();
        let major = v >> 16;
        let minor = v & 0xFFFF;
        assert_eq!(major, 1);
        assert_eq!(minor, 0);
    }
}
