//! Arena-based generational allocation for Murk simulations.
//!
//! Provides bump-allocated arenas with generation tracking for
//! efficient, deterministic memory management. This crate is one
//! of two that may contain `unsafe` code (along with `murk-ffi`).
//!
//! # Architecture
//!
//! The arena uses a double-buffered ("ping-pong") design:
//!
//! ```text
//! PingPongArena (orchestrator)
//! ├── ArenaBuffer × 2 (alternating published/staging)
//! │   └── SegmentList → Segment[] (64MB bump-allocated Vec<f32>)
//! ├── SparseSlab + sparse SegmentList (dedicated, not ping-pong'd)
//! ├── Arc<StaticArena> (gen-0 forever, shared across vectorized envs)
//! ├── ScratchRegion (per-tick temp space)
//! └── FieldDescriptor (FieldId → FieldEntry mapping, swapped on publish)
//! ```
//!
//! # Field mutability classes
//!
//! - **PerTick:** Fresh allocation each tick in the staging buffer.
//! - **Sparse:** Copy-on-write — shared until mutated, then new allocation.
//! - **Static:** Allocated once at world creation, shared via `Arc`.
//!
//! # Phase 1 safety
//!
//! All allocations are `Vec<f32>` with zero-init. No `MaybeUninit`, no
//! `unsafe`. Phase 2 (after WP-4) will introduce bounded `unsafe` in
//! `raw.rs` for `MaybeUninit` + `FullWriteGuard` optimisation.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(unsafe_code)]

pub mod config;
pub mod descriptor;
pub mod error;
pub mod handle;
pub mod pingpong;
mod raw;
pub mod read;
pub mod scratch;
pub mod segment;
pub mod sparse;
pub mod static_arena;
pub mod write;

// Public re-exports for the primary API surface.
pub use config::ArenaConfig;
pub use error::ArenaError;
pub use pingpong::{PingPongArena, TickGuard};
pub use read::Snapshot;
pub use scratch::ScratchRegion;
pub use static_arena::{SharedStaticArena, StaticArena};
