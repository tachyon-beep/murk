//! C callback propagator: bridges `MurkPropagatorDef` to the Rust `Propagator` trait.
//!
//! C code defines a propagator via function pointers in [`MurkPropagatorDef`].
//! `murk_propagator_create` wraps it in a `CallbackPropagator` and returns
//! a raw pointer (as `u64`) consumed by `murk_config_add_propagator`.

use std::ffi::{c_char, c_void, CStr};

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::propagator::WriteMode;
use murk_propagator::StepContext;

use crate::status::MurkStatus;
use crate::types::MurkWriteMode;

/// Write declaration: (field_id, write_mode) pair.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MurkWriteDecl {
    /// Field ID to write.
    pub field_id: u32,
    /// Write mode (0 = Full, 1 = Incremental).
    /// Stored as raw i32 to prevent UB from invalid C discriminators.
    pub mode: i32,
}

/// C-side step context — flat struct with function pointers for field access.
///
/// Passed to the C `step_fn` callback. The `opaque` pointer wraps the
/// Rust `StepContext` internals; C code accesses fields through the
/// provided function pointers.
#[repr(C)]
pub struct MurkStepContext {
    /// Opaque pointer to internal wrapper (do not dereference in C).
    pub opaque: *mut c_void,
    /// Read current-tick field data: `(opaque, field_id, out_ptr, out_len) -> status`.
    pub read_fn: unsafe extern "C" fn(*mut c_void, u32, *mut *const f32, *mut usize) -> i32,
    /// Read previous-tick field data.
    pub read_previous_fn:
        unsafe extern "C" fn(*mut c_void, u32, *mut *const f32, *mut usize) -> i32,
    /// Get mutable write buffer: `(opaque, field_id, out_ptr, out_len) -> status`.
    pub write_fn: unsafe extern "C" fn(*mut c_void, u32, *mut *mut f32, *mut usize) -> i32,
    /// Current tick ID.
    pub tick_id: u64,
    /// Simulation timestep.
    pub dt: f64,
    /// Number of cells in the space.
    /// Fixed-width `u64` for ABI portability (not `usize`).
    pub cell_count: u64,
}

/// C-side propagator definition with function pointers.
///
/// Caller must ensure all pointers remain valid until the propagator is
/// consumed by `murk_config_add_propagator`. After consumption, only
/// `step_fn` and `user_data` must remain valid for the world's lifetime.
#[repr(C)]
pub struct MurkPropagatorDef {
    /// Null-terminated propagator name.
    pub name: *const c_char,
    /// Array of field IDs this propagator reads (current tick).
    pub reads: *const u32,
    /// Length of `reads` array.
    pub n_reads: usize,
    /// Array of field IDs this propagator reads (previous tick).
    pub reads_previous: *const u32,
    /// Length of `reads_previous` array.
    pub n_reads_previous: usize,
    /// Array of write declarations.
    pub writes: *const MurkWriteDecl,
    /// Length of `writes` array.
    pub n_writes: usize,
    /// Step function called each tick (must not be null).
    pub step_fn: Option<unsafe extern "C" fn(*mut c_void, *const MurkStepContext) -> i32>,
    /// User data pointer passed to `step_fn`.
    pub user_data: *mut c_void,
    /// Scratch memory bytes required.
    pub scratch_bytes: usize,
}

// Compile-time layout assertions for ABI stability.
// MurkWriteDecl: u32 + i32 = 8 bytes.
const _: () = assert!(std::mem::size_of::<MurkWriteDecl>() == 8);
// MurkStepContext: all fixed-width types (u64 cell_count, not usize).
const _: () = assert!(std::mem::align_of::<MurkStepContext>() == 8);

/// Rust-side wrapper that implements `Propagator` by delegating to C callbacks.
pub(crate) struct CallbackPropagator {
    name: String,
    reads: FieldSet,
    reads_previous: FieldSet,
    writes: Vec<(FieldId, WriteMode)>,
    step_fn: unsafe extern "C" fn(*mut c_void, *const MurkStepContext) -> i32,
    user_data: *mut c_void,
    scratch: usize,
}

// SAFETY: The FFI contract requires user_data to be transferable across threads.
// C callers must ensure user_data remains valid on the thread where the engine runs.
#[allow(unsafe_code)]
unsafe impl Send for CallbackPropagator {}

// NOTE: CallbackPropagator is deliberately `!Sync`.
//
// `Sync` would permit `&self` access (including `step(&self)`) from multiple threads
// simultaneously. Since `user_data` is an opaque `*mut c_void` from C code, we cannot
// guarantee it is safe for concurrent reads — most C callback state is not thread-safe.
//
// Currently, `LockstepWorld` wraps each world in `Mutex`, which serializes all calls
// to `step()`. This `Mutex` is the load-bearing invariant that makes `!Sync` safe.
// If the engine later supports parallel propagator execution (e.g. RealtimeAsync mode
// with a thread pool), this `!Sync` will produce a compile error — which is the
// desired outcome, forcing an explicit decision about C callback thread safety.

impl murk_propagator::Propagator for CallbackPropagator {
    fn name(&self) -> &str {
        &self.name
    }

    fn reads(&self) -> FieldSet {
        self.reads.clone()
    }

    fn reads_previous(&self) -> FieldSet {
        self.reads_previous.clone()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        self.writes.clone()
    }

    fn scratch_bytes(&self) -> usize {
        self.scratch
    }

    #[allow(unsafe_code)]
    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        // Build the MurkStepContext with trampolines that call back into Rust.
        let mut wrapper = StepContextWrapper { ctx };
        let c_ctx = MurkStepContext {
            opaque: &mut wrapper as *mut StepContextWrapper<'_, '_> as *mut c_void,
            read_fn: trampoline_read,
            read_previous_fn: trampoline_read_previous,
            write_fn: trampoline_write,
            tick_id: ctx.tick_id().0,
            dt: ctx.dt(),
            cell_count: ctx.space().cell_count() as u64,
        };

        // SAFETY: step_fn and user_data are valid per FFI contract.
        let rc = unsafe { (self.step_fn)(self.user_data, &c_ctx) };

        if rc == 0 {
            Ok(())
        } else {
            Err(PropagatorError::ExecutionFailed {
                reason: format!("C callback returned error code {rc}"),
            })
        }
    }
}

// Trampoline wrapper to recover StepContext from opaque pointer.
struct StepContextWrapper<'a, 'b> {
    ctx: &'a mut StepContext<'b>,
}

#[allow(unsafe_code)]
unsafe extern "C" fn trampoline_read(
    opaque: *mut c_void,
    field_id: u32,
    out_ptr: *mut *const f32,
    out_len: *mut usize,
) -> i32 {
    if opaque.is_null() || out_ptr.is_null() || out_len.is_null() {
        return MurkStatus::InvalidArgument as i32;
    }
    // SAFETY: opaque was set to &mut StepContextWrapper in step() above.
    let wrapper = &*(opaque as *const StepContextWrapper<'_, '_>);
    match wrapper.ctx.reads().read(FieldId(field_id)) {
        Some(data) => {
            *out_ptr = data.as_ptr();
            *out_len = data.len();
            0
        }
        None => MurkStatus::InvalidArgument as i32,
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn trampoline_read_previous(
    opaque: *mut c_void,
    field_id: u32,
    out_ptr: *mut *const f32,
    out_len: *mut usize,
) -> i32 {
    if opaque.is_null() || out_ptr.is_null() || out_len.is_null() {
        return MurkStatus::InvalidArgument as i32;
    }
    // SAFETY: opaque was set to &mut StepContextWrapper in step() above.
    let wrapper = &*(opaque as *const StepContextWrapper<'_, '_>);
    match wrapper.ctx.reads_previous().read(FieldId(field_id)) {
        Some(data) => {
            *out_ptr = data.as_ptr();
            *out_len = data.len();
            0
        }
        None => MurkStatus::InvalidArgument as i32,
    }
}

#[allow(unsafe_code)]
unsafe extern "C" fn trampoline_write(
    opaque: *mut c_void,
    field_id: u32,
    out_ptr: *mut *mut f32,
    out_len: *mut usize,
) -> i32 {
    if opaque.is_null() || out_ptr.is_null() || out_len.is_null() {
        return MurkStatus::InvalidArgument as i32;
    }
    // SAFETY: opaque was set to &mut StepContextWrapper in step() above.
    let wrapper = &mut *(opaque as *mut StepContextWrapper<'_, '_>);
    match wrapper.ctx.writes().write(FieldId(field_id)) {
        Some(data) => {
            *out_ptr = data.as_mut_ptr();
            *out_len = data.len();
            0
        }
        None => MurkStatus::InvalidArgument as i32,
    }
}

/// Create a propagator from a C definition.
///
/// Returns a raw pointer (as `u64`) to be passed to `murk_config_add_propagator`.
/// The pointer is consumed exactly once; double-use is UB.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_propagator_create(
    def: *const MurkPropagatorDef,
    out_handle: *mut u64,
) -> i32 {
    ffi_guard!({
        if def.is_null() || out_handle.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }

        // SAFETY: def is a valid pointer per caller contract.
        let def = unsafe { &*def };

        if def.name.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }

        let step_fn = match def.step_fn {
            Some(f) => f,
            None => return MurkStatus::InvalidArgument as i32,
        };

        let name = match unsafe { CStr::from_ptr(def.name) }.to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => return MurkStatus::InvalidArgument as i32,
        };

        let mut reads = FieldSet::empty();
        if def.n_reads > 0 {
            if def.reads.is_null() {
                return MurkStatus::InvalidArgument as i32;
            }
            let slice = unsafe { std::slice::from_raw_parts(def.reads, def.n_reads) };
            for &id in slice {
                reads.insert(FieldId(id));
            }
        }

        let mut reads_previous = FieldSet::empty();
        if def.n_reads_previous > 0 {
            if def.reads_previous.is_null() {
                return MurkStatus::InvalidArgument as i32;
            }
            let slice =
                unsafe { std::slice::from_raw_parts(def.reads_previous, def.n_reads_previous) };
            for &id in slice {
                reads_previous.insert(FieldId(id));
            }
        }

        let mut writes = Vec::new();
        if def.n_writes > 0 {
            if def.writes.is_null() {
                return MurkStatus::InvalidArgument as i32;
            }
            let slice = unsafe { std::slice::from_raw_parts(def.writes, def.n_writes) };
            for decl in slice {
                let mode = match decl.mode {
                    x if x == MurkWriteMode::Full as i32 => WriteMode::Full,
                    x if x == MurkWriteMode::Incremental as i32 => WriteMode::Incremental,
                    _ => return MurkStatus::InvalidArgument as i32,
                };
                writes.push((FieldId(decl.field_id), mode));
            }
        }

        let prop = CallbackPropagator {
            name,
            reads,
            reads_previous,
            writes,
            step_fn,
            user_data: def.user_data,
            scratch: def.scratch_bytes,
        };

        let boxed: Box<dyn murk_propagator::Propagator> = Box::new(prop);
        // Double-box: Box<dyn Propagator> is a fat pointer (2 words), can't fit in u64.
        // Box::new(boxed) yields a thin pointer to the heap-allocated fat pointer.
        let raw = Box::into_raw(Box::new(boxed)) as u64;
        unsafe { *out_handle = raw };
        MurkStatus::Ok as i32
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    // Test step function: writes constant 42.0 to field 0.
    #[allow(unsafe_code)]
    unsafe extern "C" fn test_step_fn(_user_data: *mut c_void, ctx: *const MurkStepContext) -> i32 {
        let ctx = &*ctx;
        let mut ptr: *mut f32 = std::ptr::null_mut();
        let mut len: usize = 0;
        let rc = (ctx.write_fn)(ctx.opaque, 0, &mut ptr, &mut len);
        if rc != 0 {
            return rc;
        }
        let slice = std::slice::from_raw_parts_mut(ptr, len);
        for v in slice {
            *v = 42.0;
        }
        0
    }

    fn make_def(
        name: &CString,
        step_fn: unsafe extern "C" fn(*mut c_void, *const MurkStepContext) -> i32,
    ) -> (Vec<MurkWriteDecl>, MurkPropagatorDef) {
        let writes = vec![MurkWriteDecl {
            field_id: 0,
            mode: MurkWriteMode::Full as i32,
        }];
        let def = MurkPropagatorDef {
            name: name.as_ptr(),
            reads: std::ptr::null(),
            n_reads: 0,
            reads_previous: std::ptr::null(),
            n_reads_previous: 0,
            writes: writes.as_ptr(),
            n_writes: writes.len(),
            step_fn: Some(step_fn),
            user_data: std::ptr::null_mut(),
            scratch_bytes: 0,
        };
        (writes, def)
    }

    #[test]
    #[allow(unsafe_code)]
    fn propagator_create_succeeds() {
        let name = CString::new("test_prop").unwrap();
        let (_writes, def) = make_def(&name, test_step_fn);
        let mut handle: u64 = 0;
        let status = murk_propagator_create(&def, &mut handle);
        assert_eq!(status, MurkStatus::Ok as i32);
        assert_ne!(handle, 0);

        // Clean up: reconstruct double-box to drop it.
        unsafe {
            let _ = Box::from_raw(handle as *mut Box<dyn murk_propagator::Propagator>);
        }
    }

    #[test]
    #[allow(unsafe_code)]
    fn callback_propagator_declarations_match() {
        let name = CString::new("decl_test").unwrap();
        let reads = [0u32, 1];
        let writes = [
            MurkWriteDecl {
                field_id: 2,
                mode: MurkWriteMode::Full as i32,
            },
            MurkWriteDecl {
                field_id: 3,
                mode: MurkWriteMode::Incremental as i32,
            },
        ];
        let def = MurkPropagatorDef {
            name: name.as_ptr(),
            reads: reads.as_ptr(),
            n_reads: 2,
            reads_previous: std::ptr::null(),
            n_reads_previous: 0,
            writes: writes.as_ptr(),
            n_writes: 2,
            step_fn: Some(test_step_fn),
            user_data: std::ptr::null_mut(),
            scratch_bytes: 64,
        };

        let mut handle: u64 = 0;
        murk_propagator_create(&def, &mut handle);

        // Reconstruct double-box to inspect.
        let prop: Box<dyn murk_propagator::Propagator> =
            *unsafe { Box::from_raw(handle as *mut Box<dyn murk_propagator::Propagator>) };
        assert_eq!(prop.name(), "decl_test");
        assert!(prop.reads().contains(FieldId(0)));
        assert!(prop.reads().contains(FieldId(1)));
        assert_eq!(prop.reads().len(), 2);
        assert!(prop.reads_previous().is_empty());
        let w = prop.writes();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0], (FieldId(2), WriteMode::Full));
        assert_eq!(w[1], (FieldId(3), WriteMode::Incremental));
        assert_eq!(prop.scratch_bytes(), 64);
    }

    #[test]
    fn null_def_returns_invalid_argument() {
        let mut handle: u64 = 0;
        assert_eq!(
            murk_propagator_create(std::ptr::null(), &mut handle),
            MurkStatus::InvalidArgument as i32
        );
    }

    #[test]
    fn null_step_fn_returns_invalid_argument() {
        let name = CString::new("bad").unwrap();
        let writes = [MurkWriteDecl {
            field_id: 0,
            mode: MurkWriteMode::Full as i32,
        }];
        let def = MurkPropagatorDef {
            name: name.as_ptr(),
            reads: std::ptr::null(),
            n_reads: 0,
            reads_previous: std::ptr::null(),
            n_reads_previous: 0,
            writes: writes.as_ptr(),
            n_writes: 1,
            step_fn: None,
            user_data: std::ptr::null_mut(),
            scratch_bytes: 0,
        };
        let mut handle: u64 = 0;
        assert_eq!(
            murk_propagator_create(&def, &mut handle),
            MurkStatus::InvalidArgument as i32
        );
    }

    #[test]
    fn null_reads_with_nonzero_count_returns_invalid_argument() {
        let name = CString::new("bad_reads").unwrap();
        let writes = [MurkWriteDecl {
            field_id: 0,
            mode: MurkWriteMode::Full as i32,
        }];
        let def = MurkPropagatorDef {
            name: name.as_ptr(),
            reads: std::ptr::null(),
            n_reads: 3, // mismatch: count > 0 but pointer is null
            reads_previous: std::ptr::null(),
            n_reads_previous: 0,
            writes: writes.as_ptr(),
            n_writes: 1,
            step_fn: Some(test_step_fn),
            user_data: std::ptr::null_mut(),
            scratch_bytes: 0,
        };
        let mut handle: u64 = 0;
        assert_eq!(
            murk_propagator_create(&def, &mut handle),
            MurkStatus::InvalidArgument as i32
        );
    }

    #[test]
    fn null_writes_with_nonzero_count_returns_invalid_argument() {
        let name = CString::new("bad_writes").unwrap();
        let def = MurkPropagatorDef {
            name: name.as_ptr(),
            reads: std::ptr::null(),
            n_reads: 0,
            reads_previous: std::ptr::null(),
            n_reads_previous: 0,
            writes: std::ptr::null(),
            n_writes: 2, // mismatch
            step_fn: Some(test_step_fn),
            user_data: std::ptr::null_mut(),
            scratch_bytes: 0,
        };
        let mut handle: u64 = 0;
        assert_eq!(
            murk_propagator_create(&def, &mut handle),
            MurkStatus::InvalidArgument as i32
        );
    }

    #[test]
    fn invalid_write_mode_returns_invalid_argument() {
        let name = CString::new("bad_mode").unwrap();
        let writes = [MurkWriteDecl {
            field_id: 0,
            mode: 999, // invalid write mode
        }];
        let def = MurkPropagatorDef {
            name: name.as_ptr(),
            reads: std::ptr::null(),
            n_reads: 0,
            reads_previous: std::ptr::null(),
            n_reads_previous: 0,
            writes: writes.as_ptr(),
            n_writes: 1,
            step_fn: Some(test_step_fn),
            user_data: std::ptr::null_mut(),
            scratch_bytes: 0,
        };
        let mut handle: u64 = 0;
        assert_eq!(
            murk_propagator_create(&def, &mut handle),
            MurkStatus::InvalidArgument as i32
        );
    }
}
