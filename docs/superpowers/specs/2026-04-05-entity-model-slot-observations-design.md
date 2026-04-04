# Entity Model and Entity-Slot Observations

**Date:** 2026-04-05
**Status:** Draft (revised after 7-panel review)
**Scope:** Full entity-field model for Murk — entity store, propagator integration, entity-owned properties, and entity-slot observation extraction.

## Motivation

Murk's current model is purely cell-based: fields are `&[f32]` arrays indexed by cell rank, propagators operate on spatial cells, and observations extract spatial field values. Downstream RL consumers (e.g., Echelon) need entity-centric observations — "nearest 4 hostile mechs, each with [hp, heading, distance]" — but Murk has no entity concept.

Without a native entity model, consumers encode entities as field-value conventions (field[3] > 0.0 means "entity here") and perform sorting/filtering/slot-filling in Python per-step. At 1024 environments × 14K steps/sec, this Python-side reshaping serializes the training loop via GIL contention and blows the throughput budget.

### Why not a workaround?

A three-panel review (architecture critic, systems thinker, PyTorch engineer) evaluated a "cells-as-proxy-entities" workaround (SlotPlan reading field values to infer entity presence). All three identified the same core risk: building entity semantics on a system with no entity model creates a Shifting the Burden archetype — the workaround reduces pressure for the real solution while accumulating API entrenchment and silent correctness degradation at high entity density. Since Murk is a published crate (not Echelon-specific), the right answer is the real entity model.

### Design validation

Every design decision was validated against Echelon's concrete requirements:
- 22 mechs + ~50 projectiles = 72 entities max (fixed capacity)
- 2 entity types (mech, projectile) with homogeneous properties (~15 floats)
- Propagator access patterns: movement, LOS, combat, death — all entity-first, terrain-by-coord
- Observation contract: self-state + ranked contact slots with relative transforms

### Review history

Initial design reviewed by 7-specialist panel (architecture critic, systems thinker, PyTorch engineer, Rust engineer, API architect, QA analyst, security analyst). Key revisions from review:
- EntityId packs generation (20-bit slot + 12-bit generation) — closes stale-ID semantic use-after-free
- `alive: bool` removed from EntityRecord — aliveness determined solely by `properties[ALIVE_PROP]`
- EntityOverlayReader uses two lifetime parameters — fixes invariance compiler error
- Rollback uses entity store snapshot/restore — simpler than per-mutation undo
- `overflow_to` validated for acyclicity at compile time
- `entities()` returns `Option`, not panic — enables propagator reuse across entity/non-entity worlds
- Properties stored as flat slab in EntityStore, not `Box<[f32]>` per record
- `SinCos` added to PropertyExtract for periodic angular properties
- `RelativeTo` uses shortest-path distance through space topology
- `murk-replay` added to modified crates list
- BatchedEngine integration for vectorized training
- GridGeometry extracted to murk-space (removes murk-slot → murk-obs coupling)

## Architecture Overview

Crates involved, each with a single responsibility:

| Crate | Purpose | Depends on |
|-------|---------|------------|
| `murk-core` (modified) | `EntityId`, `PropertyIndex`, `EntityManifest`, command fixes, error variants | — |
| `murk-space` (modified) | `GridGeometry` extracted from murk-obs | `murk-core` |
| `murk-entity` (new) | `EntityRecord`, `EntityStore`, `EntitySnapshot`, `PropertyStaging`, `EntityOverlayReader` | `murk-core` |
| `murk-engine` (modified) | Spawn/Despawn/Move handling, entity rollback, entity snapshot in StepResult | `murk-entity` |
| `murk-propagator` (modified) | StepContext entity access (Euler + Jacobi), Propagator trait extensions | `murk-entity` |
| `murk-slot` (new) | `SlotSpec`, `SlotPlan`, entity-slot observation extraction | `murk-entity`, `murk-space` (for GridGeometry) |
| `murk-propagators` (modified) | `EntityProjection` convenience propagator | `murk-entity` |
| `murk-replay` (modified) | Replay format version bump for CommandPayload changes | `murk-core` |

### Dependency flow

```
murk-core
  ├── murk-space (GridGeometry extracted here)
  └── murk-entity
        ├── murk-engine (entity store in tick lifecycle)
        ├── murk-propagator (StepContext entity access)
        ├── murk-slot (entity-slot observations, uses murk-space for GridGeometry)
        └── murk-propagators (EntityProjection)
```

Key properties:
- `murk-slot` does not depend on `murk-engine`, `murk-propagator`, or `murk-obs`.
- Propagators do not depend on `murk-slot`.
- `murk-obs` coupling eliminated — GridGeometry moved to `murk-space`.

## 1. murk-core Changes

Small, targeted additions to the types crate. No behavioral logic.

### New types in id.rs

```rust
/// Identifies an entity within a simulation world.
///
/// Packs a 20-bit slot index and 12-bit generation counter into a u32.
/// The generation prevents stale-ID reuse: after despawn and slot recycling,
/// a stale EntityId will fail generation validation on lookup.
///
/// - 20-bit slot → max 1,048,575 concurrent entities
/// - 12-bit generation → 4,096 generations per slot before wrap
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(u32);  // NOT pub inner — access via methods only

impl EntityId {
    pub fn new(slot: u32, generation: u32) -> Self { ... }
    pub fn slot(&self) -> u32 { self.0 & 0x000F_FFFF }
    pub fn generation(&self) -> u32 { self.0 >> 20 }
}

/// Indexes into an entity's property array.
///
/// Property layout is defined by [`EntityManifest`] at world creation.
/// All entities in a world share the same property layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PropertyIndex(pub u32);
```

### EntityManifest in murk-core

Config/vocabulary type alongside `FieldDef`. Defines the property layout for all entities in a world.

```rust
/// Declares the property schema for entities in a world.
///
/// All entities share the same property layout (homogeneous).
/// Property values default to `property_defaults[i]` on spawn
/// unless overridden by the Spawn command.
///
/// One property should be designated as the ALIVE indicator
/// (conventionally PropertyIndex(0)). The engine stamps 1.0 on spawn,
/// 0.0 on despawn. Propagators write 0.0 for death. `iter_alive()`
/// reads this property to determine liveness.
pub struct EntityManifest {
    /// Human-readable property names (for debugging, rendering, logging).
    pub property_names: Vec<String>,
    /// Default values for each property on spawn.
    pub property_defaults: Vec<f32>,
    /// Which property index represents the ALIVE flag.
    /// `iter_alive()` filters on `properties[alive_property] > 0.0`.
    pub alive_property: PropertyIndex,
}
```

Lives in murk-core because it's a config type (like `FieldDef`), not a runtime type. The `EntityStore` (which uses it) lives in murk-entity.

### Command payload changes (BREAKING)

```rust
pub enum CommandPayload {
    Move {
        entity_id: EntityId,        // was: u64
        target_coord: Coord,
    },
    Spawn {
        coord: Coord,
        entity_type: u32,           // NEW: entity type tag
        property_overrides: Vec<(PropertyIndex, f32)>,  // was: field_values: Vec<(FieldId, f32)>
    },
    Despawn {
        entity_id: EntityId,        // was: u64
    },
    // SetField, SetParameter, SetParameterBatch, Custom: unchanged
}
```

Breaking changes:
- `entity_id: u64` → `entity_id: EntityId` on Move and Despawn
- `field_values: Vec<(FieldId, f32)>` → `property_overrides: Vec<(PropertyIndex, f32)>` on Spawn
- Spawn gains `entity_type: u32`

These variants currently return `UnsupportedCommand`, so no working consumer code breaks.

### New error variants

```rust
pub enum IngressError {
    // ... existing variants ...
    /// The referenced entity does not exist, has been despawned,
    /// or the EntityId generation does not match the current slot occupant.
    UnknownEntity,
    /// Entity capacity is full — cannot spawn.
    EntityCapacityFull,
}
```

### WorldConfig addition

```rust
/// Maximum number of concurrent entities. 0 = entities disabled.
pub max_entities: u32,
/// Entity property schema. Required if max_entities > 0.
/// ConfigError::EntityManifestRequired if None when max_entities > 0.
pub entity_manifest: Option<EntityManifest>,
```

When `max_entities == 0`, entity commands remain `UnsupportedCommand` (backwards compatible). `max_entities > 0` with `entity_manifest: None` is a `ConfigError::EntityManifestRequired` at build time.

## 2. murk-entity Crate

New crate. Pure data model + entity store. No engine or observation dependency.

### EntityRecord

```rust
pub struct EntityRecord {
    /// Unique identifier within this world (includes generation).
    pub id: EntityId,
    /// Current position in simulation space.
    pub coord: Coord,
    /// Type tag for classification (e.g., mech=0, projectile=1).
    pub entity_type: u32,
}
```

Note: `alive: bool` is NOT a field on EntityRecord. Aliveness is determined solely by `properties[manifest.alive_property] > 0.0`, read from the flat property slab. One representation, no sync divergence.

Design note: all entities have the same property count (homogeneous). Entities of different types (e.g., mechs with 15 meaningful properties, projectiles with 5) carry unused floats. At Echelon's scale (24 entities × ~10 unused floats × 4 bytes = ~960 bytes), this waste is negligible. Heterogeneous property sets (per-type manifests) are not planned. **Trigger for reconsideration:** if any propagator branches on `entity_type` to interpret a `PropertyIndex` differently for different entity types, that signals the homogeneous model has degraded.

### EntityStore

```rust
pub struct EntityStore {
    /// Structural records, indexed by slot index (EntityId.slot()).
    records: Box<[EntityRecord]>,
    /// Properties as flat SoA slab: [max_entities * property_count].
    /// Indexed by slot * property_count + prop.
    properties: Box<[f32]>,
    /// Generation counter per slot (for EntityId validation).
    generations: Box<[u32]>,
    /// Recycled slot indices available for spawn.
    free_list: Vec<u32>,
    /// Reverse lookup: coordinate → entities at that cell.
    /// Note: includes dead entities. Callers must filter by alive property.
    coord_index: HashMap<CoordKey, SmallVec<[EntityId; 4]>>,
    /// Number of currently alive entities.
    alive_count: u32,
    /// High-water mark for slot allocation.
    next_slot: u32,
    /// Maximum capacity (from WorldConfig).
    capacity: u32,
    /// Property schema.
    manifest: EntityManifest,
}
```

Properties stored as flat SoA slab (not `Box<[f32]>` per record). Same layout as PropertyStaging. Cache-friendly: iterating entity properties walks a contiguous allocation. For Echelon: 72 × 15 × 4 = 4.3 KB — fits in L1 cache.

`records` and `generations` and `properties` are always indexed by `EntityId.slot() as usize`. Debug assertions enforce this invariant.

**Operations:**
- **Spawn:** Pop from free list (or bump next_slot if empty). Write record with manifest defaults, apply property_overrides. Set `properties[alive_property] = 1.0`. Insert into coord_index. O(1). Returns `EntityId` (with current generation for that slot) or `EntityCapacityFull`.
- **Despawn:** Validate `EntityId.generation()` matches `generations[slot]` and entity is alive (else `UnknownEntity`). Set `properties[alive_property] = 0.0`, increment generation, push slot to free list, remove from coord_index. O(1).
- **Move:** Validate generation match and alive (else `UnknownEntity`). Validate `target_coord` against space topology (else `NotApplied`). Dead entities (alive=0) return `UnknownEntity` — dead entities should not be moved. Update coord, update coord_index. O(1).
- **Lookup by ID:** Check `generations[id.slot()] == id.generation()`. If mismatch, return `None` (stale ID). O(1).
- **Lookup by coord:** `coord_index[coord_key]` — O(1). Returns `SmallVec`. **Includes dead entities** — callers must filter by alive property.
- **Iterate alive:** Scan `records[0..next_slot]`, filter by `properties[slot * property_count + manifest.alive_property.0] > 0.0`.
- **Property read:** `properties[id.slot() * property_count + prop.0]` — O(1). Bounds-checked: returns `Option<f32>`, returns `None` for out-of-range PropertyIndex.
- **Property write:** Same indexing. Returns `bool` (false if out of bounds). `#[cfg(debug_assertions)]` emits a warning naming the bad index.

### EntitySnapshot

Immutable borrow of the entity store, handed out alongside field snapshots.

```rust
pub struct EntitySnapshot<'a> {
    records: &'a [EntityRecord],
    properties: &'a [f32],
    generations: &'a [u32],
    manifest: &'a EntityManifest,
    alive_count: u32,
    property_count: u32,
}

impl<'a> EntitySnapshot<'a> {
    /// Look up entity by ID. Returns None if generation mismatch (stale ID)
    /// or slot out of range.
    pub fn get(&self, id: EntityId) -> Option<&EntityRecord>;
    pub fn iter_all(&self) -> impl Iterator<Item = &EntityRecord>;
    /// Iterates entities where properties[alive_property] > 0.0.
    pub fn iter_alive(&self) -> impl Iterator<Item = &EntityRecord>;
    /// Returns None for stale ID or out-of-range PropertyIndex.
    pub fn property(&self, id: EntityId, prop: PropertyIndex) -> Option<f32>;
    pub fn is_alive(&self, id: EntityId) -> bool;
    pub fn manifest(&self) -> &EntityManifest;
    pub fn alive_count(&self) -> u32;
}
```

All lookups validate `EntityId.generation()` against `generations[slot]`. Stale IDs return `None`, not the wrong entity's data.

### PropertyStaging

Write buffer for entity property mutations during a tick. Separate allocation from the entity store, enabling split-borrow in StepContext.

```rust
pub struct PropertyStaging {
    /// Written values: flat [f32; max_entities * property_count].
    values: Vec<f32>,
    /// Bitset tracking which (entity, property) pairs have been written.
    /// Fixed-size array: [u64; N] where N = ceil(max_entities * property_count / 64).
    /// No BitVec dependency — 360 bits = 6 u64s for Echelon.
    written: Vec<u64>,
    /// Dimensions for index arithmetic.
    max_entities: u32,
    property_count: u32,
}

impl PropertyStaging {
    /// Write a property value. Returns false if entity slot or property index
    /// out of bounds.
    pub fn set(&mut self, id: EntityId, prop: PropertyIndex, value: f32) -> bool;
    /// Read a staged value. Returns None if not written or out of bounds.
    pub fn get(&self, id: EntityId, prop: PropertyIndex) -> Option<f32>;
    pub fn reset(&mut self);  // clear all writes (between ticks)
    pub fn apply_to(&self, store: &mut EntityStore);  // commit writes
}
```

Memory: for Echelon, `24 * 15 * 4 = 1.4 KB` values + `6 * 8 = 48 bytes` bitset. Fits in L1 cache. No external crate dependency for the bitset.

Bounds checking on both `set()` and `get()`: slot index < max_entities, property index < property_count. Returns `false`/`None` on out-of-bounds rather than panicking — a buggy propagator gets a recoverable error, not a process crash.

### EntityOverlayReader

Euler-style reader that checks staging before falling back to snapshot. Used by propagators that declare `reads_entities()`.

```rust
pub struct EntityOverlayReader<'snap, 'staging> {
    snapshot: &'snap EntitySnapshot<'snap>,
    staging: &'staging PropertyStaging,
}

impl<'snap, 'staging> EntityOverlayReader<'snap, 'staging> {
    /// Read a property with Euler semantics: staging (prior propagator writes)
    /// takes precedence over tick-start snapshot.
    pub fn property(&self, id: EntityId, prop: PropertyIndex) -> Option<f32> {
        if let Some(val) = self.staging.get(id, prop) {
            Some(val)
        } else {
            self.snapshot.property(id, prop)
        }
    }

    // Delegates to snapshot for non-property reads (coord, entity_type).
    // Generation validation happens in snapshot.get().
    pub fn get(&self, id: EntityId) -> Option<&EntityRecord>;
    pub fn iter_all(&self) -> impl Iterator<Item = &EntityRecord>;
    pub fn iter_alive(&self) -> impl Iterator<Item = &EntityRecord>;
}
```

Two lifetime parameters (`'snap`, `'staging`) avoid the `&'a T<'a>` invariance footgun that would cause "lifetime may not live long enough" compiler errors.

Per-lookup overhead: one bitset check (array index + bit mask). For Echelon's LOS reading 22 entity positions: ~22ns total.

## 3. murk-engine Integration

### Tick lifecycle

Entity commands are processed **before propagators run**, so propagators see spawned/moved entities this tick:

```
1. begin_tick()
2. drain ingress queue
3. apply SetField commands              (existing)
4. apply SetParameter commands          (existing)
5. snapshot entity store                (NEW — for rollback)
6. apply Spawn/Move/Despawn commands    (NEW — direct to entity store)
7. run propagator pipeline              (existing — now with entity access)
8. commit entity property staging       (NEW — propagator property writes → store)
9. publish snapshot                     (existing — now includes entity snapshot)

On rollback at step 7: restore entity store from step-5 snapshot,
discard PropertyStaging. Field staging abandoned as before.
```

**Why before propagators:** Echelon's reset() issues Spawn commands for all mechs; the first tick's propagators need to see them. If spawn takes effect next tick, reset() requires a dummy tick — an API wart every consumer would need to work around.

### Entity rollback via snapshot/restore

Before step 6, the engine takes a lightweight snapshot of the entity store state (records, properties slab, generations, free_list, coord_index, alive_count, next_slot). On propagator failure at step 7, the snapshot is restored. At Echelon's scale: 24 entities × ~80 bytes + 4.3 KB properties = ~6 KB memcpy. Trivial and simpler than per-mutation undo logic.

This replaces the earlier changeset-based design. The changeset pattern required capturing inverse state for each mutation type (particularly difficult for Despawn, which needs the full entity record, generation, and coord_index entry). Snapshot/restore is O(capacity) but constant and cheap at realistic entity counts.

### Command handling

Replaces the `UnsupportedCommand` arm at `tick.rs:310-320`:

**Spawn:** Allocate from entity store (free list or bump). Write `entity_type`, apply `property_overrides` over manifest defaults. Set `properties[alive_property] = 1.0`. Insert into coord_index. Return `EntityCapacityFull` if at max_entities. The allocated `EntityId` (with generation) is included in the receipt.

**Despawn:** Validate `EntityId.generation()` matches current generation and entity is alive (else `UnknownEntity`). Set `properties[alive_property] = 0.0`, increment generation, push slot to free list, remove from coord_index.

**Move:** Validate generation match and alive (else `UnknownEntity`). Dead entities return `UnknownEntity`. Validate `target_coord` against space topology (else `NotApplied`). Update coord, update coord_index.

### Receipt extension

```rust
pub struct Receipt {
    // ... existing fields ...
    /// EntityId allocated by a Spawn command, if applicable.
    /// Includes the generation — callers store this for future commands.
    pub spawned_entity_id: Option<EntityId>,
}
```

### StepResult extension

```rust
pub struct StepResult<'w> {
    pub snapshot: Snapshot<'w>,
    /// Entity snapshot. None when max_entities == 0 (entities disabled).
    pub entity_snapshot: Option<EntitySnapshot<'w>>,
    pub receipts: Vec<Receipt>,
    pub metrics: StepMetrics,
}
```

`Option<EntitySnapshot>` — consistent with entity accessors returning `Option`.

### Dead vs despawned: a design principle

**Death is a state. Despawn is removal.**

A dead entity (`properties[alive_property] = 0.0`) still has identity, position, and properties. It stays in the entity store. `iter_alive()` skips it, but `iter_all()` includes it. The observation system sees `alive=0` — distinguishing dead from invisible.

A despawned entity is structurally gone: slot recycled, generation incremented. Despawn happens during reset() (clear all before respawning) or for transient entities (projectiles leaving the arena).

Propagators write `properties[alive_property] = 0.0` for death. They never emit Spawn/Move/Despawn — those are exclusively external commands (from the RL agent or reset logic).

### Scope limitations

- **RealtimeAsyncWorld:** Entity support is deferred. Entity commands return `UnsupportedCommand` in async mode. Echelon uses LockstepWorld through B7.
- **murk-replay:** Replay format version must be bumped. CommandPayload serialization changes are breaking. Old replay files containing Move/Spawn/Despawn commands will not deserialize correctly. Add backward-compatible deserialization or document incompatibility.

## 4. murk-propagator Integration

### StepContext entity access

```rust
pub struct StepContext<'a> {
    // ... existing fields (reads, reads_previous, writes, scratch, space, tick_id, dt) ...
    entity_snapshot: Option<&'a EntitySnapshot<'a>>,
    entity_staging: Option<&'a mut PropertyStaging>,
}
```

Split-borrow pattern: `entity_snapshot` and `entity_staging` point to different allocations, so they can be borrowed independently.

```rust
impl<'a> StepContext<'a> {
    /// Read-only entity access (tick-start state, Jacobi).
    /// Returns None if entities are disabled (max_entities == 0).
    pub fn entities(&self) -> Option<&EntitySnapshot<'a>> { ... }

    /// Euler-style entity reads: sees prior propagators' property writes
    /// overlaid on tick-start snapshot.
    /// Returns None if entities are disabled.
    pub fn entities_overlaid(&self) -> Option<EntityOverlayReader<'_, '_>> { ... }

    /// Read-only entity access, tick-start state. Alias for entities().
    /// Naming mirrors reads_previous() for cell fields.
    pub fn entities_previous(&self) -> Option<&EntitySnapshot<'a>> { ... }

    /// Write entity properties to staging buffer.
    /// Returns None if entities are disabled.
    pub fn entity_writes(&mut self) -> Option<&mut PropertyStaging> { ... }
}
```

All accessors return `Option` when `max_entities == 0`. A propagator that calls `.unwrap()` has documented its entity requirement. A propagator that handles `None` can run in entity-disabled worlds.

Naming follows the cell field pattern: `entities()` / `entities_previous()` / `entities_overlaid()` mirrors `reads()` / `reads_previous()` (with overlay for Euler). No "euler"/"jacobi" terminology in method names.

**Exclusive-borrow constraint:** `entities_overlaid()` borrows `entity_staging` immutably (reborrow of `&mut` as `&`). `entity_writes()` borrows it mutably. These cannot be held simultaneously. This is the desired behavior (no overlapping mutable access) and matches the field reads/writes pattern. Document in method docstrings.

### Propagator trait extensions

```rust
pub trait Propagator: Send + Sync {
    // ... existing methods (name, step, reads, reads_previous, writes, max_dt) ...

    /// Entity properties this propagator reads with Euler semantics
    /// (sees prior propagators' writes this tick).
    /// Prefer reads_entities_previous() unless you need intra-tick pipeline ordering.
    fn reads_entities(&self) -> &[PropertyIndex] { &[] }

    /// Entity properties this propagator reads with Jacobi semantics
    /// (tick-start only, ignores this tick's writes).
    /// Default choice — use this unless you specifically need intra-tick writes.
    fn reads_entities_previous(&self) -> &[PropertyIndex] { &[] }

    /// Entity properties this propagator writes.
    fn writes_entities(&self) -> &[PropertyIndex] { &[] }
}
```

Default empty impls: existing propagators (cell-field-only) require no changes. The engine uses `writes_entities()` for write-conflict validation and execution ordering — same mechanism as cell fields.

**Note:** The `Sync` bound is a breaking change from the current `Send + 'static`. This is the v0.2 breaking change flagged in the three-port architecture review. All existing propagators have been audited as Sync-safe. This spec makes the change explicit rather than adding it silently.

### Euler + Jacobi entity properties

Both read modes are supported, mirroring cell fields:

- **Jacobi** (`ctx.entities()` / `reads_entities_previous()`): reads frozen tick-start snapshot. No ordering dependency between propagators. Trivially parallelizable. **This is the default choice.**
- **Euler** (`ctx.entities_overlaid()` / `reads_entities()`): reads overlay that checks PropertyStaging first, falls back to snapshot. Sees prior propagator's property writes this tick. Creates pipeline ordering.

The engine enforces execution order for entity property Euler reads using the same mechanism as cell field Euler reads (propagator pipeline ordering based on declared reads/writes).

### Example: death propagator

```rust
fn step(&self, ctx: &mut StepContext) -> Result<(), PropagatorError> {
    let snapshot = ctx.entities().unwrap();
    let ids_to_kill: SmallVec<[EntityId; 4]> = snapshot
        .iter_alive()
        .filter(|e| snapshot.property(e.id, HP).unwrap_or(0.0) <= 0.0)
        .map(|e| e.id)
        .collect();

    let staging = ctx.entity_writes().unwrap();
    for id in ids_to_kill {
        staging.set(id, ALIVE, 0.0);
    }
    Ok(())
}
```

Uses `ctx.entities()` (Jacobi) — correct because the death propagator only needs tick-start state. Does NOT use `entities_overlaid()` because it has no ordering dependency on prior propagators' entity writes.

## 5. murk-slot Crate

New crate. Produces fixed-shape entity-slot tensors from entity snapshots. No per-step Python logic. Depends on `murk-entity` and `murk-space` (for GridGeometry). Does NOT depend on `murk-obs`.

### SlotSpec

```rust
/// Specification for entity-slot observation extraction.
///
/// Compiled once against a world's space topology, executed every step.
/// Output shape is deterministic from the spec regardless of actual entity count.
pub struct SlotSpec {
    /// Spatial region to scan for entities. `ObsRegion::Fixed(RegionSpec::All)`
    /// scans all entities (appropriate for small entity counts like Echelon's 24).
    /// `ObsRegion::AgentDisk` / `AgentRect` enables spatial culling for large
    /// entity counts.
    pub scan_region: ObsRegion,
    /// Observer's own properties, prefixed to the output before contact slots.
    /// Enables self-state extraction without per-step Python logic.
    /// Note: RelativeTo is disallowed here (would always produce 0.0).
    /// Validated at compile time.
    pub self_properties: Vec<PropertyExtract>,
    /// Entity groups with fill ordering. Output slots concatenated in group order.
    pub groups: Vec<SlotGroup>,
    /// Properties to extract per entity slot.
    pub extract_properties: Vec<PropertyExtract>,
}
```

### SlotGroup

```rust
pub struct SlotGroup {
    /// Human-readable name (for debugging/logging).
    pub name: String,
    /// Which entities belong in this group.
    pub predicate: SlotPredicate,
    /// Maximum slots for this group. Output is zero-padded if fewer entities match.
    pub max_slots: usize,
    /// How to rank entities for slot assignment.
    pub fill_order: FillOrder,
    /// When this group has fewer matches than max_slots, surplus slots are
    /// offered to this group (index into `SlotSpec::groups`).
    /// Validated at compile time: must be in-bounds, acyclic, and single-pass
    /// (overflow does NOT cascade — A→B fills B's surplus but B does not
    /// then overflow to C even if B has its own overflow_to).
    /// Overflow entities are sorted by the SOURCE group's fill_order.
    /// If the target group is already full, overflow entities are discarded.
    pub overflow_to: Option<usize>,
}
```

### SlotPredicate

```rust
/// Predicates over a single entity's properties. By design, SlotPredicate
/// cannot express inter-entity relationships (joins) — those belong in
/// consumer-layer logic, not the observation framework.
///
/// v1 provides `And` as the only combinator. `Or` and `Not` are deferred —
/// use multiple groups with overflow_to for disjunctive patterns.
pub enum SlotPredicate {
    /// All alive entities.
    Alive,
    /// Entity type matches.
    EntityType(u32),
    /// Property value equals (exact f32 comparison, not epsilon).
    /// For categorical values stored as float (e.g., team=2.0).
    PropertyEq(PropertyIndex, f32),
    /// Property value in range [min, max] inclusive.
    PropertyRange(PropertyIndex, f32, f32),
    /// Logical AND of predicates.
    And(Vec<SlotPredicate>),
}
```

### FillOrder

```rust
pub enum FillOrder {
    /// Nearest to observer first (by spatial distance through space topology).
    NearestFirst,
    /// Farthest from observer first.
    FarthestFirst,
    /// Ascending by property value.
    PropertyAscending(PropertyIndex),
    /// Descending by property value.
    PropertyDescending(PropertyIndex),
    /// Stable ordering by EntityId — same entity always in same slot position
    /// if alive. Provides temporal consistency for value function learning.
    /// Note: if the policy uses sinusoidal positional encoding on the slot
    /// dimension, StableId creates an implicit entity-to-position mapping.
    /// For policies with no positional encoding, StableId is strictly better
    /// than NearestFirst for temporal stability.
    StableId,
}
```

### PropertyExtract

Replaces raw `PropertyIndex` for extraction. Enables relative transforms, normalization, angular encoding, and one-hot encoding in Rust — eliminates per-step Python arithmetic.

```rust
pub enum PropertyExtract {
    /// Raw property value.
    Raw(PropertyIndex),
    /// Property value minus observer's value (e.g., relative position).
    /// Uses shortest-path distance through the space topology for coordinate
    /// properties (handles toroidal wrap correctly).
    RelativeTo(PropertyIndex),
    /// Property value divided by a constant (e.g., HP/max_hp).
    Normalized(PropertyIndex, f32),
    /// Sine and cosine of a property interpreted as radians.
    /// Expands to 2 output elements: [sin(θ), cos(θ)].
    /// For periodic angular properties (heading, bearing) where
    /// Normalized would create a discontinuity at the wrap point.
    SinCos(PropertyIndex),
    /// One-hot encoding of a categorical property.
    /// Expands to n_categories output elements.
    /// Value is floored to integer and clamped to [0, n_categories-1].
    /// Out-of-range values produce a zero vector (all zeros).
    OneHot(PropertyIndex, u32),
}
```

`output_len` accounts for expansion: `Raw`/`RelativeTo`/`Normalized` contribute 1 element, `SinCos` contributes 2, `OneHot(_, n)` contributes `n`.

`RelativeTo` in `self_properties` is disallowed at compile time (would always produce 0.0).

### SlotPlan (compiled)

```rust
pub struct SlotPlan {
    spec: SlotSpec,
    geometry: Option<GridGeometry>,  // from murk-space
    output_len: usize,               // self_len + total_slots * extract_width
    self_len: usize,                  // number of floats in self-state prefix
    slot_len: usize,                  // number of floats per contact slot
    mask_len: usize,                  // total_slots (self-state has no mask)
    compiled_generation: WorldGenerationId,
    space_topology_hash: u64,         // for batch topology validation
}
```

- `compile(spec, space)` — validates spec against space topology, pre-computes scan geometry. Validates: overflow_to in-bounds + acyclic + single-pass. Validates: RelativeTo not in self_properties. Validates: PropertyIndex values against manifest. Validates: SlotPredicate PropertyIndex values against manifest.
- `output_len()` / `self_len()` / `slot_len()` / `mask_len()` — deterministic from spec. Caller pre-allocates.
- `space_topology_hash` — for batch API topology validation.
- Generation tracking for `PlanInvalidated` error on world resize.

### Execution

```rust
impl SlotPlan {
    /// Execute for N agents. Reads observer positions from the entity snapshot.
    /// Observer must be alive — dead/unknown observer fills output with zeros
    /// and mask with all-false, with a flag in SlotMetadata.
    pub fn execute_agents(
        &self,
        entity_snapshot: &EntitySnapshot,
        space: &dyn Space,
        agent_ids: &[EntityId],      // which entities are observing
        output: &mut [f32],          // (n_agents * output_len)
        mask: &mut [u8],             // (n_agents * mask_len) — 0/1 values, use np.bool_
    ) -> Result<Vec<SlotMetadata>, ObsError>;
}
```

Per agent:
1. Look up observer via `entity_snapshot.get(agent_id)`. If `None` (stale ID, unknown, or dead): zero-fill output, zero-fill mask, set `metadata.observer_dead = true`, continue to next agent.
2. Extract `self_properties` for the observer, write to output prefix.
3. Scan `scan_region` for entities (iterate all if `RegionSpec::All`, spatial cull otherwise).
4. For each group: filter by predicate, partial sort by fill_order (`select_nth_unstable_by` — O(n) average), take top `max_slots`.
5. Process `overflow_to` (single-pass): if a group has unfilled slots, offer surplus to target group (sorted by source group's fill_order). If target is full, discard.
6. For each matched entity: apply `extract_properties` (Raw/RelativeTo/Normalized/SinCos/OneHot), write to output.
7. Zero-fill unused slots, set mask=0 for empty, mask=1 for filled.
8. Deterministic tie-breaking: canonical EntityId order when sort keys are equal.

**Scratch buffers:** Thread-local pools. Sort scratch is `O(entities_in_scan_region)` per agent, reused across agents on the same thread. After first-step warmup, zero allocations in hot path.

**NaN/Inf validation:** `#[cfg(debug_assertions)]` scan of output slice after execution. Returns `ObsError` with entry name and first-bad-index if non-finite values detected.

### SlotMetadata

```rust
pub struct SlotMetadata {
    pub tick_id: TickId,
    pub entities_scanned: u32,
    /// Per-group match counts (before overflow).
    pub entities_matched: Vec<u32>,
    /// True if the observer entity was dead, despawned, or had a stale ID.
    pub observer_dead: bool,
}
```

## 6. FFI and Python Bindings

Follows the existing ObsPlan FFI pattern with explicit divergences noted.

### C FFI layer (murk-ffi)

New handle table for SlotPlan. All buffer parameters validated with `checked_mul` for overflow, matching the existing ObsPlan pattern (`obs.rs:275-290`).

```
murk_slotplan_compile(world_h, spec_ptr, spec_len) → status, plan_h
murk_slotplan_execute_agents(world_h, plan_h, agent_ids_ptr, n_agents, output_ptr, output_len, mask_ptr, mask_len, results_ptr) → status
murk_slotplan_execute_agents_batch(plan_h, world_handles_ptr, n_worlds, agent_ids_ptr, n_agents_per_world, output_ptr, output_len, mask_ptr, mask_len, results_ptr) → status
murk_slotplan_output_len(plan_h) → i64
murk_slotplan_self_len(plan_h) → i64
murk_slotplan_slot_len(plan_h) → i64
murk_slotplan_mask_len(plan_h) → i64
murk_slotplan_output_shape(plan_h, n_worlds, n_agents) → (i64, i64, i64, i64)
murk_slotplan_destroy(plan_h)
```

**Batch topology validation:** `execute_agents_batch` validates each world's space topology hash against the compiled plan's `space_topology_hash`. Returns `PlanInvalidated` for mismatches. Cost: one hash comparison per world.

**Buffer validation:** All FFI functions validate buffer sizes with `checked_mul` before constructing slices from raw pointers, matching the existing ObsPlan defensive pattern.

### PyO3 layer (murk-python)

**Divergence from ObsPlan:** SlotPlan mask uses `PyArray1<bool>` (np.bool_), NOT `PyArray1<u8>`. This is intentional — PyTorch's `key_padding_mask` requires BoolTensor, and uint8→bool conversion copies 28 GB/sec at scale.

```python
# Predicate factory functions
class Predicate:
    @staticmethod
    def alive() -> Predicate: ...
    @staticmethod
    def entity_type(t: int) -> Predicate: ...
    @staticmethod
    def property_eq(prop: int, value: float) -> Predicate: ...
    @staticmethod
    def property_range(prop: int, min: float, max: float) -> Predicate: ...
    def __and__(self, other: Predicate) -> Predicate: ...

# FillOrder factory functions
class FillOrder:
    @staticmethod
    def nearest_first() -> FillOrder: ...
    @staticmethod
    def farthest_first() -> FillOrder: ...
    @staticmethod
    def property_ascending(prop: int) -> FillOrder: ...
    @staticmethod
    def property_descending(prop: int) -> FillOrder: ...
    @staticmethod
    def stable_id() -> FillOrder: ...

# PropertyExtract factory functions
class Extract:
    @staticmethod
    def raw(prop: int) -> Extract: ...
    @staticmethod
    def relative_to(prop: int) -> Extract: ...
    @staticmethod
    def normalized(prop: int, max_value: float) -> Extract: ...
    @staticmethod
    def sin_cos(prop: int) -> Extract: ...
    @staticmethod
    def one_hot(prop: int, n_categories: int) -> Extract: ...

class SlotEntry:
    """Single slot group specification."""
    def __init__(self, name, predicate, max_slots, fill_order, overflow_to=None): ...

class SlotPlan:
    """Compiled entity-slot observation plan.

    Pre-allocate mask as dtype=np.bool_ (not uint8) for zero-copy
    attention masking with PyTorch's nn.MultiheadAttention.

    For CUDA training, pre-allocate output and mask with
    torch.zeros(..., pin_memory=True).numpy() for async DMA transfer.
    Use tensor.to(device, non_blocking=True) for async host→device copy.

    WARNING: torch.from_numpy() aliases the buffer. Do not call
    execute_agents into the same buffer while a tensor view of that
    buffer is still referenced upstream (e.g., in a replay buffer).
    """
    def __init__(self, world, entries, extract_properties,
                 self_properties=None, scan_region=RegionType.All): ...

    def execute_agents(self, world, agent_ids, output, mask) -> list[SlotMetadata]:
        """Execute for N agents in a single world.

        WARNING: Buffer aliasing — this writes directly into the numpy buffer.
        Do not hold tensor views of the same buffer from a previous step.

        Args:
            world: The world to observe (provides current entity snapshot).
            agent_ids: 1-D uint32 array of observing entity IDs.
            output: Pre-allocated C-contiguous float32 array.
            mask: Pre-allocated C-contiguous bool array (NOT uint8).
        """
        ...

    def execute_agents_batch(self, worlds, agent_ids, output, mask) -> list[SlotMetadata]:
        """Execute across multiple worlds in a single call.

        One Python→Rust call, GIL released once, Rust iterates all worlds.
        Each world's space topology is validated against the compiled plan.
        This is the training hot path for vectorized environments.

        Args:
            worlds: List of world handles (all envs).
            agent_ids: 2-D uint32 array of shape (n_worlds, n_agents_per_world).
            output: Pre-allocated C-contiguous float32 array of shape
                    (n_worlds * n_agents, output_len).
            mask: Pre-allocated C-contiguous bool array of shape
                  (n_worlds * n_agents, mask_len).

        Example shapes:
            output_shape(1024, 1) → (1024, output_len)
            output_shape(1024, 4) → (1024, 4, output_len)
        """
        ...

    @property
    def output_len(self) -> int: ...
    @property
    def self_len(self) -> int: ...
    @property
    def slot_len(self) -> int: ...
    @property
    def mask_len(self) -> int: ...
    def output_shape(self, n_worlds=1, n_agents=1) -> tuple[int, ...]:
        """Return the full output shape for the given world/agent counts.

        Callers should use this instead of hand-computing reshape dimensions.
        """
        ...

    def destroy(self): ...
    def __enter__(self): ...
    def __exit__(self, ...): ...
```

### BatchedEngine integration

Primary API for vectorized training. `BatchedEngine` gains SlotPlan support:

```rust
pub fn step_and_observe_slots(
    &mut self,
    commands: &[Vec<Command>],
    obs_output: &mut [f32],
    obs_mask: &mut [u8],
    slot_output: &mut [f32],
    slot_mask: &mut [u8],
    agent_ids: &[EntityId],
) -> Result<BatchResult, BatchError>;
```

This combines field observations and entity-slot observations in a single GIL-releasing call. The freestanding `execute_agents_batch` remains as a lower-level escape hatch for consumers who don't use `BatchedEngine`.

### Parallelism

`execute_agents_batch` and `step_and_observe_slots` iterate worlds sequentially in M5. Internal parallelism via Rayon is the intended future optimization but is not required for initial delivery — sequential iteration at 1024 worlds is correct-first, profile-then-parallelize.

GIL released once for the entire batch call. Buffer validation (C-contiguous checks, shape checks) happens before GIL release.

## 7. EntityProjection Propagator

Optional convenience propagator in `murk-propagators`. Projects entity state into cell fields for **rendering, debugging, and downstream cell-based propagators that were written before entity access existed.** This is a visualization bridge, not a data access pathway — new propagators should use `ctx.entities()` / `ctx.entities_overlaid()` directly.

```rust
/// Projects entity properties into cell fields.
///
/// For each alive entity at a cell, writes selected properties into
/// designated cell fields. Multi-entity-per-cell: last-write-wins,
/// ordered by EntityId for determinism.
///
/// NOTE: This propagator exists for rendering and backwards compatibility.
/// New propagators should read entities directly via StepContext, not
/// through projected cell fields.
pub struct EntityProjection {
    /// Which entity types to project (None = all alive entities).
    entity_types: Option<Vec<u32>>,
    /// Property → cell field mappings.
    projections: Vec<(PropertyIndex, FieldId)>,
    /// Value written to cells with no matching entity.
    absent_value: f32,  // typically 0.0
}
```

Implementation: ~50 lines. Clear all projected fields to `absent_value`, then iterate alive entities (filtered by type), write property values to cell fields at the entity's coordinate.

## Milestones

Each milestone is independently testable and produces a concrete artifact that the next milestone builds on. Exit criteria include specific test assertions for AI agent executability.

### M1: murk-core + murk-entity (foundation)

**Crates:** murk-core (modifications), murk-entity (new)

**Deliverables:**
- `EntityId` with packed slot+generation, `PropertyIndex(u32)`, `EntityManifest` in murk-core
- Command payload changes (EntityId, property_overrides, entity_type)
- New error variants (UnknownEntity, EntityCapacityFull)
- `EntityRecord` (no alive field), `EntityStore` (flat slab properties), `EntitySnapshot` in murk-entity
- `PropertyStaging` (Vec<u64> bitset, no BitVec), `EntityOverlayReader` (two lifetimes) in murk-entity
- Unit tests for all entity store operations

**Exit criteria:**
- Spawn 24 entities, verify each receipt contains a unique EntityId with generation 0
- Spawn at max_entities + 1: assert `EntityCapacityFull`
- Despawn entity, spawn new entity into recycled slot: assert new EntityId has generation 1
- Attempt Move/Despawn with old (generation 0) EntityId: assert `UnknownEntity`
- Kill entity by writing `properties[ALIVE] = 0.0`: `iter_alive()` count decreases, `iter_all()` count unchanged
- Despawn all, re-spawn: verify free list recycling, all IDs have incremented generations
- coord_index: after Move, old coord has no reference, new coord has entity
- EntityManifest defaults applied on spawn; property_overrides override defaults
- Property read with out-of-range PropertyIndex returns None (not panic)
- PropertyStaging set/get round-trip; reset() clears all writes
- EntityOverlayReader returns staged value when present, snapshot value when not

### M2: murk-engine integration

**Crates:** murk-engine (modifications)

**Deliverables:**
- Entity store initialization from WorldConfig
- Spawn/Despawn/Move command handling in execute_tick()
- Entity snapshot/restore rollback
- `spawned_entity_id` on Receipt
- `Option<EntitySnapshot>` in StepResult
- WorldConfig `max_entities` + `entity_manifest` with ConfigError validation
- Integration tests

**Exit criteria:**
- Spawn/Despawn/Move through LockstepWorld, receipt returns EntityId with generation
- `max_entities = 0` with Spawn command: assert `UnsupportedCommand` (backward compat)
- `max_entities > 0` with `entity_manifest = None`: assert `ConfigError::EntityManifestRequired`
- Move to out-of-bounds coord: assert `NotApplied`
- Move dead entity (alive=0): assert `UnknownEntity`
- Two Spawns in same tick: both succeed, receipts have distinct IDs
- Rollback on propagator failure: entity store fully restored (entity gone, coord_index clean, free list restored, generation reverted to pre-tick state)
- Multi-command tick (Spawn + Move + Despawn): applied in order within tick
- StepResult.entity_snapshot is Some when max_entities > 0, None when 0

### M3: murk-propagator integration

**Crates:** murk-propagator (modifications)

**Deliverables:**
- `entity_snapshot` + `entity_staging` in StepContext (both Option)
- `entities()`, `entities_overlaid()`, `entities_previous()`, `entity_writes()` methods returning Option
- `reads_entities()`, `reads_entities_previous()`, `writes_entities()` on Propagator trait
- Entity property staging populated per-propagator based on read resolution plan
- Euler overlay reader constructed when propagator declares reads_entities()
- `Propagator: Send + Sync` bound (explicit breaking change)
- Integration tests with multi-propagator pipeline

**Exit criteria:**
- Movement propagator writes entity properties via `ctx.entity_writes().unwrap().set()`
- LOS propagator reads positions via `ctx.entities_overlaid().unwrap()` (Euler, sees movement's writes)
- Death propagator using `ctx.entities().unwrap()` (Jacobi) does NOT see movement's entity property writes
- PropertyStaging committed to entity store after all propagators complete (step 8)
- PropertyStaging reset before next tick (second tick sees clean staging)
- Write-conflict: two propagators both declaring `writes_entities(&[PROP_X])` fails at pipeline construction (not at runtime)
- Existing cell-only propagators unchanged (entities() returns None, no crash)
- Propagator calling entities() on max_entities=0 world gets None

### M4: murk-slot (entity-slot observations)

**Crates:** murk-slot (new), murk-space (GridGeometry extraction)

**Deliverables:**
- GridGeometry extracted from murk-obs to murk-space
- SlotSpec, SlotGroup, SlotPredicate, FillOrder, PropertyExtract (including SinCos)
- SlotPlan compilation and execution
- Self-state extraction (self_properties)
- Relative transforms (RelativeTo with topology-aware shortest path), normalization, SinCos, OneHot
- Overflow backfill (overflow_to) with cycle detection
- Partial sort via select_nth_unstable_by
- Thread-local scratch buffer pools
- NaN/Inf debug validation
- Deterministic tie-breaking by EntityId
- Unit tests for all extraction paths

**Exit criteria:**
- SlotPlan with 3 groups (hostile/friendly/projectile), overflow backfill, execute_agents produces correct fixed-shape output
- Self-state prefix matches observer's properties (verified against known values)
- RelativeTo produces correct relative coordinates (verified numerically)
- SinCos(heading) produces [sin(h), cos(h)] (verified at 0, π/2, π, 3π/2)
- OneHot(class, 4) with value 2.0 produces [0, 0, 1, 0]; value 5.0 produces [0, 0, 0, 0] (out-of-range)
- RelativeTo in self_properties: compile error
- overflow_to circular reference (A→B→A): compile error
- overflow_to invalid index: compile error
- overflow_to target already full: surplus entities discarded, not error
- overflow_to single-pass: A→B fills B's surplus, B does NOT cascade to C
- Dead/unknown observer (stale EntityId): zero output, zero mask, metadata.observer_dead = true
- Zero alive entities in scan region: all slots zero-filled, mask all-false
- Deterministic tie-breaking: 4 entities at same distance, same properties → slot assignment matches ascending EntityId; repeat gives identical result
- NaN in entity property triggers ObsError in debug mode
- PlanInvalidated on world generation mismatch
- Benchmark: zero allocations after first-step warmup (counting allocator test)

### M5: FFI + Python bindings

**Crates:** murk-ffi (modifications), murk-python (modifications), murk-engine (BatchedEngine extension)

**Deliverables:**
- C FFI handle table for SlotPlan with checked_mul buffer validation
- All murk_slotplan_* C functions
- PyO3 SlotPlan class with execute_agents and execute_agents_batch
- Python factory functions for Predicate, FillOrder, Extract
- output_shape(), self_len, slot_len helpers
- Bool mask dtype (PyArray1<bool>)
- BatchedEngine.step_and_observe_slots
- Batch topology validation (space_topology_hash per world)
- Documentation (pinned memory pattern, buffer aliasing warning, non_blocking=True)

**Exit criteria:**
- execute_agents_batch across 4 worlds, GIL released, output goes to `torch.from_numpy()` with no per-step Python logic
- Mask dtype is np.bool_; passing uint8 array is rejected with TypeError
- Fortran-contiguous array rejected with ValueError
- output_shape(4, 22) returns correct tuple
- self_len + (total_slots * slot_len) == output_len
- Invalid agent_id (stale generation) returns zero output, not panic/segfault
- Batch with world having different space topology: PlanInvalidated error
- Handle use-after-destroy: returns RuntimeError
- Python Predicate factory: `Predicate.entity_type(0) & Predicate.alive()` constructs And predicate
- Python Extract factory: `Extract.sin_cos(HEADING)` constructs SinCos variant

### M6: EntityProjection propagator + replay format

**Crates:** murk-propagators (modification), murk-replay (modification)

**Deliverables:**
- EntityProjection propagator
- murk-replay format version bump
- Replay codec updated for new CommandPayload variants
- Unit tests

**Exit criteria:**
- EntityProjection writes entity positions to cell fields
- Cells without entities get absent_value; cells with dead entities get absent_value
- Multi-entity-per-cell uses deterministic EntityId ordering (last-write-wins, ascending)
- Replay codec serializes/deserializes new Spawn (entity_type, property_overrides) correctly
- Replay format version incremented
- Optional — can ship any time after M3

## Design Decisions Log

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Entity ID format | `EntityId(u32)` with 20-bit slot + 12-bit generation | Closes stale-ID use-after-free in observation and propagator APIs. `get()` validates generation. 1M slots, 4096 gens/slot. Packing is a bit shift — negligible cost. |
| Aliveness representation | Single source: `properties[manifest.alive_property]` | No separate `alive: bool` on EntityRecord. Eliminates sync divergence between two representations. `iter_alive()` reads the property directly. |
| Entity storage | Separate EntityStore with flat SoA property slab | Arena untouched. Properties as `Box<[f32]>` slab (not per-record `Box<[f32]>`). Cache-friendly: 4.3 KB for Echelon fits in L1. Same layout as PropertyStaging. |
| PropertyStaging bitset | `Vec<u64>` (6 words for Echelon) | No BitVec crate dependency. 360 bits = 48 bytes. Stack-friendly. |
| EntityOverlayReader lifetimes | Two parameters: `<'snap, 'staging>` | Avoids `&'a T<'a>` invariance footgun that causes compiler errors. |
| Rollback mechanism | Entity store snapshot/restore before tick | ~6 KB memcpy for Echelon. Simpler than per-mutation undo (especially Despawn which needs full record + generation + coord_index). |
| Entity accessor API | Returns `Option<_>`, not panic | Enables propagator reuse across entity/non-entity worlds. Matches internal `Option<EntityStore>`. |
| Naming convention | `entities()` / `entities_previous()` / `entities_overlaid()` | Mirrors `reads()` / `reads_previous()` for cell fields. No Euler/Jacobi terminology in method names. |
| Spatial interaction | Entities are overlay-only | Entities exist at coordinates but don't automatically write cell fields. EntityProjection is opt-in visualization bridge, not data access pathway. |
| Entity capacity | Fixed at world creation | Matches arena's fixed-capacity model. O(1) spawn/despawn with flat array + free list. Deterministic memory. RL environments have known entity budgets. |
| Entity property model | Homogeneous (all entities same property count) | Echelon: ~960 bytes wasted on unused projectile properties. Negligible. Trigger for heterogeneous: if propagators branch on entity_type to interpret PropertyIndex differently. |
| Death mechanism | Property write (alive_property=0), not Despawn | Dead entities retain state (position for debris, alive=0 for observation). Despawn = structural removal. |
| Entity property reads | Euler + Jacobi (both supported) | Marginal cost over Jacobi-only (~80 lines, ~22ns/tick). Consistent with cell field model. Jacobi is the documented default. |
| Propagator trait | `Send + Sync` (breaking from `Send + 'static`) | Required for future parallel propagator execution. Explicit v0.2 breaking change. All existing impls audited as Sync-safe. |
| Predicate scope | Single-entity only, And combinator only | Cannot express inter-entity joins or Or/Not. By design — joins and disjunctions use multiple groups. |
| PropertyEq semantics | Exact f32 comparison | For categorical values stored as float. Not epsilon comparison. |
| Slot backfill | overflow_to: Option<usize> on SlotGroup | Acyclic, single-pass, validated at compile time. Source group's fill_order applies to overflow entities. |
| Observer position source | agent_ids (EntityId, read from snapshot) | One source of truth. Generation validation on lookup. Dead observer → zero output. |
| Batch API | BatchedEngine integration + freestanding escape hatch | Primary: step_and_observe_slots on BatchedEngine. Secondary: execute_agents_batch for non-BatchedEngine consumers. |
| Mask dtype | np.bool_ (not uint8) | PyTorch's key_padding_mask requires BoolTensor. SlotPlan diverges from ObsPlan (which uses uint8). |
| Property transforms | PropertyExtract enum (Raw, RelativeTo, Normalized, SinCos, OneHot) | Eliminates per-step Python arithmetic. SinCos for periodic angles. RelativeTo uses topology-aware shortest path. OneHot clamps out-of-range to zero vector. |
| Batch topology validation | space_topology_hash per world | One hash comparison per world. Catches misconfigured environments in vectorized training. |
| coord_index and dead entities | coord_index includes dead entities | Callers must filter by alive property. Documented contract. |
| Move on dead entity | Returns UnknownEntity | Dead entities should not be moved. Same as despawned from command perspective. |
| RealtimeAsync entities | Deferred | Entity commands return UnsupportedCommand in async mode. Echelon uses Lockstep through B7. |
| Parallelism | Sequential in M5, Rayon future optimization | Correct-first, profile-then-parallelize. GIL released once for entire batch. |
