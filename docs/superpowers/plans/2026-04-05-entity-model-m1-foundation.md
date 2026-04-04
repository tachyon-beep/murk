# Entity Model M1: Foundation (murk-core + murk-entity)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create the entity data model — EntityId with packed generation, EntityManifest, EntityStore with flat SoA properties, EntitySnapshot, PropertyStaging, and EntityOverlayReader — as the foundation for all subsequent entity milestones.

**Architecture:** Two crates modified/created. murk-core gains EntityId (20-bit slot + 12-bit generation), PropertyIndex, EntityManifest, and updated CommandPayload variants. murk-entity (new) provides the EntityStore (flat array + free list + coord_index), PropertyStaging (flat slab + u64 bitset), and EntityOverlayReader (two-lifetime Euler overlay). All entity aliveness is determined by a single property value, not a separate bool field.

**Tech Stack:** Rust, smallvec, no new external dependencies.

**Spec:** `docs/superpowers/specs/2026-04-05-entity-model-slot-observations-design.md` (sections 1 and 2)

**Plan series:** This is M1 of 6. M2 (engine), M3 (propagator), M4 (slot), M5 (FFI/Python), M6 (EntityProjection + replay) follow as separate plans.

---

## File Structure

### murk-core (modifications)

| File | Change | Responsibility |
|------|--------|---------------|
| `crates/murk-core/src/id.rs` | Modify | Add `EntityId` (packed slot+gen), `PropertyIndex` |
| `crates/murk-core/src/entity.rs` | Create | `EntityManifest` config type |
| `crates/murk-core/src/command.rs` | Modify | Update Move/Spawn/Despawn variants to use EntityId, PropertyIndex |
| `crates/murk-core/src/error.rs` | Modify | Add `UnknownEntity`, `EntityCapacityFull` to IngressError |
| `crates/murk-core/src/lib.rs` | Modify | Re-export new types |

### murk-entity (new crate)

| File | Change | Responsibility |
|------|--------|---------------|
| `crates/murk-entity/Cargo.toml` | Create | Crate manifest, depends on murk-core + smallvec |
| `crates/murk-entity/src/lib.rs` | Create | Module declarations and re-exports |
| `crates/murk-entity/src/record.rs` | Create | `EntityRecord` struct (no alive field) |
| `crates/murk-entity/src/store.rs` | Create | `EntityStore` with flat SoA properties, free list, coord_index |
| `crates/murk-entity/src/snapshot.rs` | Create | `EntitySnapshot` immutable borrow with generation-validated lookups |
| `crates/murk-entity/src/staging.rs` | Create | `PropertyStaging` with Vec<u64> bitset, bounds-checked set/get |
| `crates/murk-entity/src/overlay.rs` | Create | `EntityOverlayReader` with two lifetime parameters |

### Workspace

| File | Change | Responsibility |
|------|--------|---------------|
| `Cargo.toml` (workspace root) | Modify | Add `crates/murk-entity` to members |

---

## Task 1: EntityId with packed slot + generation

**Files:**
- Modify: `crates/murk-core/src/id.rs` (after line 155, the Coord type alias)

- [ ] **Step 1: Write the EntityId unit tests**

Add at the end of `crates/murk-core/src/id.rs`:

```rust
#[cfg(test)]
mod entity_id_tests {
    use super::*;

    #[test]
    fn new_packs_slot_and_generation() {
        let id = EntityId::new(42, 7);
        assert_eq!(id.slot(), 42);
        assert_eq!(id.generation(), 7);
    }

    #[test]
    fn max_slot_value() {
        // 20-bit slot: max is 0xFFFFF = 1_048_575
        let id = EntityId::new(1_048_575, 0);
        assert_eq!(id.slot(), 1_048_575);
        assert_eq!(id.generation(), 0);
    }

    #[test]
    fn max_generation_value() {
        // 12-bit generation: max is 0xFFF = 4095
        let id = EntityId::new(0, 4095);
        assert_eq!(id.slot(), 0);
        assert_eq!(id.generation(), 4095);
    }

    #[test]
    fn slot_and_generation_do_not_alias() {
        let id = EntityId::new(1, 1);
        assert_eq!(id.slot(), 1);
        assert_eq!(id.generation(), 1);
        // Verify the packed u32 is (1 << 20) | 1 = 1_048_577
        assert_eq!(id.0, (1 << 20) | 1);
    }

    #[test]
    #[should_panic(expected = "slot")]
    fn slot_overflow_panics_in_debug() {
        EntityId::new(1_048_576, 0); // 2^20 = overflow
    }

    #[test]
    #[should_panic(expected = "generation")]
    fn generation_overflow_panics_in_debug() {
        EntityId::new(0, 4096); // 2^12 = overflow
    }

    #[test]
    fn display_shows_slot_and_generation() {
        let id = EntityId::new(5, 3);
        let s = format!("{id}");
        assert!(s.contains('5'), "Display should include slot: {s}");
        assert!(s.contains('3'), "Display should include generation: {s}");
    }

    #[test]
    fn equality_requires_both_slot_and_generation() {
        let a = EntityId::new(1, 0);
        let b = EntityId::new(1, 1);
        assert_ne!(a, b, "Different generations must not be equal");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p murk-core entity_id_tests -- --nocapture 2>&1 | head -30`
Expected: Compilation error — `EntityId` not defined.

- [ ] **Step 3: Implement EntityId and PropertyIndex**

Add to `crates/murk-core/src/id.rs` before the `Coord` type alias (before line 150):

```rust
/// Maximum slot index for [`EntityId`] (20-bit: 0..1_048_575).
const ENTITY_SLOT_BITS: u32 = 20;
/// Bitmask for extracting the slot from a packed [`EntityId`].
const ENTITY_SLOT_MASK: u32 = (1 << ENTITY_SLOT_BITS) - 1;
/// Maximum generation for [`EntityId`] (12-bit: 0..4_095).
const ENTITY_GEN_MAX: u32 = (1 << (32 - ENTITY_SLOT_BITS)) - 1;

/// Identifies an entity within a simulation world.
///
/// Packs a 20-bit slot index and 12-bit generation counter into a `u32`.
/// The generation prevents stale-ID reuse: after despawn and slot recycling,
/// a stale `EntityId` will fail generation validation on lookup.
///
/// - 20-bit slot -> max 1,048,575 concurrent entities
/// - 12-bit generation -> 4,096 generations per slot before wrap
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(u32);

impl EntityId {
    /// Create a new EntityId from a slot index and generation counter.
    ///
    /// # Panics
    ///
    /// Panics if `slot > 1_048_575` (2^20 - 1) or `generation > 4_095` (2^12 - 1).
    #[must_use]
    pub fn new(slot: u32, generation: u32) -> Self {
        assert!(
            slot <= ENTITY_SLOT_MASK,
            "slot {slot} exceeds maximum {ENTITY_SLOT_MASK}"
        );
        assert!(
            generation <= ENTITY_GEN_MAX,
            "generation {generation} exceeds maximum {ENTITY_GEN_MAX}"
        );
        Self((generation << ENTITY_SLOT_BITS) | slot)
    }

    /// The slot index (0..1_048_575). Used to index into entity store arrays.
    #[inline]
    #[must_use]
    pub fn slot(&self) -> u32 {
        self.0 & ENTITY_SLOT_MASK
    }

    /// The generation counter (0..4_095). Validated against the store on lookup.
    #[inline]
    #[must_use]
    pub fn generation(&self) -> u32 {
        self.0 >> ENTITY_SLOT_BITS
    }

    /// The raw packed u32 value. Useful for serialization.
    #[inline]
    #[must_use]
    pub fn as_u32(&self) -> u32 {
        self.0
    }

    /// Reconstruct from a raw packed u32 value. No validation.
    #[inline]
    #[must_use]
    pub fn from_u32(raw: u32) -> Self {
        Self(raw)
    }
}

impl fmt::Display for EntityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "entity(slot={}, gen={})", self.slot(), self.generation())
    }
}

/// Indexes into an entity's property array.
///
/// Property layout is defined by [`EntityManifest`](crate::EntityManifest)
/// at world creation. All entities in a world share the same property layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PropertyIndex(pub u32);

impl fmt::Display for PropertyIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for PropertyIndex {
    fn from(v: u32) -> Self {
        Self(v)
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p murk-core entity_id_tests -- --nocapture`
Expected: All 8 tests pass.

- [ ] **Step 5: Commit**

```
git add crates/murk-core/src/id.rs
git commit -m "feat(core): add EntityId with packed slot+generation and PropertyIndex"
```

---

## Task 2: EntityManifest config type

**Files:**
- Create: `crates/murk-core/src/entity.rs`
- Modify: `crates/murk-core/src/lib.rs`

- [ ] **Step 1: Create entity.rs with EntityManifest and tests**

Create `crates/murk-core/src/entity.rs`:

```rust
//! Entity manifest — property schema for entities in a world.

use crate::id::PropertyIndex;

/// Declares the property schema for entities in a world.
///
/// All entities share the same property layout (homogeneous).
/// Property values default to `property_defaults[i]` on spawn
/// unless overridden by the Spawn command.
///
/// One property must be designated as the ALIVE indicator
/// (conventionally `PropertyIndex(0)`). The engine stamps `1.0` on spawn,
/// `0.0` on despawn. Propagators write `0.0` for death. `iter_alive()`
/// reads this property to determine liveness.
///
/// # Examples
///
/// ```
/// use murk_core::{EntityManifest, PropertyIndex};
///
/// let manifest = EntityManifest {
///     property_names: vec!["alive".into(), "hp".into(), "x".into()],
///     property_defaults: vec![1.0, 100.0, 0.0],
///     alive_property: PropertyIndex(0),
/// };
///
/// assert_eq!(manifest.property_count(), 3);
/// assert_eq!(manifest.property_defaults[0], 1.0);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct EntityManifest {
    /// Human-readable property names (for debugging, rendering, logging).
    pub property_names: Vec<String>,
    /// Default values for each property on spawn.
    pub property_defaults: Vec<f32>,
    /// Which property index represents the ALIVE flag.
    /// `iter_alive()` filters on `properties[alive_property] > 0.0`.
    pub alive_property: PropertyIndex,
}

impl EntityManifest {
    /// Number of properties per entity.
    #[must_use]
    pub fn property_count(&self) -> usize {
        self.property_defaults.len()
    }

    /// Validate the manifest for internal consistency.
    ///
    /// Returns `Ok(())` if valid, or a description of the problem.
    pub fn validate(&self) -> Result<(), String> {
        if self.property_names.len() != self.property_defaults.len() {
            return Err(format!(
                "property_names length ({}) != property_defaults length ({})",
                self.property_names.len(),
                self.property_defaults.len()
            ));
        }
        if self.property_defaults.is_empty() {
            return Err("EntityManifest must have at least one property (the alive property)".into());
        }
        let alive = self.alive_property.0 as usize;
        if alive >= self.property_defaults.len() {
            return Err(format!(
                "alive_property index ({alive}) >= property count ({})",
                self.property_defaults.len()
            ));
        }
        if self.property_defaults.len() > 256 {
            return Err(format!(
                "property count ({}) exceeds maximum (256)",
                self.property_defaults.len()
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manifest() -> EntityManifest {
        EntityManifest {
            property_names: vec!["alive".into(), "hp".into(), "x".into()],
            property_defaults: vec![1.0, 100.0, 0.0],
            alive_property: PropertyIndex(0),
        }
    }

    #[test]
    fn property_count_matches_defaults() {
        let m = test_manifest();
        assert_eq!(m.property_count(), 3);
    }

    #[test]
    fn validate_accepts_valid_manifest() {
        assert!(test_manifest().validate().is_ok());
    }

    #[test]
    fn validate_rejects_length_mismatch() {
        let m = EntityManifest {
            property_names: vec!["a".into()],
            property_defaults: vec![1.0, 2.0],
            alive_property: PropertyIndex(0),
        };
        assert!(m.validate().is_err());
    }

    #[test]
    fn validate_rejects_empty() {
        let m = EntityManifest {
            property_names: vec![],
            property_defaults: vec![],
            alive_property: PropertyIndex(0),
        };
        assert!(m.validate().is_err());
    }

    #[test]
    fn validate_rejects_alive_out_of_range() {
        let m = EntityManifest {
            property_names: vec!["a".into()],
            property_defaults: vec![1.0],
            alive_property: PropertyIndex(5),
        };
        assert!(m.validate().is_err());
    }
}
```

- [ ] **Step 2: Add module to lib.rs and re-exports**

In `crates/murk-core/src/lib.rs`, add the module declaration and re-exports:

After line 14 (`pub mod traits;`), add:
```rust
pub mod entity;
```

After line 24 (the `pub use id::` block), add:
```rust
pub use entity::EntityManifest;
pub use id::{EntityId, PropertyIndex};
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p murk-core -- --nocapture`
Expected: All tests pass, including the new EntityManifest tests.

- [ ] **Step 4: Commit**

```
git add crates/murk-core/src/entity.rs crates/murk-core/src/lib.rs
git commit -m "feat(core): add EntityManifest config type with validation"
```

---

## Task 3: Update CommandPayload and IngressError

**Files:**
- Modify: `crates/murk-core/src/command.rs` (lines 76-130 for CommandPayload, line 4 for imports)
- Modify: `crates/murk-core/src/error.rs` (lines 116-136 for IngressError)

- [ ] **Step 1: Update command.rs imports**

At the top of `crates/murk-core/src/command.rs` (line 4), change:

```rust
use crate::id::{Coord, FieldId, ParameterKey, TickId};
```

to:

```rust
use crate::id::{Coord, EntityId, FieldId, ParameterKey, PropertyIndex, TickId};
```

- [ ] **Step 2: Update the Move variant**

In `crates/murk-core/src/command.rs`, replace the Move variant (lines 79-87):

```rust
    /// Move an entity to a target coordinate.
    ///
    /// Rejected if `entity_id` is unknown (generation mismatch or never allocated),
    /// the entity is dead, or `target_coord` is out of bounds.
    Move {
        /// The entity to move (includes generation for stale-ID detection).
        entity_id: EntityId,
        /// The destination coordinate.
        target_coord: Coord,
    },
```

- [ ] **Step 3: Update the Spawn variant**

Replace the Spawn variant (lines 88-94):

```rust
    /// Spawn a new entity at a coordinate with initial property values.
    Spawn {
        /// The spawn location.
        coord: Coord,
        /// Entity type tag for classification (e.g., mech=0, projectile=1).
        entity_type: u32,
        /// Property overrides applied on top of EntityManifest defaults.
        /// Only non-default values need to be specified.
        property_overrides: Vec<(PropertyIndex, f32)>,
    },
```

- [ ] **Step 4: Update the Despawn variant**

Replace the Despawn variant (lines 95-99):

```rust
    /// Remove an entity. Slot is recycled with incremented generation.
    Despawn {
        /// The entity to remove (includes generation for stale-ID detection).
        entity_id: EntityId,
    },
```

- [ ] **Step 5: Update the doc examples in command.rs**

The existing doc examples on `Command` (lines 14-28) and `CommandPayload` (lines 56-75) reference `SetParameter` and `SetField` which are unchanged. Verify they still compile. If any example references `field_values` on Spawn or `u64` on Move/Despawn, update it.

- [ ] **Step 6: Add error variants to IngressError**

In `crates/murk-core/src/error.rs`, add two new variants to `IngressError` (after `NotApplied` at line 135):

```rust
    /// The referenced entity does not exist, has been despawned,
    /// or the EntityId generation does not match the current slot occupant.
    UnknownEntity,
    /// Entity capacity is full — cannot spawn.
    EntityCapacityFull,
```

And add their `Display` arms in the `impl fmt::Display for IngressError` block (after the `NotApplied` arm):

```rust
            Self::UnknownEntity => write!(f, "unknown entity (stale or invalid EntityId)"),
            Self::EntityCapacityFull => write!(f, "entity capacity full"),
```

- [ ] **Step 7: Update Receipt with spawned_entity_id**

In `crates/murk-core/src/command.rs`, add to the `Receipt` struct (after `command_index` at line 162):

```rust
    /// EntityId allocated by a Spawn command, if applicable.
    /// Includes the generation — callers store this for future commands.
    pub spawned_entity_id: Option<EntityId>,
```

Update the Receipt doc example to include `spawned_entity_id: None`.

- [ ] **Step 8: Fix compilation across workspace**

Run: `cargo check --workspace 2>&1 | head -50`

The CommandPayload changes will break code in murk-engine (tick.rs), murk-replay (codec.rs), and murk-ffi that pattern-matches on Move/Spawn/Despawn. For now, update the pattern matches to use the new field names **without changing behavior** — they still return `UnsupportedCommand`. The Receipt struct change will also need `spawned_entity_id: None` added to all construction sites.

In `crates/murk-engine/src/tick.rs`, the match arm at lines 372-381 needs field names updated:
```rust
CommandPayload::SetParameter { .. }
| CommandPayload::SetParameterBatch { .. }
| CommandPayload::Move { .. }
| CommandPayload::Spawn { .. }
| CommandPayload::Despawn { .. }
| CommandPayload::Custom { .. } => {
    receipt.accepted = false;
    receipt.reason_code = Some(IngressError::UnsupportedCommand);
}
```
This arm uses `{ .. }` so it should compile as-is. But check all Receipt construction sites for the new `spawned_entity_id` field.

Search for all `Receipt {` constructions and add `spawned_entity_id: None` to each.

Run: `cargo check --workspace`
Expected: Clean compilation.

- [ ] **Step 9: Run all tests**

Run: `cargo test --workspace 2>&1 | tail -20`
Expected: All existing tests pass. No new test failures.

- [ ] **Step 10: Commit**

```
git add crates/murk-core/src/command.rs crates/murk-core/src/error.rs
git add -u  # catch any files modified for Receipt
git commit -m "feat(core): update CommandPayload for entity model (BREAKING)

Move/Despawn use EntityId (was u64). Spawn uses property_overrides
(was field_values) and adds entity_type. Receipt gains spawned_entity_id.
IngressError gains UnknownEntity and EntityCapacityFull variants."
```

---

## Task 4: Create murk-entity crate skeleton

**Files:**
- Create: `crates/murk-entity/Cargo.toml`
- Create: `crates/murk-entity/src/lib.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Create the crate directory and Cargo.toml**

Create `crates/murk-entity/Cargo.toml`:

```toml
[package]
name = "murk-entity"
version.workspace = true
edition.workspace = true
license.workspace = true
repository.workspace = true
authors.workspace = true
homepage.workspace = true
documentation.workspace = true
keywords.workspace = true
categories.workspace = true
rust-version.workspace = true
description = "Entity data model for the Murk simulation framework"
readme = "README.md"

[dependencies]
murk-core = { path = "../murk-core" }
smallvec = { workspace = true }

[dev-dependencies]
proptest = { workspace = true }
```

- [ ] **Step 2: Create lib.rs**

Create `crates/murk-entity/src/lib.rs`:

```rust
//! Entity data model for the Murk simulation framework.
//!
//! Provides identity-carrying entities with typed properties, lifecycle
//! management (spawn/despawn), and observation-time snapshot access.
//! This crate has no engine or observation dependency — it is consumed
//! independently by `murk-engine`, `murk-propagator`, and `murk-slot`.

#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![forbid(unsafe_code)]

pub mod overlay;
pub mod record;
pub mod snapshot;
pub mod staging;
pub mod store;

pub use overlay::EntityOverlayReader;
pub use record::EntityRecord;
pub use snapshot::EntitySnapshot;
pub use staging::PropertyStaging;
pub use store::EntityStore;
```

- [ ] **Step 3: Create placeholder modules**

Create each module file with a placeholder doc comment so the crate compiles:

`crates/murk-entity/src/record.rs`:
```rust
//! Entity record — structural data for a single entity.
```

`crates/murk-entity/src/store.rs`:
```rust
//! Entity store — fixed-capacity entity storage with free-list recycling.
```

`crates/murk-entity/src/snapshot.rs`:
```rust
//! Entity snapshot — immutable borrow for observation and propagator reads.
```

`crates/murk-entity/src/staging.rs`:
```rust
//! Property staging — write buffer for propagator entity property mutations.
```

`crates/murk-entity/src/overlay.rs`:
```rust
//! Entity overlay reader — Euler-style reads with staging fallback.
```

- [ ] **Step 4: Add to workspace**

In the root `Cargo.toml`, add `"crates/murk-entity"` to the `members` list (after `"crates/murk-core"`).

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p murk-entity`
Expected: Clean compilation (empty modules are valid).

- [ ] **Step 6: Commit**

```
git add crates/murk-entity/ Cargo.toml
git commit -m "feat(entity): create murk-entity crate skeleton"
```

---

## Task 5: EntityRecord

**Files:**
- Modify: `crates/murk-entity/src/record.rs`

- [ ] **Step 1: Write EntityRecord tests**

Replace the contents of `crates/murk-entity/src/record.rs`:

```rust
//! Entity record — structural data for a single entity.

use murk_core::id::{Coord, EntityId};

/// Structural data for a single entity in the store.
///
/// Properties (including aliveness) are stored in a separate flat slab
/// owned by [`EntityStore`](crate::EntityStore), not in this struct.
/// This keeps EntityRecord small and cache-friendly for iteration.
///
/// # Examples
///
/// ```
/// use murk_core::EntityId;
/// use murk_entity::EntityRecord;
///
/// let record = EntityRecord {
///     id: EntityId::new(0, 0),
///     coord: vec![5, 10].into(),
///     entity_type: 0,
/// };
///
/// assert_eq!(record.id.slot(), 0);
/// assert_eq!(record.coord[0], 5);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct EntityRecord {
    /// Unique identifier (includes generation for stale-ID detection).
    pub id: EntityId,
    /// Current position in simulation space.
    pub coord: Coord,
    /// Type tag for classification (e.g., mech=0, projectile=1).
    pub entity_type: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_stores_id_coord_type() {
        let r = EntityRecord {
            id: EntityId::new(3, 1),
            coord: vec![7, 8].into(),
            entity_type: 2,
        };
        assert_eq!(r.id.slot(), 3);
        assert_eq!(r.id.generation(), 1);
        assert_eq!(r.coord.as_slice(), &[7, 8]);
        assert_eq!(r.entity_type, 2);
    }

    #[test]
    fn record_is_clone() {
        let r = EntityRecord {
            id: EntityId::new(0, 0),
            coord: vec![0].into(),
            entity_type: 0,
        };
        let r2 = r.clone();
        assert_eq!(r, r2);
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p murk-entity -- --nocapture`
Expected: 2 tests pass.

- [ ] **Step 3: Commit**

```
git add crates/murk-entity/src/record.rs
git commit -m "feat(entity): add EntityRecord struct"
```

---

## Task 6: EntityStore — core data structure

**Files:**
- Modify: `crates/murk-entity/src/store.rs`

This is the largest task. The EntityStore owns the flat property slab, generation tracking, free list, and coord_index.

- [ ] **Step 1: Write EntityStore tests first**

Replace the contents of `crates/murk-entity/src/store.rs` with the full implementation + tests. Due to the interdependency between store, snapshot, and staging, we implement the store first with inline tests, then extract snapshot/staging in subsequent tasks.

```rust
//! Entity store — fixed-capacity entity storage with free-list recycling.

use std::collections::HashMap;

use murk_core::id::{Coord, EntityId, PropertyIndex};
use murk_core::entity::EntityManifest;
use murk_core::error::IngressError;
use smallvec::SmallVec;

use crate::record::EntityRecord;

/// Coordinate key for HashMap lookup. Wraps a Coord for Hash.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CoordKey(Coord);

/// Fixed-capacity entity storage with generation tracking and free-list recycling.
///
/// Properties are stored as a flat SoA slab (`properties: Vec<f32>`) indexed
/// by `slot * property_count + prop`. This matches the layout used by
/// [`PropertyStaging`](crate::PropertyStaging) and is cache-friendly for
/// iteration.
///
/// # Examples
///
/// ```
/// use murk_core::{EntityManifest, PropertyIndex};
/// use murk_entity::EntityStore;
///
/// let manifest = EntityManifest {
///     property_names: vec!["alive".into(), "hp".into()],
///     property_defaults: vec![1.0, 100.0],
///     alive_property: PropertyIndex(0),
/// };
/// let mut store = EntityStore::new(8, manifest);
///
/// let id = store.spawn(vec![0, 0].into(), 0, &[]).unwrap();
/// assert_eq!(store.alive_count(), 1);
/// ```
pub struct EntityStore {
    /// Structural records, indexed by slot index (EntityId.slot()).
    records: Vec<EntityRecord>,
    /// Properties as flat SoA slab: [capacity * property_count].
    properties: Vec<f32>,
    /// Generation counter per slot.
    generations: Vec<u32>,
    /// Recycled slot indices available for spawn.
    free_list: Vec<u32>,
    /// Reverse lookup: coordinate -> entities at that cell.
    coord_index: HashMap<CoordKey, SmallVec<[EntityId; 4]>>,
    /// High-water mark for slot allocation.
    next_slot: u32,
    /// Maximum capacity.
    capacity: u32,
    /// Property schema.
    manifest: EntityManifest,
}

impl EntityStore {
    /// Create a new entity store with the given capacity and property schema.
    ///
    /// # Panics
    ///
    /// Panics if `capacity == 0` or the manifest fails validation.
    pub fn new(capacity: u32, manifest: EntityManifest) -> Self {
        assert!(capacity > 0, "EntityStore capacity must be > 0");
        manifest.validate().expect("EntityManifest validation failed");

        let prop_count = manifest.property_count();
        let total_props = capacity as usize * prop_count;

        // Pre-allocate records with dummy values (unused slots).
        let records: Vec<EntityRecord> = (0..capacity)
            .map(|i| EntityRecord {
                id: EntityId::new(i, 0),
                coord: SmallVec::new(),
                entity_type: 0,
            })
            .collect();

        Self {
            records,
            properties: vec![0.0; total_props],
            generations: vec![0; capacity as usize],
            free_list: Vec::new(),
            coord_index: HashMap::new(),
            next_slot: 0,
            capacity,
            manifest,
        }
    }

    /// Spawn a new entity. Returns the allocated EntityId or an error.
    pub fn spawn(
        &mut self,
        coord: Coord,
        entity_type: u32,
        property_overrides: &[(PropertyIndex, f32)],
    ) -> Result<EntityId, IngressError> {
        let slot = if let Some(recycled) = self.free_list.pop() {
            recycled
        } else if self.next_slot < self.capacity {
            let s = self.next_slot;
            self.next_slot += 1;
            s
        } else {
            return Err(IngressError::EntityCapacityFull);
        };

        let gen = self.generations[slot as usize];
        let id = EntityId::new(slot, gen);

        // Write record.
        self.records[slot as usize] = EntityRecord {
            id,
            coord: coord.clone(),
            entity_type,
        };

        // Write default properties.
        let prop_count = self.manifest.property_count();
        let base = slot as usize * prop_count;
        self.properties[base..base + prop_count]
            .copy_from_slice(&self.manifest.property_defaults);

        // Apply overrides.
        for &(prop, value) in property_overrides {
            let idx = prop.0 as usize;
            if idx < prop_count {
                self.properties[base + idx] = value;
            }
        }

        // Stamp alive.
        let alive_idx = self.manifest.alive_property.0 as usize;
        self.properties[base + alive_idx] = 1.0;

        // Update coord_index.
        self.coord_index
            .entry(CoordKey(coord))
            .or_default()
            .push(id);

        Ok(id)
    }

    /// Despawn an entity. Returns Ok or UnknownEntity.
    pub fn despawn(&mut self, id: EntityId) -> Result<(), IngressError> {
        let slot = id.slot() as usize;
        if slot >= self.capacity as usize {
            return Err(IngressError::UnknownEntity);
        }
        if self.generations[slot] != id.generation() {
            return Err(IngressError::UnknownEntity);
        }
        if !self.is_alive_at_slot(slot) {
            return Err(IngressError::UnknownEntity);
        }

        // Mark dead via property.
        let prop_count = self.manifest.property_count();
        let alive_idx = self.manifest.alive_property.0 as usize;
        self.properties[slot * prop_count + alive_idx] = 0.0;

        // Increment generation, push to free list.
        self.generations[slot] = self.generations[slot].wrapping_add(1) & 0xFFF;
        self.free_list.push(id.slot());

        // Remove from coord_index.
        let coord_key = CoordKey(self.records[slot].coord.clone());
        if let Some(ids) = self.coord_index.get_mut(&coord_key) {
            ids.retain(|eid| eid.slot() != id.slot());
            if ids.is_empty() {
                self.coord_index.remove(&coord_key);
            }
        }

        Ok(())
    }

    /// Move an entity to a new coordinate. Returns Ok, UnknownEntity, or NotApplied.
    pub fn move_entity(
        &mut self,
        id: EntityId,
        target_coord: Coord,
    ) -> Result<(), IngressError> {
        let slot = id.slot() as usize;
        if slot >= self.capacity as usize {
            return Err(IngressError::UnknownEntity);
        }
        if self.generations[slot] != id.generation() {
            return Err(IngressError::UnknownEntity);
        }
        if !self.is_alive_at_slot(slot) {
            return Err(IngressError::UnknownEntity);
        }

        let old_coord = self.records[slot].coord.clone();
        let old_key = CoordKey(old_coord);
        let new_key = CoordKey(target_coord.clone());

        // Update record.
        self.records[slot].coord = target_coord;

        // Update coord_index: remove from old, add to new.
        // Need to update the EntityId in coord_index to current generation.
        let current_id = EntityId::new(id.slot(), self.generations[slot]);
        if let Some(ids) = self.coord_index.get_mut(&old_key) {
            ids.retain(|eid| eid.slot() != id.slot());
            if ids.is_empty() {
                self.coord_index.remove(&old_key);
            }
        }
        self.coord_index
            .entry(new_key)
            .or_default()
            .push(current_id);

        Ok(())
    }

    /// Read a property value. Returns None if slot/generation mismatch or property out of range.
    pub fn property(&self, id: EntityId, prop: PropertyIndex) -> Option<f32> {
        let slot = id.slot() as usize;
        if slot >= self.capacity as usize {
            return None;
        }
        if self.generations[slot] != id.generation() {
            return None;
        }
        let idx = prop.0 as usize;
        let prop_count = self.manifest.property_count();
        if idx >= prop_count {
            return None;
        }
        Some(self.properties[slot * prop_count + idx])
    }

    /// Write a property value. Returns false if out of bounds.
    pub fn set_property(&mut self, id: EntityId, prop: PropertyIndex, value: f32) -> bool {
        let slot = id.slot() as usize;
        if slot >= self.capacity as usize {
            return false;
        }
        if self.generations[slot] != id.generation() {
            return false;
        }
        let idx = prop.0 as usize;
        let prop_count = self.manifest.property_count();
        if idx >= prop_count {
            return false;
        }
        self.properties[slot * prop_count + idx] = value;
        true
    }

    /// Check if an entity at a given slot is alive.
    fn is_alive_at_slot(&self, slot: usize) -> bool {
        let prop_count = self.manifest.property_count();
        let alive_idx = self.manifest.alive_property.0 as usize;
        self.properties[slot * prop_count + alive_idx] > 0.0
    }

    /// Number of currently alive entities.
    pub fn alive_count(&self) -> u32 {
        (0..self.next_slot as usize)
            .filter(|&slot| self.is_alive_at_slot(slot))
            .count() as u32
    }

    /// Iterate all allocated records (alive and dead).
    pub fn iter_all(&self) -> impl Iterator<Item = &EntityRecord> {
        self.records[..self.next_slot as usize].iter()
    }

    /// Iterate alive records only.
    pub fn iter_alive(&self) -> impl Iterator<Item = &EntityRecord> + '_ {
        self.records[..self.next_slot as usize]
            .iter()
            .filter(|r| {
                let slot = r.id.slot() as usize;
                self.is_alive_at_slot(slot)
            })
    }

    /// Get an entity record by ID. Returns None on generation mismatch.
    pub fn get(&self, id: EntityId) -> Option<&EntityRecord> {
        let slot = id.slot() as usize;
        if slot >= self.next_slot as usize {
            return None;
        }
        if self.generations[slot] != id.generation() {
            return None;
        }
        Some(&self.records[slot])
    }

    /// Look up entities at a coordinate.
    pub fn at_coord(&self, coord: &Coord) -> &[EntityId] {
        self.coord_index
            .get(&CoordKey(coord.clone()))
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// The entity manifest.
    pub fn manifest(&self) -> &EntityManifest {
        &self.manifest
    }

    /// Maximum entity capacity.
    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Current high-water mark for slot allocation.
    pub fn next_slot(&self) -> u32 {
        self.next_slot
    }

    /// Read-only access to the property slab.
    pub fn properties(&self) -> &[f32] {
        &self.properties
    }

    /// Read-only access to generation counters.
    pub fn generations(&self) -> &[u32] {
        &self.generations
    }

    /// Create a lightweight snapshot of the store state for rollback.
    /// Returns all data needed to restore the store to this point.
    pub fn snapshot_for_rollback(&self) -> EntityStoreSnapshot {
        EntityStoreSnapshot {
            records: self.records[..self.next_slot as usize].to_vec(),
            properties: self.properties.clone(),
            generations: self.generations.clone(),
            free_list: self.free_list.clone(),
            coord_index: self.coord_index.clone(),
            next_slot: self.next_slot,
        }
    }

    /// Restore the store from a rollback snapshot.
    pub fn restore_from_snapshot(&mut self, snap: EntityStoreSnapshot) {
        self.records[..snap.next_slot as usize].clone_from_slice(&snap.records);
        self.properties = snap.properties;
        self.generations = snap.generations;
        self.free_list = snap.free_list;
        self.coord_index = snap.coord_index;
        self.next_slot = snap.next_slot;
    }
}

/// Lightweight snapshot of EntityStore state for rollback.
#[derive(Clone)]
pub struct EntityStoreSnapshot {
    records: Vec<EntityRecord>,
    properties: Vec<f32>,
    generations: Vec<u32>,
    free_list: Vec<u32>,
    coord_index: HashMap<CoordKey, SmallVec<[EntityId; 4]>>,
    next_slot: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_manifest() -> EntityManifest {
        EntityManifest {
            property_names: vec![
                "alive".into(), "hp".into(), "x".into(), "y".into(),
            ],
            property_defaults: vec![1.0, 100.0, 0.0, 0.0],
            alive_property: PropertyIndex(0),
        }
    }

    fn test_store() -> EntityStore {
        EntityStore::new(8, test_manifest())
    }

    #[test]
    fn spawn_returns_unique_ids() {
        let mut store = test_store();
        let id0 = store.spawn(vec![0, 0].into(), 0, &[]).unwrap();
        let id1 = store.spawn(vec![1, 0].into(), 0, &[]).unwrap();
        assert_ne!(id0, id1);
        assert_eq!(id0.slot(), 0);
        assert_eq!(id1.slot(), 1);
        assert_eq!(id0.generation(), 0);
    }

    #[test]
    fn spawn_at_capacity_returns_error() {
        let mut store = EntityStore::new(2, test_manifest());
        store.spawn(vec![0].into(), 0, &[]).unwrap();
        store.spawn(vec![1].into(), 0, &[]).unwrap();
        let err = store.spawn(vec![2].into(), 0, &[]).unwrap_err();
        assert_eq!(err, IngressError::EntityCapacityFull);
    }

    #[test]
    fn spawn_applies_defaults_and_overrides() {
        let mut store = test_store();
        let id = store.spawn(
            vec![0, 0].into(),
            0,
            &[(PropertyIndex(1), 50.0)], // override HP from 100 to 50
        ).unwrap();
        // Alive stamped to 1.0
        assert_eq!(store.property(id, PropertyIndex(0)), Some(1.0));
        // HP overridden
        assert_eq!(store.property(id, PropertyIndex(1)), Some(50.0));
        // x/y default to 0.0
        assert_eq!(store.property(id, PropertyIndex(2)), Some(0.0));
    }

    #[test]
    fn despawn_recycles_slot_with_incremented_generation() {
        let mut store = test_store();
        let id0 = store.spawn(vec![0].into(), 0, &[]).unwrap();
        assert_eq!(id0.generation(), 0);

        store.despawn(id0).unwrap();
        assert_eq!(store.alive_count(), 0);

        // Spawn into recycled slot.
        let id1 = store.spawn(vec![1].into(), 0, &[]).unwrap();
        assert_eq!(id1.slot(), 0, "should reuse slot 0");
        assert_eq!(id1.generation(), 1, "generation should increment");
    }

    #[test]
    fn stale_id_returns_unknown_entity() {
        let mut store = test_store();
        let old_id = store.spawn(vec![0].into(), 0, &[]).unwrap();
        store.despawn(old_id).unwrap();

        // Spawn new entity in same slot.
        let _new_id = store.spawn(vec![1].into(), 0, &[]).unwrap();

        // Old ID should fail.
        assert_eq!(store.despawn(old_id), Err(IngressError::UnknownEntity));
        assert_eq!(store.move_entity(old_id, vec![2].into()), Err(IngressError::UnknownEntity));
        assert_eq!(store.property(old_id, PropertyIndex(0)), None);
        assert_eq!(store.get(old_id), None);
    }

    #[test]
    fn kill_via_property_write() {
        let mut store = test_store();
        let id = store.spawn(vec![0].into(), 0, &[]).unwrap();
        assert_eq!(store.alive_count(), 1);

        // Kill by writing alive property to 0.
        store.set_property(id, PropertyIndex(0), 0.0);
        assert_eq!(store.alive_count(), 0);

        // iter_alive skips, iter_all includes.
        assert_eq!(store.iter_alive().count(), 0);
        assert_eq!(store.iter_all().count(), 1);
    }

    #[test]
    fn move_updates_coord_index() {
        let mut store = test_store();
        let id = store.spawn(vec![0, 0].into(), 0, &[]).unwrap();

        assert_eq!(store.at_coord(&vec![0, 0].into()).len(), 1);
        assert_eq!(store.at_coord(&vec![5, 5].into()).len(), 0);

        store.move_entity(id, vec![5, 5].into()).unwrap();

        assert_eq!(store.at_coord(&vec![0, 0].into()).len(), 0);
        assert_eq!(store.at_coord(&vec![5, 5].into()).len(), 1);
        assert_eq!(store.get(id).unwrap().coord.as_slice(), &[5, 5]);
    }

    #[test]
    fn move_dead_entity_returns_unknown() {
        let mut store = test_store();
        let id = store.spawn(vec![0].into(), 0, &[]).unwrap();
        store.set_property(id, PropertyIndex(0), 0.0); // kill
        let err = store.move_entity(id, vec![1].into()).unwrap_err();
        assert_eq!(err, IngressError::UnknownEntity);
    }

    #[test]
    fn despawn_all_then_respawn_recycles() {
        let mut store = EntityStore::new(4, test_manifest());
        let ids: Vec<_> = (0..4)
            .map(|i| store.spawn(vec![i].into(), 0, &[]).unwrap())
            .collect();

        for id in &ids {
            store.despawn(*id).unwrap();
        }
        assert_eq!(store.alive_count(), 0);

        // Re-spawn should reuse slots with incremented generations.
        let new_ids: Vec<_> = (0..4)
            .map(|i| store.spawn(vec![i + 10].into(), 0, &[]).unwrap())
            .collect();
        for new_id in &new_ids {
            assert_eq!(new_id.generation(), 1);
        }
    }

    #[test]
    fn property_out_of_range_returns_none() {
        let mut store = test_store();
        let id = store.spawn(vec![0].into(), 0, &[]).unwrap();
        // PropertyIndex(99) is way beyond the 4-property manifest.
        assert_eq!(store.property(id, PropertyIndex(99)), None);
        assert!(!store.set_property(id, PropertyIndex(99), 1.0));
    }

    #[test]
    fn snapshot_and_restore_rollback() {
        let mut store = test_store();
        let id = store.spawn(vec![0].into(), 0, &[]).unwrap();
        let snap = store.snapshot_for_rollback();

        // Mutate after snapshot.
        store.spawn(vec![1].into(), 1, &[]).unwrap();
        store.despawn(id).unwrap();
        assert_eq!(store.alive_count(), 1);

        // Restore.
        store.restore_from_snapshot(snap);
        assert_eq!(store.alive_count(), 1);
        assert!(store.get(id).is_some(), "original entity restored");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p murk-entity store::tests -- --nocapture`
Expected: All 12 tests pass.

- [ ] **Step 3: Commit**

```
git add crates/murk-entity/src/store.rs
git commit -m "feat(entity): add EntityStore with flat SoA properties and generation tracking"
```

---

## Task 7: EntitySnapshot

**Files:**
- Modify: `crates/murk-entity/src/snapshot.rs`

- [ ] **Step 1: Add snapshot() method to EntityStore**

In `crates/murk-entity/src/store.rs`, add this method to `impl EntityStore`:

```rust
    /// Create an immutable snapshot of the current store state.
    pub fn snapshot(&self) -> crate::snapshot::EntitySnapshot<'_> {
        crate::snapshot::EntitySnapshot::new(
            &self.records[..self.next_slot as usize],
            &self.properties,
            &self.generations,
            &self.manifest,
            self.next_slot,
        )
    }
```

- [ ] **Step 2: Write EntitySnapshot with tests using store.snapshot()**

Replace `crates/murk-entity/src/snapshot.rs` with:

```rust
//! Entity snapshot — immutable borrow for observation and propagator reads.

use murk_core::entity::EntityManifest;
use murk_core::id::{EntityId, PropertyIndex};

use crate::record::EntityRecord;

/// Immutable borrow of the entity store, handed out alongside field snapshots.
///
/// All lookups validate `EntityId.generation()` against the store's generation
/// for the slot. Stale IDs return `None`, not the wrong entity's data.
pub struct EntitySnapshot<'a> {
    records: &'a [EntityRecord],
    properties: &'a [f32],
    generations: &'a [u32],
    manifest: &'a EntityManifest,
    next_slot: u32,
    property_count: usize,
}

impl<'a> EntitySnapshot<'a> {
    /// Create a snapshot from store internals.
    pub fn new(
        records: &'a [EntityRecord],
        properties: &'a [f32],
        generations: &'a [u32],
        manifest: &'a EntityManifest,
        next_slot: u32,
    ) -> Self {
        Self {
            records,
            properties,
            generations,
            manifest,
            next_slot,
            property_count: manifest.property_count(),
        }
    }

    /// Look up entity by ID. Returns None if generation mismatch (stale ID)
    /// or slot out of range.
    pub fn get(&self, id: EntityId) -> Option<&EntityRecord> {
        let slot = id.slot() as usize;
        if slot >= self.next_slot as usize {
            return None;
        }
        if self.generations[slot] != id.generation() {
            return None;
        }
        Some(&self.records[slot])
    }

    /// Iterate all allocated records (alive and dead).
    pub fn iter_all(&self) -> impl Iterator<Item = &EntityRecord> {
        self.records[..self.next_slot as usize].iter()
    }

    /// Iterate alive entities (where alive property > 0.0).
    pub fn iter_alive(&self) -> impl Iterator<Item = &EntityRecord> + '_ {
        self.records[..self.next_slot as usize]
            .iter()
            .filter(|r| self.is_alive(r.id))
    }

    /// Read a property value. Returns None for stale ID or out-of-range PropertyIndex.
    pub fn property(&self, id: EntityId, prop: PropertyIndex) -> Option<f32> {
        let slot = id.slot() as usize;
        if slot >= self.next_slot as usize {
            return None;
        }
        if self.generations[slot] != id.generation() {
            return None;
        }
        let idx = prop.0 as usize;
        if idx >= self.property_count {
            return None;
        }
        Some(self.properties[slot * self.property_count + idx])
    }

    /// Check if an entity is alive.
    pub fn is_alive(&self, id: EntityId) -> bool {
        let slot = id.slot() as usize;
        if slot >= self.next_slot as usize {
            return false;
        }
        if self.generations[slot] != id.generation() {
            return false;
        }
        let alive_idx = self.manifest.alive_property.0 as usize;
        self.properties[slot * self.property_count + alive_idx] > 0.0
    }

    /// The property manifest.
    pub fn manifest(&self) -> &EntityManifest {
        self.manifest
    }

    /// Number of currently alive entities.
    pub fn alive_count(&self) -> u32 {
        self.iter_alive().count() as u32
    }

    /// Property count per entity.
    pub fn property_count(&self) -> usize {
        self.property_count
    }

    /// Read-only access to the generation array.
    pub fn generations(&self) -> &[u32] {
        self.generations
    }

    /// Read-only access to the property slab.
    pub fn properties(&self) -> &[f32] {
        self.properties
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::EntityStore;
    use murk_core::entity::EntityManifest;

    fn test_manifest() -> EntityManifest {
        EntityManifest {
            property_names: vec!["alive".into(), "hp".into()],
            property_defaults: vec![1.0, 100.0],
            alive_property: PropertyIndex(0),
        }
    }

    #[test]
    fn snapshot_get_returns_record() {
        let mut store = EntityStore::new(4, test_manifest());
        let id = store.spawn(vec![5, 10].into(), 2, &[]).unwrap();
        let snap = store.snapshot();
        let record = snap.get(id).unwrap();
        assert_eq!(record.coord.as_slice(), &[5, 10]);
        assert_eq!(record.entity_type, 2);
    }

    #[test]
    fn snapshot_stale_id_returns_none() {
        let mut store = EntityStore::new(4, test_manifest());
        let old = store.spawn(vec![0].into(), 0, &[]).unwrap();
        store.despawn(old).unwrap();
        let _new = store.spawn(vec![1].into(), 0, &[]).unwrap();
        let snap = store.snapshot();
        assert!(snap.get(old).is_none());
    }

    #[test]
    fn snapshot_iter_alive_skips_dead() {
        let mut store = EntityStore::new(4, test_manifest());
        let id0 = store.spawn(vec![0].into(), 0, &[]).unwrap();
        let _id1 = store.spawn(vec![1].into(), 0, &[]).unwrap();
        store.set_property(id0, PropertyIndex(0), 0.0); // kill id0
        let snap = store.snapshot();
        assert_eq!(snap.alive_count(), 1);
        assert_eq!(snap.iter_alive().count(), 1);
        assert_eq!(snap.iter_all().count(), 2);
    }

    #[test]
    fn snapshot_property_validates_generation() {
        let mut store = EntityStore::new(4, test_manifest());
        let old = store.spawn(vec![0].into(), 0, &[]).unwrap();
        store.despawn(old).unwrap();
        let _new = store.spawn(vec![1].into(), 0, &[]).unwrap();
        let snap = store.snapshot();
        assert_eq!(snap.property(old, PropertyIndex(1)), None);
    }

    #[test]
    fn snapshot_property_out_of_range() {
        let mut store = EntityStore::new(4, test_manifest());
        let id = store.spawn(vec![0].into(), 0, &[]).unwrap();
        let snap = store.snapshot();
        assert_eq!(snap.property(id, PropertyIndex(99)), None);
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p murk-entity snapshot::tests -- --nocapture`
Expected: All 5 tests pass.

- [ ] **Step 4: Commit**

```
git add crates/murk-entity/src/snapshot.rs crates/murk-entity/src/store.rs
git commit -m "feat(entity): add EntitySnapshot with generation-validated lookups"
```

---

## Task 8: PropertyStaging

**Files:**
- Modify: `crates/murk-entity/src/staging.rs`

- [ ] **Step 1: Implement PropertyStaging with tests**

Replace contents of `crates/murk-entity/src/staging.rs`:

```rust
//! Property staging — write buffer for propagator entity property mutations.

use murk_core::id::{EntityId, PropertyIndex};

/// Write buffer for entity property mutations during a tick.
///
/// Uses a flat `Vec<f32>` for values and a `Vec<u64>` bitset for tracking
/// which (entity, property) pairs have been written. No external crate
/// dependency for the bitset — 360 bits (Echelon scale) = 6 u64s.
///
/// Separate allocation from the entity store, enabling split-borrow in
/// `StepContext`: the snapshot borrows the store immutably while staging
/// is borrowed mutably.
pub struct PropertyStaging {
    values: Vec<f32>,
    written: Vec<u64>,
    max_entities: u32,
    property_count: u32,
}

impl PropertyStaging {
    /// Create a new staging buffer.
    pub fn new(max_entities: u32, property_count: u32) -> Self {
        let total = max_entities as usize * property_count as usize;
        let words = (total + 63) / 64;
        Self {
            values: vec![0.0; total],
            written: vec![0u64; words],
            max_entities,
            property_count,
        }
    }

    /// Write a property value to staging. Returns false if out of bounds.
    pub fn set(&mut self, id: EntityId, prop: PropertyIndex, value: f32) -> bool {
        let slot = id.slot() as usize;
        let pidx = prop.0 as usize;
        if slot >= self.max_entities as usize || pidx >= self.property_count as usize {
            return false;
        }
        let flat = slot * self.property_count as usize + pidx;
        self.values[flat] = value;
        self.written[flat / 64] |= 1u64 << (flat % 64);
        true
    }

    /// Read a staged value. Returns None if not written or out of bounds.
    pub fn get(&self, id: EntityId, prop: PropertyIndex) -> Option<f32> {
        let slot = id.slot() as usize;
        let pidx = prop.0 as usize;
        if slot >= self.max_entities as usize || pidx >= self.property_count as usize {
            return None;
        }
        let flat = slot * self.property_count as usize + pidx;
        if self.written[flat / 64] & (1u64 << (flat % 64)) != 0 {
            Some(self.values[flat])
        } else {
            None
        }
    }

    /// Clear all writes. Called between ticks.
    pub fn reset(&mut self) {
        self.written.fill(0);
    }

    /// Apply all staged writes to an entity store's property slab.
    pub fn apply_to(&self, properties: &mut [f32]) {
        let total = self.max_entities as usize * self.property_count as usize;
        for word_idx in 0..self.written.len() {
            let mut word = self.written[word_idx];
            while word != 0 {
                let bit = word.trailing_zeros() as usize;
                let flat = word_idx * 64 + bit;
                if flat < total {
                    properties[flat] = self.values[flat];
                }
                word &= word - 1; // clear lowest set bit
            }
        }
    }

    /// Max entities this staging buffer supports.
    pub fn max_entities(&self) -> u32 {
        self.max_entities
    }

    /// Property count per entity.
    pub fn property_count(&self) -> u32 {
        self.property_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get_round_trip() {
        let mut staging = PropertyStaging::new(4, 3);
        let id = EntityId::new(1, 0);
        assert!(staging.set(id, PropertyIndex(2), 42.0));
        assert_eq!(staging.get(id, PropertyIndex(2)), Some(42.0));
    }

    #[test]
    fn get_unwritten_returns_none() {
        let staging = PropertyStaging::new(4, 3);
        let id = EntityId::new(0, 0);
        assert_eq!(staging.get(id, PropertyIndex(0)), None);
    }

    #[test]
    fn set_out_of_bounds_returns_false() {
        let mut staging = PropertyStaging::new(4, 3);
        assert!(!staging.set(EntityId::new(10, 0), PropertyIndex(0), 1.0));
        assert!(!staging.set(EntityId::new(0, 0), PropertyIndex(10), 1.0));
    }

    #[test]
    fn reset_clears_all_writes() {
        let mut staging = PropertyStaging::new(4, 3);
        staging.set(EntityId::new(0, 0), PropertyIndex(0), 1.0);
        staging.set(EntityId::new(1, 0), PropertyIndex(1), 2.0);
        staging.reset();
        assert_eq!(staging.get(EntityId::new(0, 0), PropertyIndex(0)), None);
        assert_eq!(staging.get(EntityId::new(1, 0), PropertyIndex(1)), None);
    }

    #[test]
    fn apply_to_writes_staged_values() {
        let mut staging = PropertyStaging::new(2, 3);
        staging.set(EntityId::new(0, 0), PropertyIndex(1), 99.0);
        staging.set(EntityId::new(1, 0), PropertyIndex(2), 77.0);

        let mut props = vec![0.0; 6]; // 2 entities * 3 properties
        staging.apply_to(&mut props);

        assert_eq!(props[1], 99.0); // entity 0, property 1
        assert_eq!(props[5], 77.0); // entity 1, property 2
        assert_eq!(props[0], 0.0);  // untouched
    }

    #[test]
    fn bitset_handles_more_than_64_entries() {
        // 10 entities * 8 properties = 80 entries = 2 u64 words
        let mut staging = PropertyStaging::new(10, 8);
        // Write to flat index 70 (entity 8, property 6)
        staging.set(EntityId::new(8, 0), PropertyIndex(6), 3.14);
        assert_eq!(staging.get(EntityId::new(8, 0), PropertyIndex(6)), Some(3.14));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p murk-entity staging::tests -- --nocapture`
Expected: All 6 tests pass.

- [ ] **Step 3: Commit**

```
git add crates/murk-entity/src/staging.rs
git commit -m "feat(entity): add PropertyStaging with Vec<u64> bitset"
```

---

## Task 9: EntityOverlayReader

**Files:**
- Modify: `crates/murk-entity/src/overlay.rs`

- [ ] **Step 1: Implement EntityOverlayReader with tests**

Replace contents of `crates/murk-entity/src/overlay.rs`:

```rust
//! Entity overlay reader — Euler-style reads with staging fallback.

use murk_core::entity::EntityManifest;
use murk_core::id::{EntityId, PropertyIndex};

use crate::record::EntityRecord;
use crate::snapshot::EntitySnapshot;
use crate::staging::PropertyStaging;

/// Euler-style entity reader that checks staging before snapshot.
///
/// Uses two lifetime parameters to avoid the `&'a T<'a>` invariance
/// footgun that causes "lifetime may not live long enough" compiler errors.
pub struct EntityOverlayReader<'snap, 'staging> {
    snapshot: &'snap EntitySnapshot<'snap>,
    staging: &'staging PropertyStaging,
}

impl<'snap, 'staging> EntityOverlayReader<'snap, 'staging> {
    /// Create an overlay reader.
    pub fn new(
        snapshot: &'snap EntitySnapshot<'snap>,
        staging: &'staging PropertyStaging,
    ) -> Self {
        Self { snapshot, staging }
    }

    /// Read a property with Euler semantics: staging takes precedence,
    /// but only if the snapshot confirms the entity is valid (generation match).
    /// This prevents stale IDs from reading staged values for recycled slots.
    pub fn property(&self, id: EntityId, prop: PropertyIndex) -> Option<f32> {
        // Validate entity exists via snapshot first (generation check).
        // Without this gate, a stale ID could read staged values written
        // to the same slot by a different (recycled) entity.
        if self.snapshot.get(id).is_none() {
            return None;
        }
        // Entity is valid — check staging first, fall back to snapshot.
        if let Some(val) = self.staging.get(id, prop) {
            Some(val)
        } else {
            self.snapshot.property(id, prop)
        }
    }

    /// Look up entity record. Delegates to snapshot (structural data).
    pub fn get(&self, id: EntityId) -> Option<&EntityRecord> {
        self.snapshot.get(id)
    }

    /// Iterate all allocated records. Delegates to snapshot.
    pub fn iter_all(&self) -> impl Iterator<Item = &EntityRecord> {
        self.snapshot.iter_all()
    }

    /// Iterate alive entities. Delegates to snapshot.
    pub fn iter_alive(&self) -> impl Iterator<Item = &EntityRecord> + '_ {
        self.snapshot.iter_alive()
    }

    /// Check if an entity is alive. Delegates to snapshot.
    pub fn is_alive(&self, id: EntityId) -> bool {
        self.snapshot.is_alive(id)
    }

    /// The property manifest. Delegates to snapshot.
    pub fn manifest(&self) -> &EntityManifest {
        self.snapshot.manifest()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::EntityStore;
    use murk_core::entity::EntityManifest;

    fn test_manifest() -> EntityManifest {
        EntityManifest {
            property_names: vec!["alive".into(), "hp".into()],
            property_defaults: vec![1.0, 100.0],
            alive_property: PropertyIndex(0),
        }
    }

    #[test]
    fn overlay_returns_staged_value_over_snapshot() {
        let mut store = EntityStore::new(4, test_manifest());
        let id = store.spawn(vec![0].into(), 0, &[]).unwrap();
        let snap = store.snapshot();

        let mut staging = PropertyStaging::new(4, 2);
        staging.set(id, PropertyIndex(1), 50.0); // stage HP=50

        let overlay = EntityOverlayReader::new(&snap, &staging);
        // HP should be 50 (staged), not 100 (snapshot default).
        assert_eq!(overlay.property(id, PropertyIndex(1)), Some(50.0));
        // Alive is not staged, falls through to snapshot.
        assert_eq!(overlay.property(id, PropertyIndex(0)), Some(1.0));
    }

    #[test]
    fn overlay_falls_through_to_snapshot_when_not_staged() {
        let mut store = EntityStore::new(4, test_manifest());
        let id = store.spawn(vec![0].into(), 0, &[]).unwrap();
        let snap = store.snapshot();

        let staging = PropertyStaging::new(4, 2); // nothing staged
        let overlay = EntityOverlayReader::new(&snap, &staging);
        assert_eq!(overlay.property(id, PropertyIndex(1)), Some(100.0)); // snapshot default
    }

    #[test]
    fn overlay_get_delegates_to_snapshot() {
        let mut store = EntityStore::new(4, test_manifest());
        let id = store.spawn(vec![3, 7].into(), 1, &[]).unwrap();
        let snap = store.snapshot();
        let staging = PropertyStaging::new(4, 2);
        let overlay = EntityOverlayReader::new(&snap, &staging);
        let record = overlay.get(id).unwrap();
        assert_eq!(record.coord.as_slice(), &[3, 7]);
        assert_eq!(record.entity_type, 1);
    }

    #[test]
    fn overlay_stale_id_returns_none() {
        let mut store = EntityStore::new(4, test_manifest());
        let old = store.spawn(vec![0].into(), 0, &[]).unwrap();
        store.despawn(old).unwrap();
        let _new = store.spawn(vec![1].into(), 0, &[]).unwrap();
        let snap = store.snapshot();
        let staging = PropertyStaging::new(4, 2);
        let overlay = EntityOverlayReader::new(&snap, &staging);
        assert!(overlay.get(old).is_none());
        assert_eq!(overlay.property(old, PropertyIndex(0)), None);
    }

    #[test]
    fn overlay_stale_id_does_not_read_staged_value_for_recycled_slot() {
        // Regression test: staging keys by slot only, so a stale ID
        // could read staged values written for a recycled slot's new entity.
        let mut store = EntityStore::new(4, test_manifest());
        let old = store.spawn(vec![0].into(), 0, &[]).unwrap();
        store.despawn(old).unwrap();
        let new_id = store.spawn(vec![1].into(), 0, &[]).unwrap();
        assert_eq!(old.slot(), new_id.slot(), "same slot, different generation");

        let snap = store.snapshot();
        let mut staging = PropertyStaging::new(4, 2);
        // Stage a write for the NEW entity at the recycled slot.
        staging.set(new_id, PropertyIndex(1), 999.0);

        let overlay = EntityOverlayReader::new(&snap, &staging);
        // The OLD id must NOT see the staged value — generation mismatch.
        assert_eq!(overlay.property(old, PropertyIndex(1)), None);
        // The NEW id SHOULD see it.
        assert_eq!(overlay.property(new_id, PropertyIndex(1)), Some(999.0));
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p murk-entity overlay::tests -- --nocapture`
Expected: All 4 tests pass.

- [ ] **Step 3: Run full crate tests**

Run: `cargo test -p murk-entity -- --nocapture`
Expected: All tests across all modules pass (record: 2, store: 12, snapshot: 5, staging: 6, overlay: 4 = 29 total).

- [ ] **Step 4: Commit**

```
git add crates/murk-entity/src/overlay.rs
git commit -m "feat(entity): add EntityOverlayReader with two-lifetime Euler overlay"
```

---

## Task 10: Final integration verification

- [ ] **Step 1: Run full workspace check**

Run: `cargo check --workspace`
Expected: Clean compilation. No warnings related to entity types.

- [ ] **Step 2: Run full workspace tests**

Run: `cargo test --workspace 2>&1 | tail -30`
Expected: All tests pass. No regressions in existing crates.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy --workspace -- -D warnings 2>&1 | tail -20`
Expected: No warnings. Fix any clippy issues.

- [ ] **Step 4: Verify doc tests compile**

Run: `cargo test --doc -p murk-core -p murk-entity 2>&1 | tail -20`
Expected: All doc tests pass.

- [ ] **Step 5: Final commit if any fixes needed**

If clippy or doc tests required changes:
```
git add -u
git commit -m "chore(entity): fix clippy warnings and doc tests for M1"
```

---

## Task 11: Write M2 plan and continue

**This milestone is not done until M2 is planned and execution begins.**

- [ ] **Step 1: Write M2 implementation plan**

Use the writing-plans skill to create `docs/superpowers/plans/2026-04-05-entity-model-m2-engine.md` from the spec (Section 3: murk-engine Integration). The plan should be written against the actual codebase state after M1 — real file paths, real type signatures, real test patterns.

- [ ] **Step 2: Begin M2 execution**

Hand off M2 plan for execution. The entity model is a 6-milestone series:
- M1: murk-core + murk-entity (this plan)
- **M2: murk-engine integration (next)**
- M3: murk-propagator integration
- M4: murk-slot (entity-slot observations)
- M5: FFI + Python bindings
- M6: EntityProjection + replay

Each milestone produces working, testable software and feeds into the next.

> **REMINDER:** M1 alone does not deliver value to consumers. Entity types exist but nothing uses them. The entity model is inert until M2 (engine wires up commands), M3 (propagators can read/write entities), and M4 (observations extract entity-slot tensors). M5 is where Python training gets unblocked. Do not stop here — this is foundation, not product.
