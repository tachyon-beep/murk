//! Murk: a world simulation engine for reinforcement learning and real-time applications.
//!
//! This is the top-level facade crate that re-exports the public API from all
//! Murk sub-crates. For most users, adding `murk` as a single dependency is
//! sufficient.
//!
//! # Quick start
//!
//! ```rust
//! use murk::prelude::*;
//! use murk::space::Square4;
//!
//! // A minimal propagator that fills a field with zeros.
//! struct ZeroFill;
//! impl Propagator for ZeroFill {
//!     fn name(&self) -> &str { "zero_fill" }
//!     fn reads(&self) -> murk::types::FieldSet { murk::types::FieldSet::empty() }
//!     fn writes(&self) -> Vec<(FieldId, WriteMode)> {
//!         vec![(FieldId(0), WriteMode::Full)]
//!     }
//!     fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
//!         ctx.writes().write(FieldId(0)).unwrap().fill(0.0);
//!         Ok(())
//!     }
//! }
//!
//! // Build a 16×16 grid world.
//! let space = Square4::new(16, 16, EdgeBehavior::Absorb).unwrap();
//! let fields = vec![FieldDef {
//!     name: "heat".into(),
//!     field_type: FieldType::Scalar,
//!     mutability: FieldMutability::PerTick,
//!     units: None,
//!     bounds: None,
//!     boundary_behavior: BoundaryBehavior::Clamp,
//! }];
//! let config = WorldConfig {
//!     space: Box::new(space),
//!     fields,
//!     propagators: vec![Box::new(ZeroFill)],
//!     dt: 0.1,
//!     seed: 42,
//!     ring_buffer_size: 8,
//!     max_ingress_queue: 64,
//!     tick_rate_hz: None,
//!     backoff: Default::default(),
//! };
//! let mut world = LockstepWorld::new(config).unwrap();
//! let result = world.step_sync(vec![]).unwrap();
//! assert_eq!(result.snapshot.tick_id(), murk::types::TickId(1));
//! ```
//!
//! # Modules
//!
//! Each module corresponds to a sub-crate. Use them for types not in the prelude:
//!
//! | Module | Sub-crate | Contents |
//! |--------|-----------|----------|
//! | [`arena`] | `murk-arena` | Arena storage, `Snapshot`, `OwnedSnapshot` |
//! | [`types`] | `murk-core` | IDs, field definitions, commands, core traits |
//! | [`space`] | `murk-space` | Spatial backends and region planning |
//! | [`propagator`] | `murk-propagator` | Propagator trait and pipeline validation |
//! | [`propagators`] | `murk-propagators` | Reference propagators (diffusion, agents, reward) |
//! | [`obs`] | `murk-obs` | Observation specification and tensor extraction |
//! | [`engine`] | `murk-engine` | Simulation engines (lockstep and realtime-async) |
//! | [`replay`] | `murk-replay` | Deterministic replay recording and verification |

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]

/// Arena, snapshot, and field storage (`murk-arena`).
///
/// Most users only need [`arena::Snapshot`] and [`arena::OwnedSnapshot`]
/// from this module — they are also available in the [`prelude`].
pub use murk_arena as arena;

/// Core types, traits, and IDs (`murk-core`).
///
/// Contains field definitions, commands, receipts, error types, and the
/// fundamental traits ([`types::FieldReader`], [`types::FieldWriter`],
/// [`types::SnapshotAccess`]).
pub use murk_core as types;

/// Spatial backends and region planning (`murk-space`).
///
/// Provides the [`space::Space`] trait and concrete backends:
/// [`space::Line1D`], [`space::Ring1D`], [`space::Square4`], [`space::Square8`],
/// [`space::Hex2D`], [`space::Fcc12`], and [`space::ProductSpace`].
pub use murk_space as space;

/// Propagator trait and pipeline validation (`murk-propagator`).
///
/// The [`propagator::Propagator`] trait is the main extension point for
/// user-defined simulation logic.
pub use murk_propagator as propagator;

/// Reference propagator implementations (`murk-propagators`).
///
/// Includes [`propagators::DiffusionPropagator`],
/// [`propagators::AgentMovementPropagator`], and
/// [`propagators::RewardPropagator`].
pub use murk_propagators as propagators;

/// Observation specification and tensor extraction (`murk-obs`).
///
/// Build [`obs::ObsSpec`] descriptions, compile them into [`obs::ObsPlan`]s,
/// and extract flat `f32` tensors with validity masks.
pub use murk_obs as obs;

/// Simulation engines (`murk-engine`).
///
/// [`engine::LockstepWorld`] for synchronous stepping (RL training loops),
/// [`engine::RealtimeAsyncWorld`] for autonomous background ticking.
pub use murk_engine as engine;

/// Deterministic replay recording and verification (`murk-replay`).
///
/// Record simulation runs with [`replay::ReplayWriter`], replay and verify
/// determinism with [`replay::ReplayReader`].
pub use murk_replay as replay;

/// Common imports for typical Murk usage.
///
/// ```rust
/// use murk::prelude::*;
/// ```
///
/// This imports the most frequently used types: world builders, core traits,
/// field definitions, commands, spatial types, and the propagator trait.
pub mod prelude {
    // Arena snapshots
    pub use murk_arena::{OwnedSnapshot, Snapshot};

    // Core types and traits
    pub use murk_core::{
        BoundaryBehavior, Command, CommandPayload, Coord, FieldDef, FieldId, FieldMutability,
        FieldReader, FieldType, FieldWriter, Receipt, SnapshotAccess,
    };

    // Errors
    pub use murk_core::{IngressError, ObsError, PropagatorError, StepError};

    // Space
    pub use murk_space::{EdgeBehavior, Space};

    // Propagator
    pub use murk_propagator::{Propagator, StepContext, WriteMode};

    // Observation
    pub use murk_obs::{ObsEntry, ObsPlan, ObsSpec};

    // Engine
    pub use murk_engine::{
        AsyncConfig, LockstepWorld, RealtimeAsyncWorld, StepMetrics, StepResult, WorldConfig,
    };
}
