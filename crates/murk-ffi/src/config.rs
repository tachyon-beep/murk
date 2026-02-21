//! Config builder FFI: create and populate a `WorldConfig` behind an opaque handle.
//!
//! C callers build a config incrementally, then pass the handle to
//! `murk_lockstep_create` which consumes it to construct a world.

use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::Mutex;

use murk_core::{BoundaryBehavior, FieldDef, FieldMutability, FieldType};
use murk_propagator::Propagator;
use murk_space::{
    EdgeBehavior, Fcc12, Hex2D, Line1D, ProductSpace, Ring1D, Space, Square4, Square8,
};

use crate::handle::HandleTable;
use crate::status::MurkStatus;
use crate::types::{
    MurkBoundaryBehavior, MurkEdgeBehavior, MurkFieldMutability, MurkFieldType, MurkSpaceType,
};

static CONFIGS: Mutex<HandleTable<ConfigBuilder>> = Mutex::new(HandleTable::new());

/// Internal config builder accumulated by FFI calls.
pub(crate) struct ConfigBuilder {
    pub space: Option<Box<dyn Space>>,
    pub fields: Vec<FieldDef>,
    pub propagators: Vec<Box<dyn Propagator>>,
    pub dt: f64,
    pub seed: u64,
    pub ring_buffer_size: usize,
    pub max_ingress_queue: usize,
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self {
            space: None,
            fields: Vec::new(),
            propagators: Vec::new(),
            dt: 0.016,
            seed: 0,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
        }
    }
}

pub(crate) fn configs() -> &'static Mutex<HandleTable<ConfigBuilder>> {
    &CONFIGS
}

// ── FFI functions ───────────────────────────────────────────────

/// Create a new config builder. Returns handle via `out`.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_config_create(out: *mut u64) -> i32 {
    ffi_guard!({
        // SAFETY: caller must pass a valid, aligned, non-null pointer.
        if out.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }
        let handle = ffi_lock!(CONFIGS).insert(ConfigBuilder::default());
        unsafe { *out = handle };
        MurkStatus::Ok as i32
    })
}

/// Destroy a config builder, releasing its resources.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_config_destroy(handle: u64) -> i32 {
    ffi_guard!({
        match ffi_lock!(CONFIGS).remove(handle) {
            Some(_) => MurkStatus::Ok as i32,
            None => MurkStatus::InvalidHandle as i32,
        }
    })
}

/// Safely convert an f64 FFI parameter to u32.
/// Rejects non-finite, negative, non-integer, and out-of-range values.
fn f64_to_u32(v: f64) -> Option<u32> {
    if !v.is_finite() || v < 0.0 || v > u32::MAX as f64 || v != v.trunc() {
        return None;
    }
    Some(v as u32)
}

/// Safely convert an f64 FFI parameter to usize.
/// Rejects non-finite, negative, non-integer, and overly large values.
fn f64_to_usize(v: f64) -> Option<usize> {
    if !v.is_finite() || v < 0.0 || v > (isize::MAX as f64) || v != v.trunc() {
        return None;
    }
    Some(v as usize)
}

/// Parse a space from its type tag and parameter slice.
///
/// Returns `None` if the type or parameters are invalid.
fn parse_space(space_type: i32, p: &[f64]) -> Option<Box<dyn Space>> {
    match space_type {
        x if x == MurkSpaceType::Line1D as i32 => {
            if p.len() < 2 {
                return None;
            }
            let len = f64_to_u32(p[0])?;
            let edge = parse_edge_behavior(p[1] as i32)?;
            Line1D::new(len, edge)
                .ok()
                .map(|s| Box::new(s) as Box<dyn Space>)
        }
        x if x == MurkSpaceType::Ring1D as i32 => {
            if p.is_empty() {
                return None;
            }
            let len = f64_to_u32(p[0])?;
            Ring1D::new(len).ok().map(|s| Box::new(s) as Box<dyn Space>)
        }
        x if x == MurkSpaceType::Square4 as i32 => {
            if p.len() < 3 {
                return None;
            }
            let w = f64_to_u32(p[0])?;
            let h = f64_to_u32(p[1])?;
            let edge = parse_edge_behavior(p[2] as i32)?;
            Square4::new(w, h, edge)
                .ok()
                .map(|s| Box::new(s) as Box<dyn Space>)
        }
        x if x == MurkSpaceType::Square8 as i32 => {
            if p.len() < 3 {
                return None;
            }
            let w = f64_to_u32(p[0])?;
            let h = f64_to_u32(p[1])?;
            let edge = parse_edge_behavior(p[2] as i32)?;
            Square8::new(w, h, edge)
                .ok()
                .map(|s| Box::new(s) as Box<dyn Space>)
        }
        x if x == MurkSpaceType::Hex2D as i32 => {
            // params = [cols, rows]
            if p.len() < 2 {
                return None;
            }
            let cols = f64_to_u32(p[0])?;
            let rows = f64_to_u32(p[1])?;
            Hex2D::new(rows, cols)
                .ok()
                .map(|s| Box::new(s) as Box<dyn Space>)
        }
        x if x == MurkSpaceType::Fcc12 as i32 => {
            // params = [w, h, d, edge_behavior]
            if p.len() < 4 {
                return None;
            }
            let w = f64_to_u32(p[0])?;
            let h = f64_to_u32(p[1])?;
            let d = f64_to_u32(p[2])?;
            let edge = parse_edge_behavior(p[3] as i32)?;
            Fcc12::new(w, h, d, edge)
                .ok()
                .map(|s| Box::new(s) as Box<dyn Space>)
        }
        x if x == MurkSpaceType::ProductSpace as i32 => {
            // params = [n_components, type_0, n_params_0, p0_0, ..., type_1, n_params_1, p1_0, ...]
            if p.is_empty() {
                return None;
            }
            let n_components = f64_to_usize(p[0])?;
            if n_components == 0 || n_components > p.len() {
                return None;
            }
            let mut components: Vec<Box<dyn Space>> = Vec::with_capacity(n_components);
            let mut offset = 1;
            for _ in 0..n_components {
                if offset + 2 > p.len() {
                    return None;
                }
                let comp_type = p[offset] as i32;
                let n_comp_params = f64_to_usize(p[offset + 1])?;
                offset += 2;
                if offset
                    .checked_add(n_comp_params)
                    .is_none_or(|end| end > p.len())
                {
                    return None;
                }
                let comp_params = &p[offset..offset + n_comp_params];
                offset += n_comp_params;
                let comp = parse_space(comp_type, comp_params)?;
                components.push(comp);
            }
            ProductSpace::new(components)
                .ok()
                .map(|s| Box::new(s) as Box<dyn Space>)
        }
        _ => None,
    }
}

/// Set the spatial topology for the config.
///
/// `params` is an array of `n_params` f64 values interpreted per space type:
/// - Line1D: \[length, edge_behavior\]
/// - Ring1D: \[length\]
/// - Square4/Square8: \[width, height, edge_behavior\]
/// - Hex2D: \[cols, rows\]
/// - Fcc12: \[w, h, d, edge_behavior\]
/// - ProductSpace: \[n_components, type_0, n_params_0, p0_0, ..., type_1, n_params_1, p1_0, ...\]
///
/// Edge behavior: 0=Absorb, 1=Clamp, 2=Wrap.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_config_set_space(
    handle: u64,
    space_type: i32,
    params: *const f64,
    n_params: usize,
) -> i32 {
    ffi_guard!({
        if params.is_null() && n_params > 0 {
            return MurkStatus::InvalidArgument as i32;
        }
        // SAFETY: caller guarantees params points to n_params valid f64 values.
        let p: &[f64] = if n_params > 0 {
            unsafe { std::slice::from_raw_parts(params, n_params) }
        } else {
            &[]
        };

        let space = match parse_space(space_type, p) {
            Some(s) => s,
            None => return MurkStatus::InvalidArgument as i32,
        };

        let mut table = ffi_lock!(CONFIGS);
        match table.get_mut(handle) {
            Some(cfg) => {
                cfg.space = Some(space);
                MurkStatus::Ok as i32
            }
            None => MurkStatus::InvalidHandle as i32,
        }
    })
}

/// Add a field definition to the config.
///
/// `name` is a null-terminated C string.
/// `field_type`: 0=Scalar, 1=Vector, 2=Categorical.
/// `mutability`: 0=Static, 1=PerTick, 2=Sparse.
/// `dims`: components for Vector, n_values for Categorical, ignored for Scalar.
/// `boundary_behavior`: 0=Clamp, 1=Reflect, 2=Absorb, 3=Wrap.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_config_add_field(
    handle: u64,
    name: *const c_char,
    field_type: i32,
    mutability: i32,
    dims: u32,
    boundary_behavior: i32,
) -> i32 {
    ffi_guard!({
        if name.is_null() {
            return MurkStatus::InvalidArgument as i32;
        }
        // SAFETY: caller guarantees name is a valid null-terminated C string.
        let name_str = match unsafe { CStr::from_ptr(name) }.to_str() {
            Ok(s) => s.to_owned(),
            Err(_) => return MurkStatus::InvalidArgument as i32,
        };

        let ft = match field_type {
            x if x == MurkFieldType::Scalar as i32 => FieldType::Scalar,
            x if x == MurkFieldType::Vector as i32 => FieldType::Vector { dims },
            x if x == MurkFieldType::Categorical as i32 => FieldType::Categorical { n_values: dims },
            _ => return MurkStatus::InvalidArgument as i32,
        };

        let mut_class = match mutability {
            x if x == MurkFieldMutability::Static as i32 => FieldMutability::Static,
            x if x == MurkFieldMutability::PerTick as i32 => FieldMutability::PerTick,
            x if x == MurkFieldMutability::Sparse as i32 => FieldMutability::Sparse,
            _ => return MurkStatus::InvalidArgument as i32,
        };

        let bb = match boundary_behavior {
            x if x == MurkBoundaryBehavior::Clamp as i32 => BoundaryBehavior::Clamp,
            x if x == MurkBoundaryBehavior::Reflect as i32 => BoundaryBehavior::Reflect,
            x if x == MurkBoundaryBehavior::Absorb as i32 => BoundaryBehavior::Absorb,
            x if x == MurkBoundaryBehavior::Wrap as i32 => BoundaryBehavior::Wrap,
            _ => return MurkStatus::InvalidArgument as i32,
        };

        let def = FieldDef {
            name: name_str,
            field_type: ft,
            mutability: mut_class,
            units: None,
            bounds: None,
            boundary_behavior: bb,
        };

        let mut table = ffi_lock!(CONFIGS);
        match table.get_mut(handle) {
            Some(cfg) => {
                cfg.fields.push(def);
                MurkStatus::Ok as i32
            }
            None => MurkStatus::InvalidHandle as i32,
        }
    })
}

/// Add a propagator to the config. Takes ownership of the propagator box.
///
/// `prop_ptr` is a `Box<dyn Propagator>` as a raw pointer cast to u64
/// (from `murk_propagator_create`).
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_config_add_propagator(handle: u64, prop_ptr: u64) -> i32 {
    ffi_guard!({
        if prop_ptr == 0 {
            return MurkStatus::InvalidArgument as i32;
        }
        // SAFETY: prop_ptr was produced by Box::into_raw(Box::new(boxed)) in
        // murk_propagator_create. It's a thin pointer to a Box<dyn Propagator>
        // and is consumed exactly once here.
        let prop: Box<dyn Propagator> = *unsafe { Box::from_raw(prop_ptr as *mut Box<dyn Propagator>) };

        let mut table = ffi_lock!(CONFIGS);
        match table.get_mut(handle) {
            Some(cfg) => {
                cfg.propagators.push(prop);
                MurkStatus::Ok as i32
            }
            None => MurkStatus::InvalidHandle as i32,
        }
    })
}

/// Set the simulation timestep in seconds.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_config_set_dt(handle: u64, dt: f64) -> i32 {
    ffi_guard!({
        let mut table = ffi_lock!(CONFIGS);
        match table.get_mut(handle) {
            Some(cfg) => {
                cfg.dt = dt;
                MurkStatus::Ok as i32
            }
            None => MurkStatus::InvalidHandle as i32,
        }
    })
}

/// Set the RNG seed.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_config_set_seed(handle: u64, seed: u64) -> i32 {
    ffi_guard!({
        let mut table = ffi_lock!(CONFIGS);
        match table.get_mut(handle) {
            Some(cfg) => {
                cfg.seed = seed;
                MurkStatus::Ok as i32
            }
            None => MurkStatus::InvalidHandle as i32,
        }
    })
}

/// Set the ring buffer size (minimum 2).
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_config_set_ring_buffer_size(handle: u64, size: usize) -> i32 {
    ffi_guard!({
        let mut table = ffi_lock!(CONFIGS);
        match table.get_mut(handle) {
            Some(cfg) => {
                cfg.ring_buffer_size = size;
                MurkStatus::Ok as i32
            }
            None => MurkStatus::InvalidHandle as i32,
        }
    })
}

/// Set the maximum ingress queue depth.
#[no_mangle]
#[allow(unsafe_code)]
pub extern "C" fn murk_config_set_max_ingress_queue(handle: u64, size: usize) -> i32 {
    ffi_guard!({
        let mut table = ffi_lock!(CONFIGS);
        match table.get_mut(handle) {
            Some(cfg) => {
                cfg.max_ingress_queue = size;
                MurkStatus::Ok as i32
            }
            None => MurkStatus::InvalidHandle as i32,
        }
    })
}

// ── helpers ──────────────────────────────────────────────

fn parse_edge_behavior(v: i32) -> Option<EdgeBehavior> {
    match v {
        x if x == MurkEdgeBehavior::Absorb as i32 => Some(EdgeBehavior::Absorb),
        x if x == MurkEdgeBehavior::Clamp as i32 => Some(EdgeBehavior::Clamp),
        x if x == MurkEdgeBehavior::Wrap as i32 => Some(EdgeBehavior::Wrap),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn create_set_space_add_field_destroy_round_trip() {
        let mut h: u64 = 0;
        assert_eq!(murk_config_create(&mut h), MurkStatus::Ok as i32);

        let params = [10.0f64, 0.0]; // Line1D, len=10, Absorb
        assert_eq!(
            murk_config_set_space(h, MurkSpaceType::Line1D as i32, params.as_ptr(), 2),
            MurkStatus::Ok as i32
        );

        let name = CString::new("energy").unwrap();
        assert_eq!(
            murk_config_add_field(
                h,
                name.as_ptr(),
                MurkFieldType::Scalar as i32,
                MurkFieldMutability::PerTick as i32,
                0,
                MurkBoundaryBehavior::Clamp as i32,
            ),
            MurkStatus::Ok as i32
        );

        assert_eq!(murk_config_set_dt(h, 0.1), MurkStatus::Ok as i32);

        assert_eq!(murk_config_destroy(h), MurkStatus::Ok as i32);
    }

    #[test]
    fn double_destroy_returns_invalid_handle() {
        let mut h: u64 = 0;
        murk_config_create(&mut h);
        assert_eq!(murk_config_destroy(h), MurkStatus::Ok as i32);
        assert_eq!(murk_config_destroy(h), MurkStatus::InvalidHandle as i32);
    }

    #[test]
    fn use_after_destroy_returns_invalid_handle() {
        let mut h: u64 = 0;
        murk_config_create(&mut h);
        murk_config_destroy(h);
        assert_eq!(murk_config_set_dt(h, 1.0), MurkStatus::InvalidHandle as i32);
    }

    #[test]
    fn invalid_space_type_returns_invalid_argument() {
        let mut h: u64 = 0;
        murk_config_create(&mut h);
        let params = [10.0f64];
        assert_eq!(
            murk_config_set_space(h, 999, params.as_ptr(), 1),
            MurkStatus::InvalidArgument as i32
        );
        murk_config_destroy(h);
    }

    #[test]
    fn null_config_create_returns_invalid_argument() {
        assert_eq!(
            murk_config_create(std::ptr::null_mut()),
            MurkStatus::InvalidArgument as i32
        );
    }

    #[test]
    fn square4_space_params() {
        let mut h: u64 = 0;
        murk_config_create(&mut h);
        let params = [5.0f64, 5.0, 0.0]; // 5x5, Absorb
        assert_eq!(
            murk_config_set_space(h, MurkSpaceType::Square4 as i32, params.as_ptr(), 3),
            MurkStatus::Ok as i32
        );
        murk_config_destroy(h);
    }

    #[test]
    fn null_params_with_count_returns_invalid_argument() {
        let mut h: u64 = 0;
        murk_config_create(&mut h);
        assert_eq!(
            murk_config_set_space(h, MurkSpaceType::Line1D as i32, std::ptr::null(), 2),
            MurkStatus::InvalidArgument as i32
        );
        murk_config_destroy(h);
    }

    #[test]
    fn null_field_name_returns_invalid_argument() {
        let mut h: u64 = 0;
        murk_config_create(&mut h);
        assert_eq!(
            murk_config_add_field(
                h,
                std::ptr::null(),
                MurkFieldType::Scalar as i32,
                MurkFieldMutability::PerTick as i32,
                0,
                MurkBoundaryBehavior::Clamp as i32,
            ),
            MurkStatus::InvalidArgument as i32
        );
        murk_config_destroy(h);
    }

    #[test]
    fn invalid_handle_set_seed_returns_invalid_handle() {
        assert_eq!(
            murk_config_set_seed(9999, 42),
            MurkStatus::InvalidHandle as i32
        );
    }

    #[test]
    fn invalid_handle_set_ring_buffer_returns_invalid_handle() {
        assert_eq!(
            murk_config_set_ring_buffer_size(9999, 8),
            MurkStatus::InvalidHandle as i32
        );
    }

    #[test]
    fn invalid_handle_set_max_ingress_returns_invalid_handle() {
        assert_eq!(
            murk_config_set_max_ingress_queue(9999, 1024),
            MurkStatus::InvalidHandle as i32
        );
    }

    #[test]
    fn hex2d_space_params() {
        let mut h: u64 = 0;
        murk_config_create(&mut h);
        let params = [3.0f64, 3.0]; // cols=3, rows=3
        assert_eq!(
            murk_config_set_space(h, MurkSpaceType::Hex2D as i32, params.as_ptr(), 2),
            MurkStatus::Ok as i32
        );
        murk_config_destroy(h);
    }

    #[test]
    fn ring1d_space_params() {
        let mut h: u64 = 0;
        murk_config_create(&mut h);
        let params = [10.0f64]; // len=10
        assert_eq!(
            murk_config_set_space(h, MurkSpaceType::Ring1D as i32, params.as_ptr(), 1),
            MurkStatus::Ok as i32
        );
        murk_config_destroy(h);
    }

    #[test]
    fn square8_space_params() {
        let mut h: u64 = 0;
        murk_config_create(&mut h);
        let params = [4.0f64, 4.0, 1.0]; // 4x4, Wrap
        assert_eq!(
            murk_config_set_space(h, MurkSpaceType::Square8 as i32, params.as_ptr(), 3),
            MurkStatus::Ok as i32
        );
        murk_config_destroy(h);
    }

    #[test]
    fn fcc12_space_params() {
        let mut h: u64 = 0;
        murk_config_create(&mut h);
        let params = [3.0f64, 3.0, 3.0, 0.0]; // 3x3x3, Absorb
        assert_eq!(
            murk_config_set_space(h, MurkSpaceType::Fcc12 as i32, params.as_ptr(), 4),
            MurkStatus::Ok as i32
        );
        murk_config_destroy(h);
    }
}
