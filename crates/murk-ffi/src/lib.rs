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

/// Lock a mutex, returning `MurkStatus::InternalError` if poisoned.
///
/// For use in `extern "C"` functions that return `i32`. On a poisoned
/// mutex (caused by a prior panic), this early-returns an error status
/// instead of panicking — preventing undefined behavior at the FFI boundary.
macro_rules! ffi_lock {
    ($mutex:expr) => {
        match ($mutex).lock() {
            Ok(guard) => guard,
            Err(_) => return $crate::status::MurkStatus::InternalError as i32,
        }
    };
}

use std::cell::RefCell;

thread_local! {
    /// Stores the last panic message caught by [`ffi_guard!`] on this thread.
    pub(crate) static LAST_PANIC: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Extract a human-readable message from a `catch_unwind` panic payload.
fn panic_message_from_payload(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_owned()
    }
}

/// Wrap an FFI function body so that Rust panics are caught instead of
/// unwinding across the `extern "C"` boundary (which is immediate UB).
///
/// On success the inner expression's value is returned.
/// On panic the message is stored in [`LAST_PANIC`] and
/// `MurkStatus::Panicked as i32` is returned.
macro_rules! ffi_guard {
    ($body:expr) => {{
        let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| $body));
        match result {
            Ok(val) => val,
            Err(payload) => {
                let msg = $crate::panic_message_from_payload(&payload);
                $crate::LAST_PANIC.with(|cell| {
                    *cell.borrow_mut() = msg;
                });
                $crate::status::MurkStatus::Panicked as i32
            }
        }
    }};
}

/// Like [`ffi_guard!`] but returns `$default` on panic instead of
/// `MurkStatus::Panicked as i32`. Useful for FFI functions whose return
/// type is not `i32`.
macro_rules! ffi_guard_or {
    ($default:expr, $body:expr) => {{
        let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| $body));
        match result {
            Ok(val) => val,
            Err(payload) => {
                let msg = $crate::panic_message_from_payload(&payload);
                $crate::LAST_PANIC.with(|cell| {
                    *cell.borrow_mut() = msg;
                });
                $default
            }
        }
    }};
}

/// Retrieve the panic message stored by the most recent [`ffi_guard!`] catch
/// on this thread.
///
/// - If `buf` is null, returns the full message length (in bytes) without
///   copying anything. Returns `0` if no panic has been recorded.
/// - Otherwise, if `cap > 0`, copies up to `cap - 1` bytes into `buf`,
///   null-terminates, and returns the full message length.
/// - If `cap == 0`, performs no writes and returns only the full message
///   length.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_last_panic_message(buf: *mut std::ffi::c_char, cap: usize) -> i32 {
    LAST_PANIC.with(|cell| {
        let msg = cell.borrow();
        if msg.is_empty() {
            return 0i32;
        }
        let len = msg.len();
        if buf.is_null() {
            return len as i32;
        }
        let copy_len = if cap > 0 { len.min(cap - 1) } else { 0 };
        // SAFETY: caller guarantees buf points to at least cap writable bytes.
        if cap > 0 {
            unsafe {
                std::ptr::copy_nonoverlapping(msg.as_ptr(), buf as *mut u8, copy_len);
                *buf.add(copy_len) = 0; // null terminator
            }
        }
        len as i32
    })
}

pub mod batched;
pub mod command;
pub mod config;
mod handle;
pub mod metrics;
pub mod obs;
pub mod propagator;
pub mod status;
pub mod types;
pub mod world;

pub use batched::{
    murk_batched_create, murk_batched_destroy, murk_batched_num_worlds, murk_batched_obs_mask_len,
    murk_batched_obs_output_len, murk_batched_observe_all, murk_batched_reset_all,
    murk_batched_reset_world, murk_batched_step_and_observe,
};
pub use command::{MurkCommand, MurkCommandType, MurkReceipt};
pub use config::{
    murk_config_add_field, murk_config_add_propagator, murk_config_create, murk_config_destroy,
    murk_config_set_dt, murk_config_set_max_ingress_queue, murk_config_set_ring_buffer_size,
    murk_config_set_seed, murk_config_set_space,
};
pub use metrics::{murk_step_metrics, murk_step_metrics_propagator, MurkStepMetrics};
pub use obs::{
    murk_obsplan_compile, murk_obsplan_destroy, murk_obsplan_execute, murk_obsplan_execute_agents,
    murk_obsplan_mask_len, murk_obsplan_output_len, MurkObsEntry, MurkObsResult,
};
pub use propagator::{murk_propagator_create, MurkPropagatorDef, MurkStepContext, MurkWriteDecl};
pub use status::MurkStatus;
pub use types::{
    MurkBoundaryBehavior, MurkEdgeBehavior, MurkFieldMutability, MurkFieldType, MurkSpaceType,
    MurkWriteMode,
};
pub use world::{
    murk_consecutive_rollbacks, murk_current_tick, murk_is_tick_disabled, murk_lockstep_create,
    murk_lockstep_destroy, murk_lockstep_reset, murk_lockstep_step, murk_lockstep_step_vec,
    murk_seed, murk_snapshot_read_field,
};

/// ABI version: major in upper 16 bits, minor in lower 16.
///
/// Bump major on breaking changes, minor on additions.
/// Current: v2.1 (v2.0→v2.1: ffi_guard! panic safety, murk_last_panic_message, MurkStatus::Panicked)
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_abi_version() -> u32 {
    (2 << 16) | 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_version_returns_v2_1() {
        let v = murk_abi_version();
        let major = v >> 16;
        let minor = v & 0xFFFF;
        assert_eq!(major, 2);
        assert_eq!(minor, 1);
    }

    #[test]
    fn ffi_guard_returns_inner_value_on_success() {
        let result = ffi_guard!(MurkStatus::Ok as i32);
        assert_eq!(result, 0);
    }

    #[test]
    fn ffi_guard_catches_panic_and_returns_panicked() {
        let result = ffi_guard!({
            panic!("test panic message");
        });
        assert_eq!(result, MurkStatus::Panicked as i32);
    }

    #[test]
    fn ffi_guard_or_returns_default_on_panic() {
        let result: u64 = ffi_guard_or!(42u64, {
            panic!("boom");
        });
        assert_eq!(result, 42u64);
    }

    #[test]
    fn last_panic_message_stored_on_panic() {
        let _ = ffi_guard!({
            panic!("hello from test");
        });
        let mut buf = [0u8; 64];
        let len = murk_last_panic_message(buf.as_mut_ptr() as *mut std::ffi::c_char, buf.len());
        let msg = std::str::from_utf8(&buf[..len as usize]).unwrap();
        assert_eq!(msg, "hello from test");
    }

    #[test]
    fn last_panic_message_returns_zero_when_no_panic() {
        LAST_PANIC.with(|cell| cell.borrow_mut().clear());
        let len = murk_last_panic_message(std::ptr::null_mut(), 0);
        assert_eq!(len, 0);
    }

    #[test]
    fn last_panic_message_with_zero_cap_non_null_buffer_only_returns_length() {
        let _ = ffi_guard!({
            panic!("zero-cap test");
        });

        let mut sentinel = [0x7Au8; 1];
        let len = murk_last_panic_message(sentinel.as_mut_ptr() as *mut std::ffi::c_char, 0);

        assert_eq!(len, "zero-cap test".len() as i32);
        assert_eq!(sentinel[0], 0x7A);
    }
}
