//! Arena-based generational allocation for Murk simulations.
//!
//! Provides bump-allocated arenas with generation tracking for
//! efficient, deterministic memory management. This crate is one
//! of two that may contain `unsafe` code (along with `murk-ffi`).

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![deny(unsafe_code)]
