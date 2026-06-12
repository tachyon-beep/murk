# Entity Model M2 Engine Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire the M1 entity store into lockstep engine execution so spawn, move, despawn, entity snapshots, and staged entity property writes work during ticks.

**Architecture:** `murk-engine` owns an optional `EntityStore` and `PropertyStaging` when `WorldConfig::max_entities > 0`. `murk-propagator` receives optional entity snapshot/staging access through `StepContext`, while existing field-only propagators remain source-compatible through default entity dependency methods. `RealtimeAsyncWorld` intentionally remains entity-disabled for M2 and returns `UnsupportedCommand` for entity commands.

**Tech Stack:** Rust 2021, `murk-core`, `murk-entity`, `murk-engine`, `murk-propagator`, TDD with `cargo test`, `cargo clippy`, and `wardline`.

---

## File Structure

| File | Change | Responsibility |
|------|--------|----------------|
| `crates/murk-engine/Cargo.toml` | Modify | Add dependency on `murk-entity` |
| `crates/murk-engine/src/config.rs` | Modify | Add `max_entities`, `entity_manifest`, builder methods, and validation |
| `crates/murk-engine/src/tick.rs` | Modify | Own entity store/staging, apply entity commands, rollback entity mutations, publish entity snapshot |
| `crates/murk-engine/src/lockstep.rs` | Modify | Add `entity_snapshot` to `StepResult` |
| `crates/murk-engine/src/realtime.rs` | Modify | Preserve entity-disabled async behavior during config reconstruction |
| `crates/murk-propagator/Cargo.toml` | Modify | Add dependency on `murk-entity` |
| `crates/murk-propagator/src/context.rs` | Modify | Add optional entity accessors to `StepContext` |
| `crates/murk-propagator/src/propagator.rs` | Modify | Add default entity read/write declarations |
| `crates/murk-propagator/src/pipeline.rs` | Modify | Validate entity property conflicts and ordering declarations |
| `crates/murk-core/src/command.rs` | Modify | Add `spawned_entity_id` to `Receipt` |

## Task 1: WorldConfig Entity Configuration

**Files:**
- Modify: `crates/murk-engine/Cargo.toml`
- Modify: `crates/murk-engine/src/config.rs`

- [ ] **Step 1: Write failing config tests**

Add these tests inside `crates/murk-engine/src/config.rs` test module:

```rust
#[test]
fn builder_accepts_entity_manifest_when_capacity_positive() {
    let config = WorldConfig::builder()
        .space(Box::new(murk_space::Line1D::new(4, murk_space::EdgeBehavior::Absorb).unwrap()))
        .fields(vec![scalar_field("energy")])
        .propagators(vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))])
        .dt(0.1)
        .max_entities(8)
        .entity_manifest(murk_core::EntityManifest {
            property_names: vec!["alive".into(), "hp".into()],
            property_defaults: vec![1.0, 100.0],
            alive_property: murk_core::PropertyIndex(0),
        })
        .build()
        .unwrap();

    assert_eq!(config.max_entities(), 8);
    assert_eq!(config.entity_manifest().unwrap().property_count(), 2);
}

#[test]
fn positive_entity_capacity_requires_manifest() {
    let result = WorldConfig::builder()
        .space(Box::new(murk_space::Line1D::new(4, murk_space::EdgeBehavior::Absorb).unwrap()))
        .fields(vec![scalar_field("energy")])
        .propagators(vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))])
        .dt(0.1)
        .max_entities(8)
        .build();

    assert!(matches!(result, Err(ConfigError::EntityManifestRequired)));
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p murk-engine config::tests::positive_entity_capacity_requires_manifest -- --nocapture`

Expected: compile failure naming missing `max_entities`, `entity_manifest`, and `EntityManifestRequired`.

- [ ] **Step 3: Implement config fields and validation**

In `crates/murk-engine/Cargo.toml`, add:

```toml
murk-entity = { path = "../murk-entity", version = "0.1.9" }
```

In `ConfigError`, add:

```rust
/// `max_entities` is positive but no entity manifest was supplied.
EntityManifestRequired,
/// Entity manifest validation failed.
InvalidEntityManifest {
    /// Description of the validation failure.
    reason: String,
},
```

In `WorldConfig`, add:

```rust
pub(crate) max_entities: u32,
pub(crate) entity_manifest: Option<murk_core::EntityManifest>,
```

In `WorldConfig::validate()`, before pipeline validation:

```rust
if self.max_entities > 0 {
    let manifest = self
        .entity_manifest
        .as_ref()
        .ok_or(ConfigError::EntityManifestRequired)?;
    manifest
        .validate()
        .map_err(|err| ConfigError::InvalidEntityManifest {
            reason: err.to_string(),
        })?;
}
```

Add accessors and builder methods:

```rust
pub fn max_entities(&self) -> u32 { self.max_entities }
pub fn entity_manifest(&self) -> Option<&murk_core::EntityManifest> {
    self.entity_manifest.as_ref()
}
pub fn max_entities(mut self, max_entities: u32) -> Self {
    self.max_entities = max_entities;
    self
}
pub fn entity_manifest(mut self, manifest: murk_core::EntityManifest) -> Self {
    self.entity_manifest = Some(manifest);
    self
}
```

Initialize both fields in `WorldConfig::builder()` and `WorldConfigBuilder::build()`.

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p murk-engine config::tests::builder_accepts_entity_manifest_when_capacity_positive config::tests::positive_entity_capacity_requires_manifest -- --nocapture`

Expected: both tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/murk-engine/Cargo.toml crates/murk-engine/src/config.rs Cargo.lock
git commit -m "feat(engine): add entity configuration to WorldConfig"
```

## Task 2: Receipt and StepResult Entity Output

**Files:**
- Modify: `crates/murk-core/src/command.rs`
- Modify: `crates/murk-engine/src/lockstep.rs`

- [ ] **Step 1: Write failing receipt and StepResult tests**

In `crates/murk-core/src/command.rs`, add:

```rust
#[test]
fn receipt_can_carry_spawned_entity_id() {
    let id = EntityId::new(2, 1);
    let receipt = Receipt {
        accepted: true,
        applied_tick_id: Some(TickId(4)),
        reason_code: None,
        command_index: 0,
        spawned_entity_id: Some(id),
    };
    assert_eq!(receipt.spawned_entity_id, Some(id));
}
```

In `crates/murk-engine/src/lockstep.rs`, add an entity-disabled assertion to an existing simple step test:

```rust
assert!(result.entity_snapshot.is_none());
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p murk-core command::entity_command_tests::receipt_can_carry_spawned_entity_id -- --nocapture`

Expected: compile failure because `Receipt` has no `spawned_entity_id`.

- [ ] **Step 3: Add fields and construction defaults**

In `Receipt`, add:

```rust
/// Entity ID allocated by a spawn command.
pub spawned_entity_id: Option<EntityId>,
```

Update every `Receipt { ... }` literal in `murk-engine` and tests to include `spawned_entity_id: None`.

In `StepResult<'w>`, add:

```rust
/// Entity snapshot after this tick. `None` when entities are disabled.
pub entity_snapshot: Option<murk_entity::EntitySnapshot<'w>>,
```

In `LockstepWorld::step_sync()`, set `entity_snapshot: self.engine.entity_snapshot()`.

Add a convenience method:

```rust
pub fn entity_snapshot(&self) -> Option<murk_entity::EntitySnapshot<'_>> {
    self.engine.entity_snapshot()
}
```

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p murk-core command::entity_command_tests -- --nocapture && cargo test -p murk-engine lockstep::tests -- --nocapture`

Expected: tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/murk-core/src/command.rs crates/murk-engine/src/lockstep.rs crates/murk-engine/src/tick.rs
git commit -m "feat(engine): surface spawned entity receipts and snapshots"
```

## Task 3: TickEngine Entity Store Lifecycle

**Files:**
- Modify: `crates/murk-engine/src/tick.rs`
- Modify: `crates/murk-engine/src/realtime.rs`

- [ ] **Step 1: Write failing lifecycle tests**

Add tests in `crates/murk-engine/src/lockstep.rs`:

```rust
fn entity_manifest() -> EntityManifest {
    EntityManifest {
        property_names: vec!["alive".into(), "hp".into()],
        property_defaults: vec![1.0, 100.0],
        alive_property: PropertyIndex(0),
    }
}

fn entity_config(max_entities: u32) -> WorldConfig {
    WorldConfig::builder()
        .space(Box::new(murk_space::Line1D::new(4, murk_space::EdgeBehavior::Absorb).unwrap()))
        .fields(vec![scalar_field("energy")])
        .propagators(vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))])
        .dt(0.1)
        .max_entities(max_entities)
        .entity_manifest(entity_manifest())
        .build()
        .unwrap()
}

#[test]
fn entity_spawn_receipt_and_snapshot_are_returned() {
    let mut world = LockstepWorld::new(entity_config(4)).unwrap();
    let result = world.step_sync(vec![Command {
        payload: CommandPayload::Spawn {
            coord: vec![1].into(),
            entity_type: 7,
            property_overrides: vec![(PropertyIndex(1), 50.0)],
        },
        expires_after_tick: TickId(10),
        source_id: None,
        source_seq: None,
    }]).unwrap();

    let spawned = result.receipts[0].spawned_entity_id.unwrap();
    let entities = result.entity_snapshot.unwrap();
    assert_eq!(entities.get(spawned).unwrap().entity_type, 7);
    assert_eq!(entities.property(spawned, PropertyIndex(1)), Some(50.0));
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p murk-engine lockstep::tests::entity_spawn_receipt_and_snapshot_are_returned -- --nocapture`

Expected: receipt is rejected as `UnsupportedCommand` or compile failure before implementation.

- [ ] **Step 3: Add store fields and entity snapshot accessor**

In `TickEngine`, add:

```rust
entity_store: Option<murk_entity::EntityStore>,
entity_staging: Option<murk_entity::PropertyStaging>,
```

During `TickEngine::new`, initialize:

```rust
let entity_store = config.entity_manifest.clone().map(|manifest| {
    murk_entity::EntityStore::new(config.max_entities, manifest)
});
let entity_staging = config.entity_manifest.as_ref().map(|manifest| {
    murk_entity::PropertyStaging::new(config.max_entities, manifest.property_count() as u32)
});
```

Add:

```rust
pub fn entity_snapshot(&self) -> Option<murk_entity::EntitySnapshot<'_>> {
    self.entity_store.as_ref().map(murk_entity::EntityStore::snapshot)
}
```

Preserve `None` in `RealtimeAsyncWorld` config reconstruction by copying `max_entities` and `entity_manifest`.

- [ ] **Step 4: Run focused compile**

Run: `cargo check -p murk-engine`

Expected: compiles.

- [ ] **Step 5: Commit**

```bash
git add crates/murk-engine/src/tick.rs crates/murk-engine/src/realtime.rs
git commit -m "feat(engine): own optional entity store in TickEngine"
```

## Task 4: Entity Command Application and Rollback

**Files:**
- Modify: `crates/murk-engine/src/tick.rs`
- Test: `crates/murk-engine/src/lockstep.rs`

- [ ] **Step 1: Write failing command behavior tests**

Add tests:

```rust
fn spawn_command(coord: Vec<i32>, entity_type: u32) -> Command {
    Command {
        payload: CommandPayload::Spawn {
            coord: coord.into(),
            entity_type,
            property_overrides: Vec::new(),
        },
        expires_after_tick: TickId(10),
        source_id: None,
        source_seq: None,
    }
}

fn move_command(entity_id: EntityId, target_coord: Vec<i32>) -> Command {
    Command {
        payload: CommandPayload::Move {
            entity_id,
            target_coord: target_coord.into(),
        },
        expires_after_tick: TickId(10),
        source_id: None,
        source_seq: None,
    }
}

#[test]
fn move_and_despawn_validate_entity_generation() {
    let mut world = LockstepWorld::new(entity_config(4)).unwrap();
    let spawn = world.step_sync(vec![spawn_command(vec![0], 1)]).unwrap();
    let id = spawn.receipts[0].spawned_entity_id.unwrap();

    let moved = world.step_sync(vec![move_command(id, vec![2])]).unwrap();
    assert!(moved.receipts[0].accepted);
    assert_eq!(moved.entity_snapshot.unwrap().get(id).unwrap().coord.as_slice(), &[2]);

    let stale = EntityId::new(id.slot(), id.generation() + 1);
    let rejected = world.step_sync(vec![move_command(stale, vec![3])]).unwrap();
    assert_eq!(rejected.receipts[0].reason_code, Some(IngressError::UnknownEntity));
}

#[test]
fn propagator_failure_rolls_back_entity_commands() {
    let mut world = LockstepWorld::new(entity_config_with_failing_propagator(4)).unwrap();
    let err = match world.step_sync(vec![spawn_command(vec![0], 1)]) {
        Ok(_) => panic!("expected propagator failure"),
        Err(err) => err,
    };
    assert_eq!(err.receipts[0].reason_code, Some(IngressError::TickRollback));
    assert_eq!(world.snapshot().generation().0, 0);
    assert!(world.entity_snapshot().unwrap().iter_all().next().is_none());
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test -p murk-engine lockstep::tests::move_and_despawn_validate_entity_generation lockstep::tests::propagator_failure_rolls_back_entity_commands -- --nocapture`

Expected: entity commands still rejected or rollback does not restore entity state.

- [ ] **Step 3: Implement command application**

Before applying entity commands, capture:

```rust
let entity_rollback = self.entity_store.as_ref().map(murk_entity::EntityStore::snapshot_for_rollback);
```

Handle entity payloads:

```rust
CommandPayload::Spawn { coord, entity_type, property_overrides } => {
    match self.entity_store.as_mut() {
        Some(store) => match store.spawn(coord.clone(), *entity_type, property_overrides) {
            Ok(id) => receipt.spawned_entity_id = Some(id),
            Err(err) => { receipt.accepted = false; receipt.reason_code = Some(err); }
        },
        None => { receipt.accepted = false; receipt.reason_code = Some(IngressError::UnsupportedCommand); }
    }
}
CommandPayload::Move { entity_id, target_coord } => {
    let applied = self.space.canonical_rank(target_coord).is_some();
    let result = if applied {
        self.entity_store
            .as_mut()
            .ok_or(IngressError::UnsupportedCommand)
            .and_then(|store| store.move_entity(*entity_id, target_coord.clone()))
    } else {
        Err(IngressError::NotApplied)
    };
    if let Err(err) = result {
        receipt.accepted = false;
        receipt.reason_code = Some(err);
    }
}
CommandPayload::Despawn { entity_id } => {
    let result = self
        .entity_store
        .as_mut()
        .ok_or(IngressError::UnsupportedCommand)
        .and_then(|store| store.despawn(*entity_id));
    if let Err(err) = result {
        receipt.accepted = false;
        receipt.reason_code = Some(err);
    }
}
```

On rollback, restore `entity_rollback` before returning `TickError`, and call `entity_staging.reset()`.

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p murk-engine lockstep::tests::entity_ -- --nocapture`

Expected: entity command tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/murk-engine/src/tick.rs crates/murk-engine/src/lockstep.rs
git commit -m "feat(engine): apply entity commands with rollback"
```

## Task 5: StepContext Entity Access

**Files:**
- Modify: `crates/murk-propagator/Cargo.toml`
- Modify: `crates/murk-propagator/src/context.rs`
- Modify: `crates/murk-propagator/src/propagator.rs`
- Modify: `crates/murk-engine/src/tick.rs`

- [ ] **Step 1: Write failing StepContext tests**

Add to `crates/murk-propagator/src/context.rs`:

```rust
fn context_with_entities<'a>(
    snapshot: murk_entity::EntitySnapshot<'a>,
    staging: &'a mut murk_entity::PropertyStaging,
) -> StepContext<'a> {
    let reader = Box::leak(Box::new(murk_test_utils::MockFieldReader::new()));
    let writer = Box::leak(Box::new(murk_test_utils::MockFieldWriter::new()));
    let scratch = Box::leak(Box::new(ScratchRegion::new(0)));
    let space = Box::leak(Box::new(murk_space::Line1D::new(1, murk_space::EdgeBehavior::Absorb).unwrap()));
    StepContext::new(
        reader,
        reader,
        writer,
        scratch,
        space,
        TickId(1),
        0.1,
        Some(snapshot),
        Some(staging),
    )
}

#[test]
fn context_exposes_entities_and_entity_writes() {
    let manifest = murk_core::EntityManifest {
        property_names: vec!["alive".into(), "hp".into()],
        property_defaults: vec![1.0, 100.0],
        alive_property: murk_core::PropertyIndex(0),
    };
    let mut store = murk_entity::EntityStore::new(4, manifest);
    let id = store.spawn(vec![0].into(), 0, &[]).unwrap();
    let snapshot = store.snapshot();
    let mut staging = murk_entity::PropertyStaging::new(4, 2);
    let mut ctx = context_with_entities(snapshot, &mut staging);

    assert_eq!(ctx.entities().unwrap().property(id, PropertyIndex(1)), Some(100.0));
    assert!(ctx.entity_writes().unwrap().set(id, PropertyIndex(1), 25.0));
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cargo test -p murk-propagator context::tests::context_exposes_entities_and_entity_writes -- --nocapture`

Expected: compile failure because entity accessors are missing.

- [ ] **Step 3: Add optional entity fields and accessors**

Extend `StepContext::new` with `entity_snapshot` and `entity_staging` options, then add:

```rust
pub fn entities(&self) -> Option<murk_entity::EntitySnapshot<'_>> { self.entity_snapshot }
pub fn entities_previous(&self) -> Option<murk_entity::EntitySnapshot<'_>> { self.entity_snapshot }
pub fn entities_overlaid(&self) -> Option<murk_entity::EntityOverlayReader<'_, '_>> {
    Some(murk_entity::EntityOverlayReader::new(self.entity_snapshot?, self.entity_staging.as_deref()?))
}
pub fn entity_writes(&mut self) -> Option<&mut murk_entity::PropertyStaging> {
    self.entity_staging.as_deref_mut()
}
```

Add default methods to `Propagator`:

```rust
fn reads_entities(&self) -> &[murk_core::PropertyIndex] { &[] }
fn reads_entities_previous(&self) -> &[murk_core::PropertyIndex] { &[] }
fn writes_entities(&self) -> &[murk_core::PropertyIndex] { &[] }
```

- [ ] **Step 4: Wire TickEngine context construction**

Pass `self.entity_store.as_ref().map(EntityStore::snapshot)` and `self.entity_staging.as_mut()` to `StepContext::new`. After all propagators pass, commit staging with `store.apply_staged_properties(staging)` and reset staging.

- [ ] **Step 5: Run focused tests**

Run: `cargo test -p murk-propagator context::tests -- --nocapture && cargo test -p murk-engine lockstep::tests::entity_ -- --nocapture`

Expected: tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/murk-propagator crates/murk-engine/src/tick.rs Cargo.lock
git commit -m "feat(propagator): expose entity access through StepContext"
```

## Task 6: Verification Gate

**Files:**
- Modify only files required by failures found in this task.

- [ ] **Step 1: Run full verification**

Run:

```bash
cargo fmt --all
cargo check --workspace
cargo test --workspace --all-targets
cargo clippy --workspace -- -D warnings
cargo test --doc -p murk-core -p murk-entity -p murk-engine -p murk-propagator
wardline scan . --lang rust --fail-on ERROR
```

Expected: all commands exit 0. Wardline may still warn that no `@trusted` functions are declared; that warning is not an ERROR gate failure.

- [ ] **Step 2: Fix any verification failures**

For compile or test failures, patch the failing module directly and rerun the exact failed command before rerunning the full gate. For Wardline ERROR findings, fix the boundary function named by Wardline and rerun `wardline scan . --lang rust --fail-on ERROR`.

- [ ] **Step 3: Commit verification fixes**

```bash
git add -u
git commit -m "chore(engine): pass entity integration verification"
```

## Self-Review Notes

- Spec coverage: covers M2 lockstep entity config, spawn/move/despawn, rollback, receipt output, StepResult entity snapshot, StepContext entity access, property staging commit, and realtime async deferral.
- Deferred by design: `murk-slot`, FFI/Python entity API, and replay compatibility beyond M1 command shape remain later milestones.
- Type consistency: this plan uses M1 names now present in the branch: `EntityStore`, `EntityStoreSnapshot`, `EntitySnapshot`, `PropertyStaging`, `EntityOverlayReader`, `EntityId`, `PropertyIndex`, and `EntityManifest`.
