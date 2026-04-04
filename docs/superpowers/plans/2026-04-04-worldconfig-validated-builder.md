# WorldConfig Validated Builder Pattern

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace direct `WorldConfig` struct construction with a validated builder that prevents invalid configs from existing.

**Architecture:** Add `WorldConfigBuilder` with fluent setter methods and a `build()` that runs all validation. Make `WorldConfig` fields `pub(crate)` so only the builder (or crate-internal code) can construct instances. External crates must use the builder. No public getters needed yet — no external crate reads WorldConfig fields after construction.

**Tech Stack:** Pure Rust, no new dependencies. TDD against existing `ConfigError` variants plus two new ones (`MissingSpace`, `MissingDt`).

**Filigree:** murk-439ccbb000

---

## Semver Note

Murk is pre-1.0 with no published crate and no external consumers outside this workspace. The `pub` → `pub(crate)` visibility change on `WorldConfig` fields is a breaking API change, but all consumers are within the workspace and will be migrated atomically in this branch. No version bump is required. The PR description should note this is a breaking change for the record.

---

## Review Findings Incorporated

This plan was reviewed by 4 specialist agents (architecture, reality, quality, systems). The following findings have been incorporated:

**Blocking fixes applied:**
- B1 (semver): Added semver note above
- B2 (broken workspace): Restructured so Tasks 2-5 are WIP (no per-task commits for broken intermediate states) — the visibility change and ALL migrations land before the next `cargo test --workspace`
- B3 (invalid dt test): Added `builder_invalid_dt_zero_rejected` test to Task 1

**Warnings addressed:**
- W1-W2 (construction counts): Corrected counts in File Map — actual struct literal counts, not grep noise from return-type declarations
- W3 (FFI dt default): Added comment in Task 5 at the FFI bridge site
- W4 (realtime.rs escape hatch): Expanded bypass comment to name TickEngine::new() re-validation
- W5 (double validation): Added comment to TickEngine::new() in Task 7
- W6 (BackoffConfig inconsistency): Task 7 creates a follow-up filigree issue
- W7 (tick_rate_hz triple validation): Noted as out-of-scope but documented
- W8 (struct-update syntax): Two realtime.rs tests using `..test_config()` explicitly migrated in Task 3
- W9 (Display tests): Added Display round-trip tests for MissingSpace/MissingDt in Task 1
- W10 (error variant asymmetry): Added doc comment on `build()` explaining the design choice
- W11 (reference_profile location): Resolved — it's in `murk-bench/src/lib.rs:27`, concrete migration in Task 5
- W12-W14 (missing comments): All three comments incorporated into respective tasks

---

## File Map

Construction counts are **actual struct literal counts** (not grep hits that include return-type declarations).

| File | Action | Struct Literals |
|------|--------|-----------------|
| `crates/murk-engine/src/config.rs` | Modify: add builder, change visibility | 1 (test helper) |
| `crates/murk-engine/src/lib.rs` | Modify: re-export `WorldConfigBuilder` | 0 |
| `crates/murk-engine/src/tick.rs` | Migrate test constructions | 10 |
| `crates/murk-engine/src/lockstep.rs` | Migrate test constructions | 5 |
| `crates/murk-engine/src/batched.rs` | Migrate test constructions | 10 |
| `crates/murk-engine/src/realtime.rs` | Migrate tests + keep production reconstruction | 4 (3 test + 1 production kept as-is) |
| `crates/murk-engine/src/ring.rs` | Migrate test construction | 1 |
| `crates/murk-engine/src/tick_thread.rs` | Migrate test construction | 1 |
| `crates/murk-engine/tests/arena_fragmentation.rs` | Migrate test construction | 1 |
| `crates/murk-engine/tests/nan_detection.rs` | Migrate test construction | 1 |
| `crates/murk-engine/tests/stress_death_spiral.rs` | Migrate test construction | 1 |
| `crates/murk-engine/tests/stress_rejection_oscillation.rs` | Remove field mutations (construction is in murk-bench) | 0 (uses `reference_profile()`) |
| `crates/murk-engine/examples/quickstart.rs` | Migrate construction | 1 |
| `crates/murk-engine/examples/replay.rs` | Migrate construction | 1 |
| `crates/murk-engine/examples/realtime_async.rs` | Migrate construction | 1 |
| `crates/murk-ffi/src/world.rs` | Migrate FFI→WorldConfig bridge | 1 |
| `crates/murk-ffi/src/batched.rs` | Check for construction, migrate if present | 1 |
| `crates/murk-propagators/tests/integration.rs` | Migrate test constructions | 2 |
| `crates/murk-propagators/tests/p4_integration.rs` | Migrate test constructions | 5 |
| `crates/murk-propagators/tests/library_integration.rs` | Migrate test constructions | 9 |
| `crates/murk-replay/tests/determinism.rs` | Migrate test constructions | 6 |
| `crates/murk-bench/src/lib.rs` | Migrate `reference_profile()` and `stress_profile()` | 2 |
| `crates/murk/src/lib.rs` | Update doc example, re-export in prelude | 1 |
| `crates/murk/README.md` | Update code example | 0 (non-compiled) |
| `crates/murk-engine/README.md` | Update code example | 0 (non-compiled) |
| `book/src/getting-started.md` | Update code example | 0 (non-compiled) |

**Total struct literals to migrate:** ~55 (plus ~5 non-compiled doc examples)

---

## API Design

### Builder API

```rust
// Required: space, fields (non-empty), propagators (non-empty), dt
// Defaulted: seed=0, ring_buffer_size=8, max_ingress_queue=1024,
//            tick_rate_hz=None, backoff=BackoffConfig::default()

let config = WorldConfig::builder()
    .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
    .fields(vec![scalar_field("energy")])
    .propagators(vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))])
    .dt(0.1)
    .seed(42)
    .build()?;
```

### WorldConfig field visibility

```rust
pub struct WorldConfig {
    pub(crate) space: Box<dyn Space>,
    pub(crate) fields: Vec<FieldDef>,
    pub(crate) propagators: Vec<Box<dyn Propagator>>,
    pub(crate) dt: f64,
    pub(crate) seed: u64,
    pub(crate) ring_buffer_size: usize,
    pub(crate) max_ingress_queue: usize,
    pub(crate) tick_rate_hz: Option<f64>,
    pub(crate) backoff: BackoffConfig,
}
```

### New ConfigError variants

```rust
/// Builder: `space` was not set.
MissingSpace,
/// Builder: `dt` was not set.
MissingDt,
```

### Design note: required field asymmetry

`space` and `dt` get dedicated `MissingSpace`/`MissingDt` error variants because they use `Option<T>` in the builder (no sensible default). `fields` and `propagators` default to empty `Vec`s — emptiness is caught by existing validation errors (`NoFields`, `Pipeline(EmptyPipeline)`) which have adequate error messages. Adding `MissingFields`/`MissingPropagators` variants would duplicate those checks without improving diagnostics.

### Migration pattern

Every `WorldConfig { field: value, ... }` becomes:

```rust
WorldConfig::builder()
    .space(space_value)
    .fields(fields_value)
    .propagators(propagators_value)
    .dt(dt_value)
    .seed(seed_value)              // omit if 0 is acceptable
    .ring_buffer_size(size_value)  // omit if 8 is acceptable
    .max_ingress_queue(queue_value) // omit if 1024 is acceptable
    .tick_rate_hz(hz_value)        // omit if None is acceptable
    .backoff(backoff_value)        // omit if default is acceptable
    .build()
    .unwrap()                      // in tests; use ? in production
```

---

## Task 1: Add WorldConfigBuilder and new ConfigError variants

**Files:**
- Modify: `crates/murk-engine/src/config.rs:98-162` (ConfigError enum)
- Modify: `crates/murk-engine/src/config.rs:164-209` (Display impl)
- Modify: `crates/murk-engine/src/config.rs:240-259` (WorldConfig struct — after builder exists)
- Create (in same file): `WorldConfigBuilder` struct + impl block

### Steps

- [ ] **Step 1: Write failing test for builder missing space**

Add at the bottom of `config.rs` `mod tests`:

```rust
#[test]
fn builder_missing_space_fails() {
    let result = WorldConfig::builder()
        .fields(vec![scalar_field("energy")])
        .propagators(vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))])
        .dt(0.1)
        .build();
    match result {
        Err(ConfigError::MissingSpace) => {}
        other => panic!("expected MissingSpace, got {other:?}"),
    }
}
```

- [ ] **Step 2: Write failing test for builder missing dt**

```rust
#[test]
fn builder_missing_dt_fails() {
    let result = WorldConfig::builder()
        .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
        .fields(vec![scalar_field("energy")])
        .propagators(vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))])
        .build();
    match result {
        Err(ConfigError::MissingDt) => {}
        other => panic!("expected MissingDt, got {other:?}"),
    }
}
```

- [ ] **Step 3: Write failing test for builder with defaults**

```rust
#[test]
fn builder_with_defaults_succeeds() {
    let config = WorldConfig::builder()
        .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
        .fields(vec![scalar_field("energy")])
        .propagators(vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))])
        .dt(0.1)
        .build()
        .unwrap();
    // These assertions access pub(crate) fields — valid because this test
    // is in the same crate's #[cfg(test)] module. If fields become fully
    // private in the future, these would need getter methods.
    assert_eq!(config.seed, 0);
    assert_eq!(config.ring_buffer_size, 8);
    assert_eq!(config.max_ingress_queue, 1024);
    assert!(config.tick_rate_hz.is_none());
}
```

- [ ] **Step 4: Write failing test for builder with all options**

```rust
#[test]
fn builder_with_all_options_succeeds() {
    let config = WorldConfig::builder()
        .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
        .fields(vec![scalar_field("energy")])
        .propagators(vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))])
        .dt(0.1)
        .seed(99)
        .ring_buffer_size(16)
        .max_ingress_queue(512)
        .tick_rate_hz(60.0)
        .backoff(BackoffConfig {
            initial_max_skew: 3,
            backoff_factor: 2.0,
            max_skew_cap: 20,
            decay_rate: 30,
            rejection_rate_threshold: 0.10,
        })
        .build()
        .unwrap();
    assert_eq!(config.seed, 99);
    assert_eq!(config.ring_buffer_size, 16);
    assert_eq!(config.max_ingress_queue, 512);
    assert_eq!(config.tick_rate_hz, Some(60.0));
    assert_eq!(config.backoff.initial_max_skew, 3);
}
```

- [ ] **Step 5: Write failing test for builder validation passthrough (ring buffer)**

```rust
#[test]
fn builder_validates_ring_buffer_too_small() {
    let result = WorldConfig::builder()
        .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
        .fields(vec![scalar_field("energy")])
        .propagators(vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))])
        .dt(0.1)
        .ring_buffer_size(1)
        .build();
    match result {
        Err(ConfigError::RingBufferTooSmall { configured: 1 }) => {}
        other => panic!("expected RingBufferTooSmall, got {other:?}"),
    }
}
```

- [ ] **Step 6: Write failing test for builder validation passthrough (invalid dt)**

This confirms that `build()` catches invalid-but-non-None dt values through `validate()`.

```rust
#[test]
fn builder_invalid_dt_zero_rejected() {
    let result = WorldConfig::builder()
        .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
        .fields(vec![scalar_field("energy")])
        .propagators(vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))])
        .dt(0.0)
        .build();
    match result {
        Err(ConfigError::Pipeline(_)) => {}
        other => panic!("expected Pipeline error for dt=0.0, got {other:?}"),
    }
}
```

- [ ] **Step 7: Write failing tests for Display of new error variants**

Follow the existing pattern established by `cell_count_overflow_display_says_cell_count` and friends:

```rust
#[test]
fn missing_space_display() {
    let err = ConfigError::MissingSpace;
    let msg = format!("{err}");
    assert!(
        msg.contains("space"),
        "MissingSpace Display should mention 'space', got: {msg}"
    );
}

#[test]
fn missing_dt_display() {
    let err = ConfigError::MissingDt;
    let msg = format!("{err}");
    assert!(
        msg.contains("dt"),
        "MissingDt Display should mention 'dt', got: {msg}"
    );
}
```

- [ ] **Step 8: Run tests to confirm they fail**

Run: `cargo test -p murk-engine --lib config::tests -- builder missing_space_display missing_dt_display`
Expected: compilation errors (WorldConfig::builder doesn't exist yet)

- [ ] **Step 9: Add MissingSpace and MissingDt to ConfigError**

In `config.rs`, add to the `ConfigError` enum (after `IngressQueueZero`):

```rust
/// Builder: `space` was not set.
MissingSpace,
/// Builder: `dt` was not set.
MissingDt,
```

Add Display arms in the `fmt::Display` impl:

```rust
Self::MissingSpace => write!(f, "builder: space not set — call .space() before .build()"),
Self::MissingDt => write!(f, "builder: dt not set — call .dt() before .build()"),
```

- [ ] **Step 10: Add WorldConfigBuilder struct**

Add after the `WorldConfig` impl block (before `impl fmt::Debug for WorldConfig`):

```rust
// ── WorldConfigBuilder ────────────────────────────────────────

/// Fluent builder for [`WorldConfig`].
///
/// Required: [`space`](Self::space), [`fields`](Self::fields) (non-empty),
/// [`propagators`](Self::propagators) (non-empty), [`dt`](Self::dt).
///
/// Optional (with defaults): [`seed`](Self::seed) (0),
/// [`ring_buffer_size`](Self::ring_buffer_size) (8),
/// [`max_ingress_queue`](Self::max_ingress_queue) (1024),
/// [`tick_rate_hz`](Self::tick_rate_hz) (None),
/// [`backoff`](Self::backoff) ([`BackoffConfig::default()`]).
///
/// # Required field enforcement
///
/// `space` and `dt` are enforced via dedicated error variants
/// ([`ConfigError::MissingSpace`], [`ConfigError::MissingDt`]) because
/// they have no sensible defaults. `fields` and `propagators` default
/// to empty `Vec`s — emptiness is caught by existing validation
/// ([`ConfigError::NoFields`], [`ConfigError::Pipeline`]) which
/// provides adequate diagnostics.
///
/// # Move semantics
///
/// The builder is consumed by [`build()`](Self::build). Calling
/// `.build()` twice is a compile error (Rust move semantics). Each
/// setter method takes `self` by value and returns `Self`, enabling
/// the fluent chaining pattern.
pub struct WorldConfigBuilder {
    space: Option<Box<dyn Space>>,
    fields: Vec<FieldDef>,
    propagators: Vec<Box<dyn Propagator>>,
    dt: Option<f64>,
    seed: u64,
    ring_buffer_size: usize,
    max_ingress_queue: usize,
    tick_rate_hz: Option<f64>,
    backoff: BackoffConfig,
}

impl WorldConfigBuilder {
    /// Set the spatial topology. **Required.**
    ///
    /// If called multiple times, the last value wins.
    pub fn space(mut self, space: Box<dyn Space>) -> Self {
        self.space = Some(space);
        self
    }

    /// Set all field definitions at once. **Required (non-empty).**
    ///
    /// If called multiple times, the last value wins.
    pub fn fields(mut self, fields: Vec<FieldDef>) -> Self {
        self.fields = fields;
        self
    }

    /// Set all propagators at once. **Required (non-empty).**
    ///
    /// If called multiple times, the last value wins.
    pub fn propagators(mut self, propagators: Vec<Box<dyn Propagator>>) -> Self {
        self.propagators = propagators;
        self
    }

    /// Set the simulation timestep in seconds. **Required.**
    ///
    /// If called multiple times, the last value wins.
    pub fn dt(mut self, dt: f64) -> Self {
        self.dt = Some(dt);
        self
    }

    /// Set the RNG seed for deterministic simulation. Default: 0.
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Set the snapshot ring buffer size. Default: 8. Minimum: 2.
    pub fn ring_buffer_size(mut self, size: usize) -> Self {
        self.ring_buffer_size = size;
        self
    }

    /// Set the maximum ingress queue depth. Default: 1024. Minimum: 1.
    pub fn max_ingress_queue(mut self, size: usize) -> Self {
        self.max_ingress_queue = size;
        self
    }

    /// Set the target tick rate for realtime-async mode.
    /// Default: None (lockstep mode, no autonomous ticking).
    pub fn tick_rate_hz(mut self, hz: f64) -> Self {
        self.tick_rate_hz = Some(hz);
        self
    }

    /// Set the adaptive backoff configuration. Default: [`BackoffConfig::default()`].
    pub fn backoff(mut self, backoff: BackoffConfig) -> Self {
        self.backoff = backoff;
        self
    }

    /// Consume the builder and produce a validated [`WorldConfig`].
    ///
    /// Checks required fields (`space`, `dt`), then delegates to
    /// [`WorldConfig::validate()`] for all structural invariants
    /// (field validity, ring buffer size, backoff config, pipeline
    /// validation including CFL checks).
    ///
    /// Returns [`ConfigError`] if required fields are missing or any
    /// validation constraint fails.
    pub fn build(self) -> Result<WorldConfig, ConfigError> {
        let space = self.space.ok_or(ConfigError::MissingSpace)?;
        let dt = self.dt.ok_or(ConfigError::MissingDt)?;

        let config = WorldConfig {
            space,
            fields: self.fields,
            propagators: self.propagators,
            dt,
            seed: self.seed,
            ring_buffer_size: self.ring_buffer_size,
            max_ingress_queue: self.max_ingress_queue,
            tick_rate_hz: self.tick_rate_hz,
            backoff: self.backoff,
        };
        config.validate()?;
        Ok(config)
    }
}
```

- [ ] **Step 11: Add `builder()` associated function to WorldConfig**

In the `impl WorldConfig` block, add:

```rust
/// Create a new [`WorldConfigBuilder`] with default optional values.
pub fn builder() -> WorldConfigBuilder {
    WorldConfigBuilder {
        space: None,
        fields: Vec::new(),
        propagators: Vec::new(),
        dt: None,
        seed: 0,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    }
}
```

- [ ] **Step 12: Run tests to confirm they pass**

Run: `cargo test -p murk-engine --lib config::tests -- builder missing_space_display missing_dt_display`
Expected: all 9 new tests pass

- [ ] **Step 13: Run full murk-engine test suite**

Run: `cargo test -p murk-engine --lib`
Expected: all existing tests still pass (fields still pub, no breakage)

- [ ] **Step 14: Commit**

```
feat(config): add WorldConfigBuilder with validated build()

Adds a fluent builder for WorldConfig. build() runs all existing
validation, preventing invalid configs from being constructed.
New ConfigError variants: MissingSpace, MissingDt.
```

---

## Task 2: Make WorldConfig fields pub(crate), re-export builder, migrate config.rs tests

**Files:**
- Modify: `crates/murk-engine/src/config.rs:240-259` (field visibility)
- Modify: `crates/murk-engine/src/config.rs:380-714` (test functions)
- Modify: `crates/murk-engine/src/lib.rs:29` (re-export WorldConfigBuilder)

**Note:** After this task, `cargo test --workspace` will NOT pass. External crates that use `WorldConfig {}` struct literals will fail to compile. This is intentional — Tasks 3-5 fix those crates. Do NOT commit this task independently; it will be squashed with Tasks 3-5 into a single commit.

### Steps

- [ ] **Step 1: Re-export WorldConfigBuilder from murk-engine**

In `crates/murk-engine/src/lib.rs:29`, change:

```rust
pub use config::{AsyncConfig, BackoffConfig, ConfigError, WorldConfig};
```

to:

```rust
pub use config::{AsyncConfig, BackoffConfig, ConfigError, WorldConfig, WorldConfigBuilder};
```

- [ ] **Step 2: Change WorldConfig fields from `pub` to `pub(crate)`**

In `crates/murk-engine/src/config.rs`, change the WorldConfig struct (lines 240-259):

```rust
pub struct WorldConfig {
    pub(crate) space: Box<dyn Space>,
    pub(crate) fields: Vec<FieldDef>,
    pub(crate) propagators: Vec<Box<dyn Propagator>>,
    pub(crate) dt: f64,
    pub(crate) seed: u64,
    pub(crate) ring_buffer_size: usize,
    pub(crate) max_ingress_queue: usize,
    pub(crate) tick_rate_hz: Option<f64>,
    pub(crate) backoff: BackoffConfig,
}
```

- [ ] **Step 3: Migrate `valid_config()` test helper to builder**

In `config.rs` tests, replace `valid_config()` (lines ~397-409):

```rust
fn valid_config() -> WorldConfig {
    WorldConfig::builder()
        .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
        .fields(vec![scalar_field("energy")])
        .propagators(vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))])
        .dt(0.1)
        .seed(42)
        .build()
        .unwrap()
}
```

- [ ] **Step 4: Verify existing validation tests still compile**

These tests create a valid config then mutate a field before validating. Since fields are `pub(crate)`, this still works within the crate's own `#[cfg(test)]` module. **No changes needed** for:
- `validate_empty_propagators_fails` (mutates `cfg.propagators`)
- `validate_invalid_dt_fails` (mutates `cfg.dt`)
- `validate_write_conflict_fails` (mutates `cfg.propagators`)
- `validate_dt_exceeds_max_dt_fails` (mutates `cfg.propagators`, `cfg.dt`)
- `validate_empty_space_fails` (mutates `cfg.space`)
- `validate_no_fields_fails` (mutates `cfg.fields`)
- `validate_backoff_*` tests (mutate `cfg.backoff.*`)
- `validate_subnormal_tick_rate_hz_rejected` (mutates `cfg.tick_rate_hz`)
- `validate_valid_backoff_succeeds` (mutates `cfg.backoff`)

These tests compile because `pub(crate)` is accessible from `#[cfg(test)]` within the same crate.

- [ ] **Step 5: Run config tests**

Run: `cargo test -p murk-engine --lib config::tests`
Expected: all tests pass

- [ ] **Step 6: Check that murk-engine internal code still compiles**

Run: `cargo check -p murk-engine --lib`
Expected: passes (tick.rs, lockstep.rs, realtime.rs, batched.rs are all in-crate)

- [ ] **Step 7: Identify external compilation failures (informational)**

Run: `cargo check --workspace 2>&1 | head -60`
Expected: compilation errors in external crates. This confirms what Tasks 3-5 must fix. **Do NOT commit yet** — workspace is intentionally broken until Task 5.

---

## Task 3: Migrate murk-engine internal test helpers and tests

**Files:**
- Modify: `crates/murk-engine/src/tick.rs` (10 constructions)
- Modify: `crates/murk-engine/src/lockstep.rs` (5 constructions)
- Modify: `crates/murk-engine/src/batched.rs` (10 constructions)
- Modify: `crates/murk-engine/src/realtime.rs` (3 test constructions + 2 struct-update tests + 1 production site kept)
- Modify: `crates/murk-engine/src/ring.rs` (1 construction)
- Modify: `crates/murk-engine/src/tick_thread.rs` (1 construction)

**Note:** These are all within `murk-engine` so `pub(crate)` fields ARE accessible. Migrating for consistency — all construction should go through the builder.

### Steps

- [ ] **Step 1: Migrate lockstep.rs test helpers**

Find `simple_config()`, `two_field_config()`, and `square4_config()` helpers in `crates/murk-engine/src/lockstep.rs` and convert each from `WorldConfig { ... }` to `WorldConfig::builder()...build().unwrap()`.

Example for `simple_config()`:

```rust
fn simple_config() -> WorldConfig {
    WorldConfig::builder()
        .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
        .fields(vec![scalar_field("energy")])
        .propagators(vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))])
        .dt(0.1)
        .seed(42)
        .build()
        .unwrap()
}
```

Apply the same pattern to `two_field_config()` and `square4_config()`. Preserve the exact same field values — only change the construction syntax. Also convert any remaining `WorldConfig {` literals in lockstep.rs test functions.

- [ ] **Step 2: Migrate batched.rs test helpers**

Convert `make_config()` and `make_grid_config()` in `crates/murk-engine/src/batched.rs`:

```rust
fn make_config(seed: u64, value: f32) -> WorldConfig {
    WorldConfig::builder()
        .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
        .fields(vec![scalar_field("energy")])
        .propagators(vec![Box::new(ConstPropagator::new("const", FieldId(0), value))])
        .dt(0.1)
        .seed(seed)
        .build()
        .unwrap()
}
```

Same pattern for `make_grid_config()`. Also convert all remaining `WorldConfig {` literals in batched.rs tests.

- [ ] **Step 3: Migrate realtime.rs test helper and struct-update tests**

Convert `test_config()` in `crates/murk-engine/src/realtime.rs`:

```rust
fn test_config() -> WorldConfig {
    WorldConfig::builder()
        .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
        .fields(vec![scalar_field("energy")])
        .propagators(vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))])
        .dt(0.1)
        .seed(42)
        .tick_rate_hz(60.0)
        .build()
        .unwrap()
}
```

**Important — struct-update syntax tests:** Two tests at lines ~1036 and ~1116 use `WorldConfig { tick_rate_hz: Some(0.5), ..test_config() }`. These must be converted to use the builder directly (they are in-crate so they could still use struct syntax, but converting them ensures consistency):

```rust
// In shutdown_fast_with_slow_tick_rate (line ~1036):
let config = WorldConfig::builder()
    .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
    .fields(vec![scalar_field("energy")])
    .propagators(vec![Box::new(ConstPropagator::new("const", FieldId(0), 1.0))])
    .dt(0.1)
    .seed(42)
    .tick_rate_hz(0.5)
    .build()
    .unwrap();

// In preflight_observes_ingress_backlog (line ~1116):
// Same pattern — identical to above.
```

- [ ] **Step 4: Handle realtime.rs production reconstruction (line ~190)**

The `RealtimeAsyncWorld::new()` method destructures config fields and reconstructs a `WorldConfig` with an `ArcSpaceWrapper`. Since this is in-crate code with `pub(crate)` access, it stays as direct struct construction. **Replace or update the existing comment** around line 190:

```rust
// Direct struct construction is intentional here: we replace `space`
// with an ArcSpaceWrapper so TickEngine and this world share the same
// spatial topology via Arc. The original config was validated by the
// caller (either via WorldConfigBuilder::build() or direct validate()).
// TickEngine::new() calls validate_pipeline() internally, so the
// reconstructed config is re-validated before use.
let engine_config = WorldConfig {
    space: engine_space,
    // ... rest unchanged
};
```

- [ ] **Step 5: Migrate remaining WorldConfig {} literals in tick.rs, ring.rs, tick_thread.rs**

Search each file for `WorldConfig {` and convert every occurrence to builder syntax. Preserve exact field values.

- [ ] **Step 6: Run murk-engine unit tests**

Run: `cargo test -p murk-engine --lib`
Expected: all tests pass

---

## Task 4: Migrate integration tests

**Files:**
- Modify: `crates/murk-engine/tests/arena_fragmentation.rs`
- Modify: `crates/murk-engine/tests/nan_detection.rs`
- Modify: `crates/murk-engine/tests/stress_death_spiral.rs`
- Modify: `crates/murk-engine/tests/stress_rejection_oscillation.rs`

**Note:** Integration tests in `tests/` are external to the crate — `pub(crate)` fields are **not** accessible. These MUST be migrated for the code to compile.

### Steps

- [ ] **Step 1: Migrate arena_fragmentation.rs**

Convert `sparse_churn_config()`. There is 1 struct literal construction in this file (the function return-type declaration `fn sparse_churn_config() -> WorldConfig {` is NOT a construction):

```rust
fn sparse_churn_config() -> WorldConfig {
    let fields = vec![
        FieldDef { name: "energy".to_string(), /* ... keep same values ... */ },
        FieldDef { name: "sparse_marker".to_string(), /* ... keep same values ... */ },
    ];
    WorldConfig::builder()
        .space(Box::new(murk_space::Line1D::new(100, murk_space::EdgeBehavior::Absorb).unwrap()))
        .fields(fields)
        .propagators(vec![Box::new(FillPropagator::new("fill_energy", FieldId(0), 1.0))])
        .dt(0.1)
        .seed(42)
        .build()
        .unwrap()
}
```

Check for any other `WorldConfig {` literals in the file (including the static-field variant if present) and convert those too.

- [ ] **Step 2: Migrate nan_detection.rs**

Convert `nan_config()` (1 struct literal):

```rust
fn nan_config(succeed_count: usize) -> WorldConfig {
    WorldConfig::builder()
        .space(Box::new(Line1D::new(10, EdgeBehavior::Absorb).unwrap()))
        .fields(vec![FieldDef {
            name: "value".to_string(),
            field_type: FieldType::Scalar,
            mutability: FieldMutability::PerTick,
            units: None,
            bounds: None,
            boundary_behavior: BoundaryBehavior::Clamp,
        }])
        .propagators(vec![Box::new(NanOnTickPropagator::new("nan_prop", FieldId(0), succeed_count))])
        .dt(0.1)
        .seed(42)
        .build()
        .unwrap()
}
```

- [ ] **Step 3: Migrate stress_death_spiral.rs**

Convert the config construction (1 struct literal, around line 95):

```rust
let config = WorldConfig::builder()
    .space(Box::new(Square4::new(100, 100, EdgeBehavior::Absorb).unwrap()))
    .fields(murk_propagators::reference_fields())
    .propagators(vec![
        Box::new(DiffusionPropagator::new(0.1)),
        Box::new(AgentMovementPropagator::new(action_buffer.clone(), initial_positions)),
        Box::new(RewardPropagator::new(1.0, -0.01)),
    ])
    .dt(0.1)
    .seed(seed)
    .build()
    .unwrap();
```

- [ ] **Step 4: Migrate stress_rejection_oscillation.rs**

This file has **zero** `WorldConfig {}` struct literals. It calls `murk_bench::reference_profile(42, action_buffer)` which returns a `WorldConfig`, then mutates `config.max_ingress_queue` and `config.backoff` at lines 67 and 70. After `pub(crate)`, those mutations will fail to compile.

The fix depends on Task 5 migrating `reference_profile()` in `murk-bench` to return a builder-constructed config. But since `reference_profile()` returns an owned `WorldConfig` (not a builder), the mutations can't be done on the returned value.

**Solution:** Inline the construction. Replace lines 62-76 with a builder call that includes the custom `max_ingress_queue` and `backoff`:

```rust
let action_buffer = new_action_buffer();
let cell_count = 100 * 100;
let initial_positions = murk_bench::init_agent_positions(cell_count, 4, 42);

let config = WorldConfig::builder()
    .space(Box::new(Square4::new(100, 100, EdgeBehavior::Absorb).unwrap()))
    .fields(murk_propagators::reference_fields())
    .propagators(vec![
        Box::new(ScalarDiffusion::builder()
            .input_field(HEAT).output_field(HEAT).coefficient(0.1)
            .build().unwrap()),
        Box::new(ScalarDiffusion::builder()
            .input_field(VELOCITY).output_field(VELOCITY).coefficient(0.1)
            .build().unwrap()),
        Box::new(GradientCompute::builder()
            .input_field(HEAT).output_field(HEAT_GRADIENT)
            .build().unwrap()),
        Box::new(AgentMovementPropagator::new(action_buffer, initial_positions)),
        Box::new(RewardPropagator::new(1.0, -0.01)),
    ])
    .dt(0.1)
    .seed(42)
    .max_ingress_queue(64)
    .backoff(BackoffConfig {
        initial_max_skew: 2,
        backoff_factor: 1.5,
        max_skew_cap: 10,
        decay_rate: 60,
        rejection_rate_threshold: 0.20,
    })
    .build()
    .unwrap();
```

This requires adding imports for `ScalarDiffusion`, `GradientCompute`, `HEAT`, `VELOCITY`, `HEAT_GRADIENT`, `Square4`, `EdgeBehavior`, and `WorldConfig` to this test file. Check what's already imported and add the missing ones.

**Note:** This test is `#[ignore]` (stress test), so it won't run in normal CI. But it must compile.

- [ ] **Step 5: Run integration tests**

Run: `cargo test -p murk-engine --test arena_fragmentation --test nan_detection --test stress_death_spiral`
Expected: all pass

Run: `cargo check -p murk-engine --test stress_rejection_oscillation`
Expected: compiles (test is `#[ignore]` so we only check compilation)

---

## Task 5: Migrate external crates

**Files:**
- Modify: `crates/murk-ffi/src/world.rs` (1 construction)
- Modify: `crates/murk-ffi/src/batched.rs` (1 construction)
- Modify: `crates/murk-propagators/tests/integration.rs` (2 constructions)
- Modify: `crates/murk-propagators/tests/p4_integration.rs` (5 constructions)
- Modify: `crates/murk-propagators/tests/library_integration.rs` (9 constructions)
- Modify: `crates/murk-replay/tests/determinism.rs` (6 constructions)
- Modify: `crates/murk-bench/src/lib.rs` (2 constructions: `reference_profile()` and `stress_profile()`)

### Steps

- [ ] **Step 1: Migrate murk-ffi world.rs**

In `crates/murk-ffi/src/world.rs` around line 101, the FFI `murk_lockstep_create` function constructs a `WorldConfig` from the internal `ConfigBuilder`. Convert:

```rust
let config = WorldConfig::builder()
    .space(space)
    .fields(builder.fields)
    .propagators(builder.propagators)
    .dt(builder.dt)
    .seed(builder.seed)
    .ring_buffer_size(builder.ring_buffer_size)
    .max_ingress_queue(builder.max_ingress_queue)
    .build();

let config = match config {
    Ok(c) => c,
    Err(e) => return MurkStatus::from(&e) as i32,
};

let world = match LockstepWorld::new(config) {
    Ok(w) => w,
    Err(e) => return MurkStatus::from(&e) as i32,
};
```

This removes the manual pre-validation checks (empty fields, empty propagators) at lines 89-99 since `build()` handles those. **Add a comment** preserving the intent of the removed code:

```rust
// The murk-ffi ConfigBuilder (murk-ffi/src/config.rs) is intentionally
// unvalidated — C callers cannot receive Rust Result values during
// incremental builder calls. All validation happens here at world-
// creation time via WorldConfigBuilder::build().
//
// Note: the FFI ConfigBuilder defaults dt to 0.016 (60Hz). This is a
// convenience default for C/Python callers. The Rust-level
// WorldConfigBuilder requires dt to be set explicitly.
```

Also remove the `BackoffConfig` import from this file if no longer needed directly (check whether `BackoffConfig::default()` was used — it was at line 110, but after migration the builder handles that default).

- [ ] **Step 2: Check and migrate murk-ffi batched.rs**

Search `crates/murk-ffi/src/batched.rs` for `WorldConfig {`. If found, apply the same builder pattern. If it delegates to `murk-ffi/src/world.rs`, no migration needed.

- [ ] **Step 3: Migrate murk-propagators test files**

For each of `integration.rs`, `p4_integration.rs`, `library_integration.rs`:
- Search for `WorldConfig {`
- Convert each to `WorldConfig::builder()...build().unwrap()`
- These files likely have helper functions — convert the helpers first, then any inline constructions
- Replace `use murk_engine::WorldConfig;` with `use murk_engine::{WorldConfig, BackoffConfig};` only if BackoffConfig is needed; otherwise just `WorldConfig` suffices since the builder handles defaults

- [ ] **Step 4: Migrate murk-replay/tests/determinism.rs**

Search for `WorldConfig {` (6 struct literal occurrences). Convert each. These likely have a helper function — convert the helper, and inline constructions will follow.

- [ ] **Step 5: Migrate murk-bench/src/lib.rs**

Convert `reference_profile()` (line 27) and `stress_profile()` (line 77):

```rust
pub fn reference_profile(seed: u64, action_buffer: ActionBuffer) -> WorldConfig {
    let cell_count = 100 * 100;
    let initial_positions = init_agent_positions(cell_count, 4, seed);

    WorldConfig::builder()
        .space(Box::new(Square4::new(100, 100, EdgeBehavior::Absorb).unwrap()))
        .fields(murk_propagators::reference_fields())
        .propagators(vec![
            Box::new(
                ScalarDiffusion::builder()
                    .input_field(HEAT)
                    .output_field(HEAT)
                    .coefficient(0.1)
                    .build()
                    .unwrap(),
            ),
            Box::new(
                ScalarDiffusion::builder()
                    .input_field(VELOCITY)
                    .output_field(VELOCITY)
                    .coefficient(0.1)
                    .build()
                    .unwrap(),
            ),
            Box::new(
                GradientCompute::builder()
                    .input_field(HEAT)
                    .output_field(HEAT_GRADIENT)
                    .build()
                    .unwrap(),
            ),
            Box::new(AgentMovementPropagator::new(action_buffer, initial_positions)),
            Box::new(RewardPropagator::new(1.0, -0.01)),
        ])
        .dt(0.1)
        .seed(seed)
        .build()
        .unwrap()
}
```

Same pattern for `stress_profile()` (316x316 grid).

Update the import line to remove `BackoffConfig` if no longer needed:
```rust
use murk_engine::WorldConfig;  // was: use murk_engine::{BackoffConfig, WorldConfig};
```

- [ ] **Step 6: Run full workspace tests**

Run: `cargo test --workspace`
Expected: all tests pass across all crates. **This is the first time `cargo test --workspace` should pass since Task 2.**

- [ ] **Step 7: Commit Tasks 2-5 together**

Squash Tasks 2-5 into a single commit so there are no broken intermediate states in git history:

```
refactor(config): make WorldConfig fields pub(crate), migrate all construction sites

WorldConfig fields are no longer publicly accessible. All construction
in the workspace now uses WorldConfig::builder(). External crates must
use the builder; internal murk-engine code retains pub(crate) access.

Production code in realtime.rs retains direct struct construction for
the ArcSpaceWrapper replacement (documented with comment explaining
why this is safe: TickEngine::new() re-validates).

Breaking change: WorldConfig struct literal syntax no longer works
outside murk-engine. Murk is pre-1.0 with no external consumers.
```

---

## Task 6: Update public API, docs, and examples

**Files:**
- Modify: `crates/murk/src/lib.rs` (doc example + prelude re-export)
- Modify: `crates/murk-engine/examples/quickstart.rs`
- Modify: `crates/murk-engine/examples/replay.rs`
- Modify: `crates/murk-engine/examples/realtime_async.rs`
- Modify: `crates/murk/README.md`
- Modify: `crates/murk-engine/README.md`
- Modify: `book/src/getting-started.md`

### Steps

- [ ] **Step 1: Add WorldConfigBuilder to murk prelude**

In `crates/murk/src/lib.rs` around line 154, add to the prelude:

```rust
pub use murk_engine::{
    AsyncConfig, LockstepWorld, RealtimeAsyncWorld, StepMetrics, StepResult, WorldConfig,
    WorldConfigBuilder,
};
```

- [ ] **Step 2: Update murk/src/lib.rs doc example**

Convert the doc example (lines 37-48) from `WorldConfig { ... }` to builder:

```rust
/// let config = WorldConfig::builder()
///     .space(Box::new(space))
///     .fields(fields)
///     .propagators(vec![Box::new(ZeroFill)])
///     .dt(0.1)
///     .seed(42)
///     .max_ingress_queue(64)
///     .build()
///     .unwrap();
```

- [ ] **Step 3: Migrate examples**

Convert `quickstart.rs`, `replay.rs` (1 struct literal), and `realtime_async.rs` from direct construction to builder. Same mechanical pattern.

- [ ] **Step 4: Update README code examples**

Update `crates/murk/README.md`, `crates/murk-engine/README.md`, and `book/src/getting-started.md` with builder syntax. These are not compiled, so just update the code blocks to match the builder pattern.

- [ ] **Step 5: Run doc tests**

Run: `cargo test --doc -p murk`
Expected: doc example compiles and passes

- [ ] **Step 6: Run examples**

Run: `cargo build --examples -p murk-engine`
Expected: all examples compile

- [ ] **Step 7: Run clippy**

Run: `cargo clippy --workspace --all-targets`
Expected: no warnings related to the builder changes

- [ ] **Step 8: Commit**

```
docs(config): update examples and docs for WorldConfigBuilder

Updates murk prelude, doc examples, engine examples, READMEs, and
book to use the builder pattern.
```

---

## Task 7: Final verification, cleanup, and follow-up tracking

**Files:**
- Modify: `crates/murk-engine/src/config.rs` (update module doc comment)
- Modify: `crates/murk-engine/src/tick.rs` (add comment about intentional double validation)

### Steps

- [ ] **Step 1: Update WorldConfig module doc**

In `config.rs` lines 1-6, the comment says `WorldConfig` is the "builder-input". Replace the module doc:

```rust
//! World configuration, validation, and error types.
//!
//! [`WorldConfig`] holds validated simulation configuration. Construct
//! it via [`WorldConfig::builder()`] → [`WorldConfigBuilder::build()`].
//! The builder runs all validation, so a `WorldConfig` value is always
//! structurally valid.
//!
//! Crate-internal code (e.g., `realtime.rs`) retains `pub(crate)` field
//! access for reconstruction patterns where the space is replaced with
//! an `Arc`-wrapped variant. See [`RealtimeAsyncWorld::new()`] for details.
```

- [ ] **Step 2: Add comment to TickEngine::new() about intentional double validation**

In `crates/murk-engine/src/tick.rs` at line 124, where `config.validate()?;` is called, add a comment:

```rust
// This validate() call is intentional even though WorldConfigBuilder::build()
// also validates. TickEngine::new() may receive configs constructed via
// pub(crate) struct literals (e.g., the ArcSpaceWrapper reconstruction in
// realtime.rs), which bypass the builder. Defense-in-depth: validate here
// regardless of how the config was constructed.
config.validate()?;
```

- [ ] **Step 3: Verify no remaining direct constructions outside murk-engine**

Run: `grep -r 'WorldConfig {' crates/ --include='*.rs' | grep -v 'pub(crate)\|pub struct\|target/' | grep -v 'murk-engine/src/'`
Expected: no matches (all external-crate constructions have been migrated)

Also check within murk-engine for any remaining non-production struct literals that should have been migrated:

Run: `grep -rn 'WorldConfig {' crates/murk-engine/src/ --include='*.rs'`
Expected: only the `realtime.rs` production reconstruction (around line 190)

- [ ] **Step 4: Run full CI equivalent**

Run: `cargo test --workspace && cargo clippy --workspace --all-targets && cargo doc --workspace --no-deps`
Expected: all green

- [ ] **Step 5: Create follow-up issue for BackoffConfig builder**

`BackoffConfig` fields remain `pub` while `WorldConfig` fields are now `pub(crate)`. This asymmetry is intentional for now (BackoffConfig doesn't need build-time validation as complex as WorldConfig's), but should be tracked for future consistency. Create a filigree issue:

```
Title: "BackoffConfig should use validated builder pattern (consistency with WorldConfig)"
Type: task
Priority: P4
Description: BackoffConfig fields remain pub with cross-field invariants
  (initial_max_skew <= max_skew_cap, backoff_factor >= 1.0, etc.) enforced
  only at WorldConfig validation time. For consistency with WorldConfig's
  builder pattern, BackoffConfig could get its own builder. Low priority —
  the invariants are enforced by WorldConfig::validate() so no invalid
  BackoffConfig can exist within a validated WorldConfig. Follow-up from
  murk-439ccbb000.
Labels: ["refactor"]
```

- [ ] **Step 6: Commit**

```
refactor(config): final cleanup for WorldConfig builder migration

Updates module docs, adds defense-in-depth comment to TickEngine::new(),
verifies no remaining external direct constructions. Creates follow-up
issue for BackoffConfig builder pattern.
```
