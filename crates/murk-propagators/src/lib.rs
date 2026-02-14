//! Reference propagators for the Murk simulation framework.
//!
//! Provides production-quality propagators that exercise the full engine
//! pipeline: Jacobi diffusion, incremental agent movement, and multi-field
//! reward computation.
//!
//! # Pipeline order (each tick)
//!
//! 1. [`DiffusionPropagator`] — reads_previous(heat, velocity) → writes(heat, velocity, gradient)
//! 2. [`AgentMovementPropagator`] — ActionBuffer → writes(agent_presence)
//! 3. [`RewardPropagator`] — reads(heat, agent_presence, gradient) → writes(reward)

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

pub mod agent_movement;
pub mod diffusion;
pub mod fields;
pub mod reward;

pub use agent_movement::{ActionBuffer, AgentAction, AgentMovementPropagator, Direction};
pub use diffusion::DiffusionPropagator;
pub use fields::{reference_fields, AGENT_PRESENCE, HEAT, HEAT_GRADIENT, REWARD, VELOCITY};
pub use reward::RewardPropagator;
