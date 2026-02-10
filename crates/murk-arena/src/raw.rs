//! Low-level primitives for arena memory operations.
//!
//! Phase 1: empty placeholder. All allocations use `Vec<f32>` with zero-init.
//!
//! Phase 2 (after WP-4 delivers `FullWriteGuard`): this module will contain
//! ≤5 `unsafe` functions for `MaybeUninit<f32>` support, each with a
//! mandatory `// SAFETY:` comment and Miri coverage.

#![allow(unsafe_code)]
