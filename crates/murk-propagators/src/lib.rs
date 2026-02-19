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

pub mod agent_emission;
#[allow(deprecated)]
pub mod agent_movement;
#[allow(deprecated)]
pub mod diffusion;
pub mod fields;
pub mod flow_field;
pub mod gradient_compute;
pub mod identity_copy;
#[allow(deprecated)]
pub mod reward;
pub mod resource_field;
pub mod scalar_diffusion;

pub use agent_emission::{AgentEmission, EmissionMode};
pub use agent_movement::{ActionBuffer, AgentAction, AgentMovementPropagator, Direction};
pub use diffusion::DiffusionPropagator;
#[allow(deprecated)]
pub use fields::{reference_fields, AGENT_PRESENCE, HEAT, HEAT_GRADIENT, REWARD, VELOCITY};
pub use flow_field::FlowField;
pub use gradient_compute::GradientCompute;
pub use identity_copy::IdentityCopy;
pub use resource_field::{RegrowthModel, ResourceField};
pub use reward::RewardPropagator;
pub use scalar_diffusion::ScalarDiffusion;
