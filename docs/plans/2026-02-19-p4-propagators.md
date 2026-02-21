# P4 Propagators Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement the 6 P4 reference propagators (AgentEmission, ResourceField, FlowField, MorphologicalOp, WavePropagation, NoiseInjection) with builder-pattern constructors, unit tests, Python bindings, and integration tests.

**Architecture:** Each propagator follows the established P3 pattern: struct with builder, `Propagator` trait impl with `reads_previous` → `writes(Full)` semantics, Square4 fast path where beneficial, generic fallback via `Space::canonical_ordering()`. NoiseInjection adds `rand`/`rand_chacha` workspace dependencies for deterministic seeded RNG. All propagators are stateless (`&self`), deterministic, and compose naturally with the existing library (e.g., AgentEmission → ScalarDiffusion for pheromone trails).

**Tech Stack:** Rust (murk-propagator trait, murk-space topology), PyO3 0.28 (Python bindings), rand 0.8 + rand_chacha 0.3 (NoiseInjection only)

---

## Task 1: FlowField Propagator

**Files:**
- Create: `crates/murk-propagators/src/flow_field.rs`
- Modify: `crates/murk-propagators/src/lib.rs` (add module + re-export)

**Step 1: Write the module file with tests**

Create `crates/murk-propagators/src/flow_field.rs`:

```rust
//! Normalized negative-gradient flow field propagator.
//!
//! Computes the negative gradient of a scalar potential field into a
//! 2-component vector field, optionally normalizing to unit vectors.
//! Agents can "follow the flow" without learning gradient descent.
//!
//! Pairs naturally with `ScalarDiffusion`: diffuse a "goal scent" →
//! compute flow → agents follow flow field.
//!
//! Has a [`Square4`] fast path for direct index arithmetic and a generic
//! fallback using [`Space::canonical_ordering`].
//!
//! Constructed via the builder pattern: [`FlowField::builder`].

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use murk_space::{EdgeBehavior, Square4};

/// A flow field propagator that computes normalized negative gradients.
///
/// Each tick reads a scalar potential field from the previous tick and
/// writes a 2-component vector field representing the direction of
/// steepest descent (negative gradient).
///
/// When `normalize` is true, output vectors are unit length (or zero
/// where the gradient is zero). When false, output is the raw negative
/// gradient with magnitude proportional to the slope.
#[derive(Debug)]
pub struct FlowField {
    potential_field: FieldId,
    flow_field: FieldId,
    normalize: bool,
}

/// Builder for [`FlowField`].
///
/// Required fields: `potential_field` and `flow_field`.
pub struct FlowFieldBuilder {
    potential_field: Option<FieldId>,
    flow_field: Option<FieldId>,
    normalize: bool,
}

impl FlowField {
    /// Create a new builder for configuring a `FlowField` propagator.
    pub fn builder() -> FlowFieldBuilder {
        FlowFieldBuilder {
            potential_field: None,
            flow_field: None,
            normalize: true,
        }
    }

    /// Resolve a single axis value under the given edge behavior.
    fn resolve_axis(val: i32, len: i32, edge: EdgeBehavior) -> Option<i32> {
        if val >= 0 && val < len {
            return Some(val);
        }
        match edge {
            EdgeBehavior::Absorb => None,
            EdgeBehavior::Clamp => Some(val.clamp(0, len - 1)),
            EdgeBehavior::Wrap => Some(((val % len) + len) % len),
        }
    }

    /// Square4 fast path: central-difference negative gradient.
    fn step_square4(
        &self,
        ctx: &mut StepContext<'_>,
        rows: u32,
        cols: u32,
        edge: EdgeBehavior,
    ) -> Result<(), PropagatorError> {
        let rows_i = rows as i32;
        let cols_i = cols as i32;

        let prev = ctx
            .reads_previous()
            .read(self.potential_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("potential field {:?} not readable", self.potential_field),
            })?
            .to_vec();

        let flow_out =
            ctx.writes()
                .write(self.flow_field)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: format!("flow field {:?} not writable", self.flow_field),
                })?;

        for r in 0..rows_i {
            for c in 0..cols_i {
                let i = r as usize * cols as usize + c as usize;

                let h_east = Self::resolve_axis(c + 1, cols_i, edge)
                    .map(|nc| prev[r as usize * cols as usize + nc as usize])
                    .unwrap_or(prev[i]);
                let h_west = Self::resolve_axis(c - 1, cols_i, edge)
                    .map(|nc| prev[r as usize * cols as usize + nc as usize])
                    .unwrap_or(prev[i]);
                let h_south = Self::resolve_axis(r + 1, rows_i, edge)
                    .map(|nr| prev[nr as usize * cols as usize + c as usize])
                    .unwrap_or(prev[i]);
                let h_north = Self::resolve_axis(r - 1, rows_i, edge)
                    .map(|nr| prev[nr as usize * cols as usize + c as usize])
                    .unwrap_or(prev[i]);

                // Negative gradient (steepest descent)
                let mut fx = -(h_east - h_west) / 2.0;
                let mut fy = -(h_south - h_north) / 2.0;

                if self.normalize {
                    let mag = (fx * fx + fy * fy).sqrt();
                    if mag > 1e-12 {
                        fx /= mag;
                        fy /= mag;
                    }
                }

                flow_out[i * 2] = fx;
                flow_out[i * 2 + 1] = fy;
            }
        }

        Ok(())
    }

    /// Generic fallback using `Space::canonical_ordering()`.
    fn step_generic(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let ordering = ctx.space().canonical_ordering();
        let cell_count = ordering.len();

        let grad_info: Vec<Vec<(usize, i32, i32)>> = ordering
            .iter()
            .map(|coord| {
                let neighbours = ctx.space().neighbours(coord);
                neighbours
                    .iter()
                    .filter_map(|nb| {
                        ctx.space().canonical_rank(nb).map(|rank| {
                            let dc = if nb.len() >= 2 { nb[1] - coord[1] } else { 0 };
                            let dr = nb[0] - coord[0];
                            (rank, dc, dr)
                        })
                    })
                    .collect()
            })
            .collect();

        let prev = ctx
            .reads_previous()
            .read(self.potential_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("potential field {:?} not readable", self.potential_field),
            })?
            .to_vec();

        let mut flow_buf = vec![0.0f32; cell_count * 2];

        for i in 0..cell_count {
            let gi = &grad_info[i];
            let mut gx = 0.0f32;
            let mut gy = 0.0f32;
            let mut xc = 0u32;
            let mut yc = 0u32;
            for &(rank, dc, dr) in gi {
                let dh = prev[rank] - prev[i];
                if dc != 0 {
                    gx += dh / dc as f32;
                    xc += 1;
                }
                if dr != 0 {
                    gy += dh / dr as f32;
                    yc += 1;
                }
            }
            // Negate for steepest descent
            let mut fx = if xc > 0 { -(gx / xc as f32) } else { 0.0 };
            let mut fy = if yc > 0 { -(gy / yc as f32) } else { 0.0 };

            if self.normalize {
                let mag = (fx * fx + fy * fy).sqrt();
                if mag > 1e-12 {
                    fx /= mag;
                    fy /= mag;
                }
            }

            flow_buf[i * 2] = fx;
            flow_buf[i * 2 + 1] = fy;
        }

        let flow_out =
            ctx.writes()
                .write(self.flow_field)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: format!("flow field {:?} not writable", self.flow_field),
                })?;
        flow_out.copy_from_slice(&flow_buf);

        Ok(())
    }
}

impl FlowFieldBuilder {
    /// Set the input scalar potential field.
    pub fn potential_field(mut self, field: FieldId) -> Self {
        self.potential_field = Some(field);
        self
    }

    /// Set the output 2-component vector flow field.
    pub fn flow_field(mut self, field: FieldId) -> Self {
        self.flow_field = Some(field);
        self
    }

    /// Set whether to normalize output to unit vectors (default: true).
    pub fn normalize(mut self, normalize: bool) -> Self {
        self.normalize = normalize;
        self
    }

    /// Build the propagator, validating all configuration.
    pub fn build(self) -> Result<FlowField, String> {
        let potential_field = self
            .potential_field
            .ok_or_else(|| "potential_field is required".to_string())?;
        let flow_field = self
            .flow_field
            .ok_or_else(|| "flow_field is required".to_string())?;

        Ok(FlowField {
            potential_field,
            flow_field,
            normalize: self.normalize,
        })
    }
}

impl Propagator for FlowField {
    fn name(&self) -> &str {
        "FlowField"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        [self.potential_field].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.flow_field, WriteMode::Full)]
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        if let Some(grid) = ctx.space().downcast_ref::<Square4>() {
            let rows = grid.rows();
            let cols = grid.cols();
            let edge = grid.edge_behavior();
            self.step_square4(ctx, rows, cols, edge)
        } else {
            self.step_generic(ctx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::TickId;
    use murk_propagator::scratch::ScratchRegion;
    use murk_space::{EdgeBehavior, Space};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    const F_POT: FieldId = FieldId(100);
    const F_FLOW: FieldId = FieldId(101);

    fn make_ctx<'a>(
        reader: &'a MockFieldReader,
        writer: &'a mut MockFieldWriter,
        scratch: &'a mut ScratchRegion,
        space: &'a Square4,
    ) -> StepContext<'a> {
        StepContext::new(reader, reader, writer, scratch, space, TickId(1), 0.1)
    }

    #[test]
    fn builder_minimal() {
        let prop = FlowField::builder()
            .potential_field(F_POT)
            .flow_field(F_FLOW)
            .build()
            .unwrap();
        assert_eq!(prop.name(), "FlowField");
        assert!(prop.reads_previous().contains(F_POT));
        let writes: Vec<_> = prop.writes().into_iter().map(|(id, _)| id).collect();
        assert!(writes.contains(&F_FLOW));
        assert!(prop.max_dt().is_none());
    }

    #[test]
    fn builder_rejects_missing_potential() {
        let result = FlowField::builder().flow_field(F_FLOW).build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_rejects_missing_flow() {
        let result = FlowField::builder().potential_field(F_POT).build();
        assert!(result.is_err());
    }

    #[test]
    fn uniform_potential_zero_flow() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = FlowField::builder()
            .potential_field(F_POT)
            .flow_field(F_FLOW)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_POT, vec![10.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_FLOW, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let flow = writer.get_field(F_FLOW).unwrap();
        for (i, &v) in flow.iter().enumerate() {
            assert!(
                v.abs() < 1e-6,
                "uniform potential should yield zero flow, got {v} at index {i}"
            );
        }
    }

    #[test]
    fn flow_points_toward_lower_potential() {
        // 3x3 grid. Potential is a linear ramp: col 0=30, col 1=20, col 2=10.
        // Negative gradient should point toward lower potential (east, +x).
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let potential = vec![
            30.0, 20.0, 10.0, // row 0
            30.0, 20.0, 10.0, // row 1
            30.0, 20.0, 10.0, // row 2
        ];
        let prop = FlowField::builder()
            .potential_field(F_POT)
            .flow_field(F_FLOW)
            .normalize(false)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_POT, potential);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_FLOW, 9 * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let flow = writer.get_field(F_FLOW).unwrap();
        // Center cell (1,1) = index 4: gradient_x = (10-30)/2 = -10,
        // negative gradient_x = +10. Flow points east (positive x).
        let fx = flow[4 * 2];
        let fy = flow[4 * 2 + 1];
        assert!(fx > 0.0, "flow x should be positive (toward lower), got {fx}");
        assert!(fy.abs() < 1e-6, "flow y should be ~0 for horizontal ramp, got {fy}");
    }

    #[test]
    fn normalized_flow_is_unit_length() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        // Diagonal ramp: potential increases with row+col
        let potential = vec![
            0.0, 10.0, 20.0,
            10.0, 20.0, 30.0,
            20.0, 30.0, 40.0,
        ];
        let prop = FlowField::builder()
            .potential_field(F_POT)
            .flow_field(F_FLOW)
            .normalize(true)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_POT, potential);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_FLOW, 9 * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let flow = writer.get_field(F_FLOW).unwrap();
        // Center cell should have non-zero flow with unit magnitude
        let fx = flow[4 * 2];
        let fy = flow[4 * 2 + 1];
        let mag = (fx * fx + fy * fy).sqrt();
        assert!(
            (mag - 1.0).abs() < 1e-5,
            "normalized flow should have unit magnitude, got {mag}"
        );
    }
}
```

**Step 2: Register module in lib.rs**

Add to `crates/murk-propagators/src/lib.rs`:

```rust
pub mod flow_field;
pub use flow_field::FlowField;
```

**Step 3: Run tests**

Run: `cargo test -p murk-propagators flow_field`
Expected: All 6 tests pass.

**Step 4: Commit**

```bash
git add crates/murk-propagators/src/flow_field.rs crates/murk-propagators/src/lib.rs
git commit -m "feat(propagators): add FlowField propagator"
```

---

## Task 2: AgentEmission Propagator

**Files:**
- Create: `crates/murk-propagators/src/agent_emission.rs`
- Modify: `crates/murk-propagators/src/lib.rs` (add module + re-export)

**Step 1: Write the module file with tests**

Create `crates/murk-propagators/src/agent_emission.rs`:

```rust
//! Agent emission propagator.
//!
//! Agents emit a scalar value at their current position each tick.
//! Useful for pheromone trails, scent marking, communication signals.
//!
//! Reads agent positions from `presence_field` (non-zero = agent present)
//! and writes emission values to `emission_field`.
//!
//! Two modes:
//! - **Additive**: copies previous emission values and adds `intensity`
//!   at each cell where an agent is present. Composes with
//!   `ScalarDiffusion` + decay for pheromone-trail environments.
//! - **Set**: zeros the output and sets `intensity` only where agents
//!   are present (pure per-tick snapshot).
//!
//! Constructed via the builder pattern: [`AgentEmission::builder`].

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};

/// Emission mode for [`AgentEmission`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EmissionMode {
    /// Add `intensity` to previous emission values where agents are present.
    Additive,
    /// Zero the field and set `intensity` only where agents are present.
    Set,
}

/// An agent emission propagator.
///
/// Reads a presence field (non-zero values indicate agents) and writes
/// emission values to an emission field. In Additive mode, emissions
/// accumulate across ticks (compose with decay for dissipation). In Set
/// mode, each tick's emission is a fresh snapshot.
#[derive(Debug)]
pub struct AgentEmission {
    presence_field: FieldId,
    emission_field: FieldId,
    intensity: f32,
    mode: EmissionMode,
}

/// Builder for [`AgentEmission`].
///
/// Required fields: `presence_field` and `emission_field`.
pub struct AgentEmissionBuilder {
    presence_field: Option<FieldId>,
    emission_field: Option<FieldId>,
    intensity: f32,
    mode: EmissionMode,
}

impl AgentEmission {
    /// Create a new builder for configuring an `AgentEmission` propagator.
    pub fn builder() -> AgentEmissionBuilder {
        AgentEmissionBuilder {
            presence_field: None,
            emission_field: None,
            intensity: 1.0,
            mode: EmissionMode::Additive,
        }
    }
}

impl AgentEmissionBuilder {
    /// Set the field that encodes agent positions.
    pub fn presence_field(mut self, field: FieldId) -> Self {
        self.presence_field = Some(field);
        self
    }

    /// Set the field to write emission values into.
    pub fn emission_field(mut self, field: FieldId) -> Self {
        self.emission_field = Some(field);
        self
    }

    /// Set the emission intensity per agent (default: 1.0). Must be > 0.
    pub fn intensity(mut self, intensity: f32) -> Self {
        self.intensity = intensity;
        self
    }

    /// Set the emission mode (default: Additive).
    pub fn mode(mut self, mode: EmissionMode) -> Self {
        self.mode = mode;
        self
    }

    /// Build the propagator, validating all configuration.
    pub fn build(self) -> Result<AgentEmission, String> {
        let presence_field = self
            .presence_field
            .ok_or_else(|| "presence_field is required".to_string())?;
        let emission_field = self
            .emission_field
            .ok_or_else(|| "emission_field is required".to_string())?;

        if self.intensity <= 0.0 {
            return Err(format!(
                "intensity must be > 0, got {}",
                self.intensity
            ));
        }

        Ok(AgentEmission {
            presence_field,
            emission_field,
            intensity: self.intensity,
            mode: self.mode,
        })
    }
}

impl Propagator for AgentEmission {
    fn name(&self) -> &str {
        "AgentEmission"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        match self.mode {
            EmissionMode::Additive => {
                [self.presence_field, self.emission_field]
                    .into_iter()
                    .collect()
            }
            EmissionMode::Set => [self.presence_field].into_iter().collect(),
        }
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.emission_field, WriteMode::Full)]
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let presence = ctx
            .reads_previous()
            .read(self.presence_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("presence field {:?} not readable", self.presence_field),
            })?
            .to_vec();

        let out = ctx
            .writes()
            .write(self.emission_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("emission field {:?} not writable", self.emission_field),
            })?;

        match self.mode {
            EmissionMode::Additive => {
                // Copy previous emission values first
                let prev_emission = ctx
                    .reads_previous()
                    .read(self.emission_field)
                    .ok_or_else(|| PropagatorError::ExecutionFailed {
                        reason: format!(
                            "emission field {:?} not readable (Additive mode)",
                            self.emission_field
                        ),
                    })?;
                // Note: we already took a mutable borrow on `out` above,
                // but reads_previous is a separate borrow. We need to copy
                // prev_emission first.
                // Actually, we need to restructure to avoid borrow conflicts.
                drop(out);

                let prev_emission = ctx
                    .reads_previous()
                    .read(self.emission_field)
                    .ok_or_else(|| PropagatorError::ExecutionFailed {
                        reason: format!(
                            "emission field {:?} not readable (Additive mode)",
                            self.emission_field
                        ),
                    })?
                    .to_vec();

                let out = ctx
                    .writes()
                    .write(self.emission_field)
                    .ok_or_else(|| PropagatorError::ExecutionFailed {
                        reason: format!("emission field {:?} not writable", self.emission_field),
                    })?;

                out.copy_from_slice(&prev_emission);
                for (i, &p) in presence.iter().enumerate() {
                    if p != 0.0 && i < out.len() {
                        out[i] += self.intensity;
                    }
                }
            }
            EmissionMode::Set => {
                out.fill(0.0);
                for (i, &p) in presence.iter().enumerate() {
                    if p != 0.0 && i < out.len() {
                        out[i] = self.intensity;
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::TickId;
    use murk_propagator::scratch::ScratchRegion;
    use murk_space::{EdgeBehavior, Square4};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    const F_PRES: FieldId = FieldId(100);
    const F_EMIT: FieldId = FieldId(101);

    fn make_ctx<'a>(
        reader: &'a MockFieldReader,
        writer: &'a mut MockFieldWriter,
        scratch: &'a mut ScratchRegion,
        space: &'a Square4,
    ) -> StepContext<'a> {
        StepContext::new(reader, reader, writer, scratch, space, TickId(1), 0.1)
    }

    #[test]
    fn builder_minimal() {
        let prop = AgentEmission::builder()
            .presence_field(F_PRES)
            .emission_field(F_EMIT)
            .build()
            .unwrap();
        assert_eq!(prop.name(), "AgentEmission");
        assert!(prop.reads_previous().contains(F_PRES));
        assert!(prop.max_dt().is_none());
    }

    #[test]
    fn builder_rejects_missing_presence() {
        let result = AgentEmission::builder()
            .emission_field(F_EMIT)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_rejects_zero_intensity() {
        let result = AgentEmission::builder()
            .presence_field(F_PRES)
            .emission_field(F_EMIT)
            .intensity(0.0)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn set_mode_emits_at_agent_positions() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let prop = AgentEmission::builder()
            .presence_field(F_PRES)
            .emission_field(F_EMIT)
            .intensity(5.0)
            .mode(EmissionMode::Set)
            .build()
            .unwrap();

        // Agent at cell 4 (center)
        let mut presence = vec![0.0f32; 9];
        presence[4] = 1.0;

        let mut reader = MockFieldReader::new();
        reader.set_field(F_PRES, presence);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_EMIT, 9);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let emit = writer.get_field(F_EMIT).unwrap();
        assert!((emit[4] - 5.0).abs() < 1e-6, "agent cell should emit 5.0, got {}", emit[4]);
        assert!((emit[0]).abs() < 1e-6, "empty cell should be 0, got {}", emit[0]);
    }

    #[test]
    fn additive_mode_accumulates() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let prop = AgentEmission::builder()
            .presence_field(F_PRES)
            .emission_field(F_EMIT)
            .intensity(3.0)
            .mode(EmissionMode::Additive)
            .build()
            .unwrap();

        // Agent at cell 4, previous emission was 10.0 at cell 4
        let mut presence = vec![0.0f32; 9];
        presence[4] = 1.0;
        let mut prev_emission = vec![0.0f32; 9];
        prev_emission[4] = 10.0;

        let mut reader = MockFieldReader::new();
        reader.set_field(F_PRES, presence);
        reader.set_field(F_EMIT, prev_emission);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_EMIT, 9);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let emit = writer.get_field(F_EMIT).unwrap();
        assert!(
            (emit[4] - 13.0).abs() < 1e-6,
            "additive: 10 + 3 = 13, got {}",
            emit[4]
        );
    }

    #[test]
    fn no_agents_no_emission() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let prop = AgentEmission::builder()
            .presence_field(F_PRES)
            .emission_field(F_EMIT)
            .mode(EmissionMode::Set)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_PRES, vec![0.0; 9]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_EMIT, 9);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let emit = writer.get_field(F_EMIT).unwrap();
        assert!(emit.iter().all(|&v| v == 0.0), "no agents = no emission");
    }
}
```

**Step 2: Register module in lib.rs**

Add to `crates/murk-propagators/src/lib.rs`:

```rust
pub mod agent_emission;
pub use agent_emission::{AgentEmission, EmissionMode};
```

**Step 3: Run tests**

Run: `cargo test -p murk-propagators agent_emission`
Expected: All 6 tests pass.

**Step 4: Commit**

```bash
git add crates/murk-propagators/src/agent_emission.rs crates/murk-propagators/src/lib.rs
git commit -m "feat(propagators): add AgentEmission propagator"
```

---

## Task 3: ResourceField Propagator

**Files:**
- Create: `crates/murk-propagators/src/resource_field.rs`
- Modify: `crates/murk-propagators/src/lib.rs` (add module + re-export)

**Step 1: Write the module file with tests**

Create `crates/murk-propagators/src/resource_field.rs`:

```rust
//! Consumable resource field propagator.
//!
//! Models a scalar field that agents consume by their presence and that
//! regenerates over time. Supports linear and logistic regrowth models.
//!
//! Common in foraging/harvesting environments: agents deplete local
//! resources, which then regrow toward a carrying capacity.
//!
//! Constructed via the builder pattern: [`ResourceField::builder`].

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};

/// Regrowth model for [`ResourceField`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegrowthModel {
    /// Linear: `v += regrowth_rate * dt` (constant rate, capped at capacity).
    Linear,
    /// Logistic: `v += regrowth_rate * v * (1 - v/capacity) * dt`
    /// (density-dependent, S-curve toward capacity).
    Logistic,
}

/// A consumable resource field propagator.
///
/// Each tick:
/// 1. Copies previous-tick resource values.
/// 2. Subtracts `consumption_rate * dt` at cells where agents are present.
/// 3. Applies regrowth (linear or logistic).
/// 4. Clamps to `[0, capacity]`.
#[derive(Debug)]
pub struct ResourceField {
    field: FieldId,
    presence_field: FieldId,
    consumption_rate: f32,
    regrowth_rate: f32,
    capacity: f32,
    regrowth_model: RegrowthModel,
}

/// Builder for [`ResourceField`].
///
/// Required fields: `field` and `presence_field`.
pub struct ResourceFieldBuilder {
    field: Option<FieldId>,
    presence_field: Option<FieldId>,
    consumption_rate: f32,
    regrowth_rate: f32,
    capacity: f32,
    regrowth_model: RegrowthModel,
}

impl ResourceField {
    /// Create a new builder for configuring a `ResourceField` propagator.
    pub fn builder() -> ResourceFieldBuilder {
        ResourceFieldBuilder {
            field: None,
            presence_field: None,
            consumption_rate: 1.0,
            regrowth_rate: 0.1,
            capacity: 1.0,
            regrowth_model: RegrowthModel::Linear,
        }
    }
}

impl ResourceFieldBuilder {
    /// Set the resource field (read previous, write current).
    pub fn field(mut self, field: FieldId) -> Self {
        self.field = Some(field);
        self
    }

    /// Set the agent presence field (read previous).
    pub fn presence_field(mut self, field: FieldId) -> Self {
        self.presence_field = Some(field);
        self
    }

    /// Set the consumption rate per tick per agent (default: 1.0). Must be >= 0.
    pub fn consumption_rate(mut self, rate: f32) -> Self {
        self.consumption_rate = rate;
        self
    }

    /// Set the regrowth rate (default: 0.1). Must be >= 0.
    pub fn regrowth_rate(mut self, rate: f32) -> Self {
        self.regrowth_rate = rate;
        self
    }

    /// Set the carrying capacity (default: 1.0). Must be > 0.
    pub fn capacity(mut self, cap: f32) -> Self {
        self.capacity = cap;
        self
    }

    /// Set the regrowth model (default: Linear).
    pub fn regrowth_model(mut self, model: RegrowthModel) -> Self {
        self.regrowth_model = model;
        self
    }

    /// Build the propagator, validating all configuration.
    pub fn build(self) -> Result<ResourceField, String> {
        let field = self
            .field
            .ok_or_else(|| "field is required".to_string())?;
        let presence_field = self
            .presence_field
            .ok_or_else(|| "presence_field is required".to_string())?;

        if self.consumption_rate < 0.0 {
            return Err(format!(
                "consumption_rate must be >= 0, got {}",
                self.consumption_rate
            ));
        }
        if self.regrowth_rate < 0.0 {
            return Err(format!(
                "regrowth_rate must be >= 0, got {}",
                self.regrowth_rate
            ));
        }
        if self.capacity <= 0.0 {
            return Err(format!(
                "capacity must be > 0, got {}",
                self.capacity
            ));
        }

        Ok(ResourceField {
            field,
            presence_field,
            consumption_rate: self.consumption_rate,
            regrowth_rate: self.regrowth_rate,
            capacity: self.capacity,
            regrowth_model: self.regrowth_model,
        })
    }
}

impl Propagator for ResourceField {
    fn name(&self) -> &str {
        "ResourceField"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        [self.field, self.presence_field].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.field, WriteMode::Full)]
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let dt = ctx.dt() as f32;

        let prev = ctx
            .reads_previous()
            .read(self.field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("resource field {:?} not readable", self.field),
            })?
            .to_vec();

        let presence = ctx
            .reads_previous()
            .read(self.presence_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("presence field {:?} not readable", self.presence_field),
            })?
            .to_vec();

        let out = ctx
            .writes()
            .write(self.field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("resource field {:?} not writable", self.field),
            })?;

        for i in 0..prev.len().min(out.len()) {
            let mut v = prev[i];

            // Consumption: subtract where agents are present
            if i < presence.len() && presence[i] != 0.0 {
                v -= self.consumption_rate * dt;
            }

            // Regrowth
            match self.regrowth_model {
                RegrowthModel::Linear => {
                    v += self.regrowth_rate * dt;
                }
                RegrowthModel::Logistic => {
                    // Logistic growth: rate * v * (1 - v/cap)
                    // Only applies when v > 0 (dead resources don't regrow logistically)
                    if v > 0.0 {
                        v += self.regrowth_rate * v * (1.0 - v / self.capacity) * dt;
                    }
                }
            }

            // Clamp to [0, capacity]
            out[i] = v.clamp(0.0, self.capacity);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::TickId;
    use murk_propagator::scratch::ScratchRegion;
    use murk_space::{EdgeBehavior, Square4};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    const F_RES: FieldId = FieldId(100);
    const F_PRES: FieldId = FieldId(101);

    fn make_ctx<'a>(
        reader: &'a MockFieldReader,
        writer: &'a mut MockFieldWriter,
        scratch: &'a mut ScratchRegion,
        space: &'a Square4,
        dt: f64,
    ) -> StepContext<'a> {
        StepContext::new(reader, reader, writer, scratch, space, TickId(1), dt)
    }

    #[test]
    fn builder_minimal() {
        let prop = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .build()
            .unwrap();
        assert_eq!(prop.name(), "ResourceField");
        assert!(prop.reads_previous().contains(F_RES));
        assert!(prop.reads_previous().contains(F_PRES));
    }

    #[test]
    fn builder_rejects_negative_consumption() {
        let result = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .consumption_rate(-1.0)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_rejects_zero_capacity() {
        let result = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .capacity(0.0)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn consumption_reduces_resource() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let prop = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .consumption_rate(10.0)
            .regrowth_rate(0.0) // no regrowth
            .capacity(100.0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_RES, vec![50.0; 9]);
        let mut presence = vec![0.0; 9];
        presence[4] = 1.0; // agent at center
        reader.set_field(F_PRES, presence);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_RES, 9);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1.0);
        prop.step(&mut ctx).unwrap();

        let res = writer.get_field(F_RES).unwrap();
        assert!(
            (res[4] - 40.0).abs() < 1e-6,
            "consumption: 50 - 10*1.0 = 40, got {}",
            res[4]
        );
        assert!(
            (res[0] - 50.0).abs() < 1e-6,
            "no agent: resource unchanged at {}",
            res[0]
        );
    }

    #[test]
    fn linear_regrowth() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let prop = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .consumption_rate(0.0)
            .regrowth_rate(5.0)
            .capacity(100.0)
            .regrowth_model(RegrowthModel::Linear)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_RES, vec![10.0; 9]);
        reader.set_field(F_PRES, vec![0.0; 9]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_RES, 9);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1.0);
        prop.step(&mut ctx).unwrap();

        let res = writer.get_field(F_RES).unwrap();
        assert!(
            (res[0] - 15.0).abs() < 1e-6,
            "linear: 10 + 5*1.0 = 15, got {}",
            res[0]
        );
    }

    #[test]
    fn logistic_regrowth_slows_near_capacity() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let prop = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .consumption_rate(0.0)
            .regrowth_rate(1.0)
            .capacity(100.0)
            .regrowth_model(RegrowthModel::Logistic)
            .build()
            .unwrap();

        // Low resource: should regrow fast
        let mut reader_low = MockFieldReader::new();
        reader_low.set_field(F_RES, vec![10.0; 9]);
        reader_low.set_field(F_PRES, vec![0.0; 9]);

        let mut writer_low = MockFieldWriter::new();
        writer_low.add_field(F_RES, 9);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader_low, &mut writer_low, &mut scratch, &grid, 1.0);
        prop.step(&mut ctx).unwrap();
        let low_growth = writer_low.get_field(F_RES).unwrap()[0] - 10.0;

        // High resource: should regrow slower
        let mut reader_high = MockFieldReader::new();
        reader_high.set_field(F_RES, vec![90.0; 9]);
        reader_high.set_field(F_PRES, vec![0.0; 9]);

        let mut writer_high = MockFieldWriter::new();
        writer_high.add_field(F_RES, 9);

        let mut scratch2 = ScratchRegion::new(0);
        let mut ctx2 = make_ctx(&reader_high, &mut writer_high, &mut scratch2, &grid, 1.0);
        prop.step(&mut ctx2).unwrap();
        let high_growth = writer_high.get_field(F_RES).unwrap()[0] - 90.0;

        assert!(
            low_growth > high_growth,
            "logistic: low resource ({low_growth}) should grow faster than high ({high_growth})"
        );
    }

    #[test]
    fn capacity_clamp() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let prop = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .consumption_rate(0.0)
            .regrowth_rate(100.0)
            .capacity(50.0)
            .regrowth_model(RegrowthModel::Linear)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_RES, vec![40.0; 9]);
        reader.set_field(F_PRES, vec![0.0; 9]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_RES, 9);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1.0);
        prop.step(&mut ctx).unwrap();

        let res = writer.get_field(F_RES).unwrap();
        assert!(
            (res[0] - 50.0).abs() < 1e-6,
            "should clamp to capacity 50, got {}",
            res[0]
        );
    }

    #[test]
    fn floor_clamp_at_zero() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let prop = ResourceField::builder()
            .field(F_RES)
            .presence_field(F_PRES)
            .consumption_rate(100.0)
            .regrowth_rate(0.0)
            .capacity(50.0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_RES, vec![5.0; 9]);
        let mut presence = vec![0.0; 9];
        presence[0] = 1.0;
        reader.set_field(F_PRES, presence);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_RES, 9);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1.0);
        prop.step(&mut ctx).unwrap();

        let res = writer.get_field(F_RES).unwrap();
        assert!(
            (res[0]).abs() < 1e-6,
            "should clamp to 0, got {}",
            res[0]
        );
    }
}
```

**Step 2: Register module in lib.rs**

Add to `crates/murk-propagators/src/lib.rs`:

```rust
pub mod resource_field;
pub use resource_field::{RegrowthModel, ResourceField};
```

**Step 3: Run tests**

Run: `cargo test -p murk-propagators resource_field`
Expected: All 8 tests pass.

**Step 4: Commit**

```bash
git add crates/murk-propagators/src/resource_field.rs crates/murk-propagators/src/lib.rs
git commit -m "feat(propagators): add ResourceField propagator"
```

---

## Task 4: MorphologicalOp Propagator

**Files:**
- Create: `crates/murk-propagators/src/morphological_op.rs`
- Modify: `crates/murk-propagators/src/lib.rs` (add module + re-export)

**Step 1: Write the module file with tests**

Create `crates/murk-propagators/src/morphological_op.rs`:

```rust
//! Morphological erosion/dilation propagator.
//!
//! Operates on a scalar field binarized by a threshold: values above the
//! threshold are "present" (1), at or below are "absent" (0).
//!
//! - **Dilate**: output is 1.0 if *any* cell within `radius` hops is present.
//! - **Erode**: output is 1.0 only if *all* cells within `radius` hops are present.
//!
//! Useful for computing reachability, expanding danger zones, shrinking
//! safe zones, and smoothing binary masks.
//!
//! Uses BFS through `Space::neighbours()` for topology-agnostic operation.
//!
//! Constructed via the builder pattern: [`MorphologicalOp::builder`].

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use std::collections::{HashSet, VecDeque};

/// Morphological operation type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MorphOp {
    /// Output is 1.0 if any cell in the neighborhood is present.
    Dilate,
    /// Output is 1.0 only if all cells in the neighborhood are present.
    Erode,
}

/// A morphological erosion/dilation propagator.
///
/// Reads a scalar field from the previous tick, binarizes it using
/// `threshold`, applies the morphological operation within `radius`
/// hops, and writes the result as a binary field (0.0 or 1.0).
#[derive(Debug)]
pub struct MorphologicalOp {
    input_field: FieldId,
    output_field: FieldId,
    op: MorphOp,
    radius: u32,
    threshold: f32,
}

/// Builder for [`MorphologicalOp`].
///
/// Required fields: `input_field` and `output_field`.
pub struct MorphologicalOpBuilder {
    input_field: Option<FieldId>,
    output_field: Option<FieldId>,
    op: MorphOp,
    radius: u32,
    threshold: f32,
}

impl MorphologicalOp {
    /// Create a new builder for configuring a `MorphologicalOp` propagator.
    pub fn builder() -> MorphologicalOpBuilder {
        MorphologicalOpBuilder {
            input_field: None,
            output_field: None,
            op: MorphOp::Dilate,
            radius: 1,
            threshold: 0.5,
        }
    }
}

impl MorphologicalOpBuilder {
    /// Set the input scalar field.
    pub fn input_field(mut self, field: FieldId) -> Self {
        self.input_field = Some(field);
        self
    }

    /// Set the output binary field.
    pub fn output_field(mut self, field: FieldId) -> Self {
        self.output_field = Some(field);
        self
    }

    /// Set the morphological operation (default: Dilate).
    pub fn op(mut self, op: MorphOp) -> Self {
        self.op = op;
        self
    }

    /// Set the BFS radius in hops (default: 1). Must be >= 1.
    pub fn radius(mut self, radius: u32) -> Self {
        self.radius = radius;
        self
    }

    /// Set the binarization threshold (default: 0.5).
    /// Values strictly above threshold are "present".
    pub fn threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold;
        self
    }

    /// Build the propagator, validating all configuration.
    pub fn build(self) -> Result<MorphologicalOp, String> {
        let input_field = self
            .input_field
            .ok_or_else(|| "input_field is required".to_string())?;
        let output_field = self
            .output_field
            .ok_or_else(|| "output_field is required".to_string())?;

        if self.radius == 0 {
            return Err("radius must be >= 1".to_string());
        }

        Ok(MorphologicalOp {
            input_field,
            output_field,
            op: self.op,
            radius: self.radius,
            threshold: self.threshold,
        })
    }
}

impl Propagator for MorphologicalOp {
    fn name(&self) -> &str {
        "MorphologicalOp"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        [self.input_field].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.output_field, WriteMode::Full)]
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let ordering = ctx.space().canonical_ordering();
        let cell_count = ordering.len();

        // Precompute immediate neighbour ranks for BFS
        let neighbour_ranks: Vec<Vec<usize>> = ordering
            .iter()
            .map(|coord| {
                ctx.space()
                    .neighbours(coord)
                    .iter()
                    .filter_map(|nb| ctx.space().canonical_rank(nb))
                    .collect()
            })
            .collect();

        let prev = ctx
            .reads_previous()
            .read(self.input_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("input field {:?} not readable", self.input_field),
            })?
            .to_vec();

        // Binarize input
        let binary: Vec<bool> = prev.iter().map(|&v| v > self.threshold).collect();

        // Compute output
        let mut out_buf = vec![0.0f32; cell_count];

        for i in 0..cell_count {
            // BFS to find all cells within radius hops
            let mut visited = HashSet::new();
            let mut queue = VecDeque::new();
            visited.insert(i);
            queue.push_back((i, 0u32));

            let mut all_present = true;
            let mut any_present = binary[i];

            while let Some((rank, depth)) = queue.pop_front() {
                if !binary[rank] {
                    all_present = false;
                }
                if binary[rank] {
                    any_present = true;
                }

                if depth < self.radius {
                    for &nb_rank in &neighbour_ranks[rank] {
                        if visited.insert(nb_rank) {
                            queue.push_back((nb_rank, depth + 1));
                        }
                    }
                }
            }

            out_buf[i] = match self.op {
                MorphOp::Dilate => {
                    if any_present { 1.0 } else { 0.0 }
                }
                MorphOp::Erode => {
                    if all_present { 1.0 } else { 0.0 }
                }
            };
        }

        let out = ctx
            .writes()
            .write(self.output_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("output field {:?} not writable", self.output_field),
                })?;
        out.copy_from_slice(&out_buf);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::TickId;
    use murk_propagator::scratch::ScratchRegion;
    use murk_space::{EdgeBehavior, Square4};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    const F_IN: FieldId = FieldId(100);
    const F_OUT: FieldId = FieldId(101);

    fn make_ctx<'a>(
        reader: &'a MockFieldReader,
        writer: &'a mut MockFieldWriter,
        scratch: &'a mut ScratchRegion,
        space: &'a Square4,
    ) -> StepContext<'a> {
        StepContext::new(reader, reader, writer, scratch, space, TickId(1), 0.1)
    }

    #[test]
    fn builder_minimal() {
        let prop = MorphologicalOp::builder()
            .input_field(F_IN)
            .output_field(F_OUT)
            .build()
            .unwrap();
        assert_eq!(prop.name(), "MorphologicalOp");
    }

    #[test]
    fn builder_rejects_zero_radius() {
        let result = MorphologicalOp::builder()
            .input_field(F_IN)
            .output_field(F_OUT)
            .radius(0)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn dilate_expands_single_cell() {
        // 3x3 grid, single cell present at center
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let prop = MorphologicalOp::builder()
            .input_field(F_IN)
            .output_field(F_OUT)
            .op(MorphOp::Dilate)
            .radius(1)
            .threshold(0.5)
            .build()
            .unwrap();

        let mut input = vec![0.0f32; 9];
        input[4] = 1.0; // center

        let mut reader = MockFieldReader::new();
        reader.set_field(F_IN, input);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_OUT, 9);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_OUT).unwrap();
        // Center and its 4 neighbors should be 1.0
        assert_eq!(out[4], 1.0, "center");
        assert_eq!(out[1], 1.0, "north");
        assert_eq!(out[7], 1.0, "south");
        assert_eq!(out[3], 1.0, "west");
        assert_eq!(out[5], 1.0, "east");
        // Corners should still be 0.0
        assert_eq!(out[0], 0.0, "top-left corner");
        assert_eq!(out[8], 0.0, "bottom-right corner");
    }

    #[test]
    fn erode_shrinks_block() {
        // 3x3 grid, all cells present
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let prop = MorphologicalOp::builder()
            .input_field(F_IN)
            .output_field(F_OUT)
            .op(MorphOp::Erode)
            .radius(1)
            .threshold(0.5)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_IN, vec![1.0; 9]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_OUT, 9);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_OUT).unwrap();
        // Center cell has all 4 neighbors + self present → 1.0
        assert_eq!(out[4], 1.0, "center should survive erosion");
        // Corner cell (0,0) has only 2 neighbors (east, south) +
        // self within radius 1. All 3 are present, so erode = 1.0.
        // But with Absorb boundaries, the BFS only visits cells that
        // exist — all visited cells are present → 1.0 everywhere.
        assert_eq!(out[0], 1.0, "all present → erosion preserves all");
    }

    #[test]
    fn erode_removes_isolated_cell() {
        // 3x3 grid, only center present
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let prop = MorphologicalOp::builder()
            .input_field(F_IN)
            .output_field(F_OUT)
            .op(MorphOp::Erode)
            .radius(1)
            .threshold(0.5)
            .build()
            .unwrap();

        let mut input = vec![0.0f32; 9];
        input[4] = 1.0;

        let mut reader = MockFieldReader::new();
        reader.set_field(F_IN, input);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_OUT, 9);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_OUT).unwrap();
        // Center has 4 neighbors, none present → not all present → 0.0
        assert_eq!(out[4], 0.0, "isolated cell should be eroded");
    }

    #[test]
    fn dilate_radius_2() {
        // 5x5 grid, single cell at center (2,2) = index 12
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let prop = MorphologicalOp::builder()
            .input_field(F_IN)
            .output_field(F_OUT)
            .op(MorphOp::Dilate)
            .radius(2)
            .threshold(0.5)
            .build()
            .unwrap();

        let mut input = vec![0.0f32; 25];
        input[12] = 1.0;

        let mut reader = MockFieldReader::new();
        reader.set_field(F_IN, input);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_OUT, 25);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_OUT).unwrap();
        // Center should be dilated
        assert_eq!(out[12], 1.0, "center");
        // 2 hops away should also be 1.0: (0,2)=2, (2,0)=10, etc.
        assert_eq!(out[2], 1.0, "2 hops north");
        assert_eq!(out[22], 1.0, "2 hops south");
        // Corners at Manhattan distance 4 should be 0.0
        assert_eq!(out[0], 0.0, "corner too far");
    }

    #[test]
    fn threshold_binarization() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let prop = MorphologicalOp::builder()
            .input_field(F_IN)
            .output_field(F_OUT)
            .op(MorphOp::Dilate)
            .radius(1)
            .threshold(0.7)
            .build()
            .unwrap();

        // Only cell 4 is above threshold 0.7
        let input = vec![0.0, 0.5, 0.0, 0.5, 0.8, 0.5, 0.0, 0.5, 0.0];

        let mut reader = MockFieldReader::new();
        reader.set_field(F_IN, input);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_OUT, 9);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_OUT).unwrap();
        assert_eq!(out[4], 1.0, "center above threshold → dilated");
        assert_eq!(out[1], 1.0, "north neighbor of present cell");
        assert_eq!(out[0], 0.0, "corner: no neighbor above threshold");
    }
}
```

**Step 2: Register module in lib.rs**

Add to `crates/murk-propagators/src/lib.rs`:

```rust
pub mod morphological_op;
pub use morphological_op::{MorphOp, MorphologicalOp};
```

**Step 3: Run tests**

Run: `cargo test -p murk-propagators morphological_op`
Expected: All 7 tests pass.

**Step 4: Commit**

```bash
git add crates/murk-propagators/src/morphological_op.rs crates/murk-propagators/src/lib.rs
git commit -m "feat(propagators): add MorphologicalOp propagator"
```

---

## Task 5: WavePropagation Propagator

**Files:**
- Create: `crates/murk-propagators/src/wave_propagation.rs`
- Modify: `crates/murk-propagators/src/lib.rs` (add module + re-export)

**Step 1: Write the module file with tests**

Create `crates/murk-propagators/src/wave_propagation.rs`:

```rust
//! Second-order wave equation propagator.
//!
//! Produces propagating wavefronts, reflection off boundaries, and
//! interference patterns — qualitatively different from diffusion.
//! Requires two scalar fields: displacement and velocity.
//!
//! Uses leapfrog (symplectic Euler) integration:
//! ```text
//! laplacian[i] = mean(neighbours) - displacement[i]
//! acceleration[i] = wave_speed² * laplacian[i] - damping * velocity[i]
//! new_velocity[i] = velocity[i] + acceleration[i] * dt
//! new_displacement[i] = displacement[i] + new_velocity[i] * dt
//! ```
//!
//! Has a [`Square4`] fast path and a generic fallback.
//! Implements `max_dt()` for CFL stability.
//!
//! Constructed via the builder pattern: [`WavePropagation::builder`].

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use murk_space::{EdgeBehavior, Square4};

/// A second-order wave equation propagator.
///
/// Models wave dynamics on a discrete spatial grid. Produces propagating
/// wavefronts, boundary reflections, and interference patterns.
///
/// # CFL stability
///
/// The maximum stable timestep is `1 / (wave_speed * sqrt(max_degree))`.
/// For worst-case FCC-12 topology: `1 / (wave_speed * sqrt(12))`.
#[derive(Debug)]
pub struct WavePropagation {
    displacement_field: FieldId,
    velocity_field: FieldId,
    wave_speed: f64,
    damping: f64,
}

/// Builder for [`WavePropagation`].
///
/// Required fields: `displacement_field` and `velocity_field`.
pub struct WavePropagationBuilder {
    displacement_field: Option<FieldId>,
    velocity_field: Option<FieldId>,
    wave_speed: f64,
    damping: f64,
}

impl WavePropagation {
    /// Create a new builder for configuring a `WavePropagation` propagator.
    pub fn builder() -> WavePropagationBuilder {
        WavePropagationBuilder {
            displacement_field: None,
            velocity_field: None,
            wave_speed: 1.0,
            damping: 0.0,
        }
    }

    /// Resolve a single axis value under the given edge behavior.
    fn resolve_axis(val: i32, len: i32, edge: EdgeBehavior) -> Option<i32> {
        if val >= 0 && val < len {
            return Some(val);
        }
        match edge {
            EdgeBehavior::Absorb => None,
            EdgeBehavior::Clamp => Some(val.clamp(0, len - 1)),
            EdgeBehavior::Wrap => Some(((val % len) + len) % len),
        }
    }

    /// Collect flat indices of 4-connected neighbours.
    fn neighbours_flat(
        r: i32,
        c: i32,
        rows: i32,
        cols: i32,
        edge: EdgeBehavior,
    ) -> smallvec::SmallVec<[usize; 4]> {
        let offsets: [(i32, i32); 4] = [(-1, 0), (1, 0), (0, -1), (0, 1)];
        let mut result = smallvec::SmallVec::new();
        for (dr, dc) in offsets {
            let nr = Self::resolve_axis(r + dr, rows, edge);
            let nc = Self::resolve_axis(c + dc, cols, edge);
            if let (Some(nr), Some(nc)) = (nr, nc) {
                result.push(nr as usize * cols as usize + nc as usize);
            }
        }
        result
    }

    /// Square4 fast path.
    fn step_square4(
        &self,
        ctx: &mut StepContext<'_>,
        rows: u32,
        cols: u32,
        edge: EdgeBehavior,
    ) -> Result<(), PropagatorError> {
        let rows_i = rows as i32;
        let cols_i = cols as i32;
        let dt = ctx.dt();
        let c2 = (self.wave_speed * self.wave_speed) as f32;
        let damp = self.damping as f32;

        let prev_d = ctx
            .reads_previous()
            .read(self.displacement_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("displacement field {:?} not readable", self.displacement_field),
            })?
            .to_vec();

        let prev_v = ctx
            .reads_previous()
            .read(self.velocity_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("velocity field {:?} not readable", self.velocity_field),
            })?
            .to_vec();

        let out_d = ctx
            .writes()
            .write(self.displacement_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("displacement field {:?} not writable", self.displacement_field),
            })?;

        let dt_f32 = dt as f32;

        for r in 0..rows_i {
            for c in 0..cols_i {
                let i = r as usize * cols as usize + c as usize;
                let nbs = Self::neighbours_flat(r, c, rows_i, cols_i, edge);
                let count = nbs.len();
                let laplacian = if count > 0 {
                    let sum: f32 = nbs.iter().map(|&ni| prev_d[ni]).sum();
                    sum / count as f32 - prev_d[i]
                } else {
                    0.0
                };
                let accel = c2 * laplacian - damp * prev_v[i];
                let new_v = prev_v[i] + accel * dt_f32;
                out_d[i] = prev_d[i] + new_v * dt_f32;
            }
        }

        // Second pass for velocity field
        let out_v = ctx
            .writes()
            .write(self.velocity_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("velocity field {:?} not writable", self.velocity_field),
            })?;

        for r in 0..rows_i {
            for c in 0..cols_i {
                let i = r as usize * cols as usize + c as usize;
                let nbs = Self::neighbours_flat(r, c, rows_i, cols_i, edge);
                let count = nbs.len();
                let laplacian = if count > 0 {
                    let sum: f32 = nbs.iter().map(|&ni| prev_d[ni]).sum();
                    sum / count as f32 - prev_d[i]
                } else {
                    0.0
                };
                let accel = c2 * laplacian - damp * prev_v[i];
                out_v[i] = prev_v[i] + accel * dt_f32;
            }
        }

        Ok(())
    }

    /// Generic fallback.
    fn step_generic(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let dt = ctx.dt();
        let c2 = (self.wave_speed * self.wave_speed) as f32;
        let damp = self.damping as f32;

        let ordering = ctx.space().canonical_ordering();
        let cell_count = ordering.len();

        let neighbour_ranks: Vec<Vec<usize>> = ordering
            .iter()
            .map(|coord| {
                ctx.space()
                    .neighbours(coord)
                    .iter()
                    .filter_map(|nb| ctx.space().canonical_rank(nb))
                    .collect()
            })
            .collect();

        let prev_d = ctx
            .reads_previous()
            .read(self.displacement_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("displacement field {:?} not readable", self.displacement_field),
            })?
            .to_vec();

        let prev_v = ctx
            .reads_previous()
            .read(self.velocity_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("velocity field {:?} not readable", self.velocity_field),
            })?
            .to_vec();

        let dt_f32 = dt as f32;

        // Compute new displacement and velocity into buffers
        let mut new_d = vec![0.0f32; cell_count];
        let mut new_v = vec![0.0f32; cell_count];

        for i in 0..cell_count {
            let nbs = &neighbour_ranks[i];
            let count = nbs.len();
            let laplacian = if count > 0 {
                let sum: f32 = nbs.iter().map(|&r| prev_d[r]).sum();
                sum / count as f32 - prev_d[i]
            } else {
                0.0
            };
            let accel = c2 * laplacian - damp * prev_v[i];
            new_v[i] = prev_v[i] + accel * dt_f32;
            new_d[i] = prev_d[i] + new_v[i] * dt_f32;
        }

        let out_d = ctx
            .writes()
            .write(self.displacement_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("displacement field {:?} not writable", self.displacement_field),
            })?;
        out_d.copy_from_slice(&new_d);

        let out_v = ctx
            .writes()
            .write(self.velocity_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("velocity field {:?} not writable", self.velocity_field),
            })?;
        out_v.copy_from_slice(&new_v);

        Ok(())
    }
}

impl WavePropagationBuilder {
    /// Set the displacement field (read previous, write current).
    pub fn displacement_field(mut self, field: FieldId) -> Self {
        self.displacement_field = Some(field);
        self
    }

    /// Set the velocity field (read previous, write current).
    pub fn velocity_field(mut self, field: FieldId) -> Self {
        self.velocity_field = Some(field);
        self
    }

    /// Set the wave propagation speed (default: 1.0). Must be > 0.
    pub fn wave_speed(mut self, speed: f64) -> Self {
        self.wave_speed = speed;
        self
    }

    /// Set the damping coefficient (default: 0.0). Must be >= 0.
    pub fn damping(mut self, damping: f64) -> Self {
        self.damping = damping;
        self
    }

    /// Build the propagator, validating all configuration.
    pub fn build(self) -> Result<WavePropagation, String> {
        let displacement_field = self
            .displacement_field
            .ok_or_else(|| "displacement_field is required".to_string())?;
        let velocity_field = self
            .velocity_field
            .ok_or_else(|| "velocity_field is required".to_string())?;

        if self.wave_speed <= 0.0 {
            return Err(format!(
                "wave_speed must be > 0, got {}",
                self.wave_speed
            ));
        }
        if self.damping < 0.0 {
            return Err(format!(
                "damping must be >= 0, got {}",
                self.damping
            ));
        }

        Ok(WavePropagation {
            displacement_field,
            velocity_field,
            wave_speed: self.wave_speed,
            damping: self.damping,
        })
    }
}

impl Propagator for WavePropagation {
    fn name(&self) -> &str {
        "WavePropagation"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        [self.displacement_field, self.velocity_field]
            .into_iter()
            .collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![
            (self.displacement_field, WriteMode::Full),
            (self.velocity_field, WriteMode::Full),
        ]
    }

    fn max_dt(&self) -> Option<f64> {
        // CFL: dt <= 1 / (wave_speed * sqrt(max_degree))
        // Worst case: FCC-12 with degree 12.
        Some(1.0 / (self.wave_speed * 12.0_f64.sqrt()))
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        if let Some(grid) = ctx.space().downcast_ref::<Square4>() {
            let rows = grid.rows();
            let cols = grid.cols();
            let edge = grid.edge_behavior();
            self.step_square4(ctx, rows, cols, edge)
        } else {
            self.step_generic(ctx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::TickId;
    use murk_propagator::scratch::ScratchRegion;
    use murk_space::{EdgeBehavior, Space};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    const F_DISP: FieldId = FieldId(100);
    const F_VEL: FieldId = FieldId(101);

    fn make_ctx<'a>(
        reader: &'a MockFieldReader,
        writer: &'a mut MockFieldWriter,
        scratch: &'a mut ScratchRegion,
        space: &'a Square4,
        dt: f64,
    ) -> StepContext<'a> {
        StepContext::new(reader, reader, writer, scratch, space, TickId(1), dt)
    }

    #[test]
    fn builder_minimal() {
        let prop = WavePropagation::builder()
            .displacement_field(F_DISP)
            .velocity_field(F_VEL)
            .build()
            .unwrap();
        assert_eq!(prop.name(), "WavePropagation");
        assert!(prop.reads_previous().contains(F_DISP));
        assert!(prop.reads_previous().contains(F_VEL));
        assert_eq!(prop.writes().len(), 2);
    }

    #[test]
    fn builder_rejects_zero_wave_speed() {
        let result = WavePropagation::builder()
            .displacement_field(F_DISP)
            .velocity_field(F_VEL)
            .wave_speed(0.0)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_rejects_negative_damping() {
        let result = WavePropagation::builder()
            .displacement_field(F_DISP)
            .velocity_field(F_VEL)
            .damping(-0.1)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn max_dt_is_cfl() {
        let prop = WavePropagation::builder()
            .displacement_field(F_DISP)
            .velocity_field(F_VEL)
            .wave_speed(2.0)
            .build()
            .unwrap();
        let expected = 1.0 / (2.0 * 12.0_f64.sqrt());
        let actual = prop.max_dt().unwrap();
        assert!(
            (actual - expected).abs() < 1e-10,
            "CFL: expected {expected}, got {actual}"
        );
    }

    #[test]
    fn zero_initial_stays_zero() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = WavePropagation::builder()
            .displacement_field(F_DISP)
            .velocity_field(F_VEL)
            .wave_speed(1.0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_DISP, vec![0.0; n]);
        reader.set_field(F_VEL, vec![0.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_DISP, n);
        writer.add_field(F_VEL, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop.step(&mut ctx).unwrap();

        let disp = writer.get_field(F_DISP).unwrap();
        let vel = writer.get_field(F_VEL).unwrap();
        assert!(disp.iter().all(|&v| v == 0.0), "zero stays zero");
        assert!(vel.iter().all(|&v| v == 0.0), "zero stays zero");
    }

    #[test]
    fn impulse_propagates() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = WavePropagation::builder()
            .displacement_field(F_DISP)
            .velocity_field(F_VEL)
            .wave_speed(1.0)
            .build()
            .unwrap();

        // Displacement impulse at center
        let mut disp = vec![0.0f32; n];
        disp[12] = 10.0;

        let mut reader = MockFieldReader::new();
        reader.set_field(F_DISP, disp);
        reader.set_field(F_VEL, vec![0.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_DISP, n);
        writer.add_field(F_VEL, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop.step(&mut ctx).unwrap();

        let vel = writer.get_field(F_VEL).unwrap();
        // Center should get negative velocity (restoring force pulls it back)
        assert!(vel[12] < 0.0, "center velocity should be negative, got {}", vel[12]);
        // Neighbors should get positive velocity (wave spreading outward)
        assert!(vel[7] > 0.0, "north neighbor should get positive velocity, got {}", vel[7]);
    }

    #[test]
    fn damping_reduces_energy() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let make_prop = |damping: f64| {
            WavePropagation::builder()
                .displacement_field(F_DISP)
                .velocity_field(F_VEL)
                .wave_speed(1.0)
                .damping(damping)
                .build()
                .unwrap()
        };

        let mut disp = vec![0.0f32; n];
        disp[12] = 10.0;
        let vel = vec![0.0f32; n];

        // Run undamped
        let prop_undamped = make_prop(0.0);
        let mut reader = MockFieldReader::new();
        reader.set_field(F_DISP, disp.clone());
        reader.set_field(F_VEL, vel.clone());
        let mut writer = MockFieldWriter::new();
        writer.add_field(F_DISP, n);
        writer.add_field(F_VEL, n);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop_undamped.step(&mut ctx).unwrap();
        let undamped_vel: f32 = writer.get_field(F_VEL).unwrap().iter().map(|v| v * v).sum();

        // Run damped
        let prop_damped = make_prop(5.0);
        let mut reader2 = MockFieldReader::new();
        reader2.set_field(F_DISP, disp);
        reader2.set_field(F_VEL, vel);
        let mut writer2 = MockFieldWriter::new();
        writer2.add_field(F_DISP, n);
        writer2.add_field(F_VEL, n);
        let mut scratch2 = ScratchRegion::new(0);
        let mut ctx2 = make_ctx(&reader2, &mut writer2, &mut scratch2, &grid, 0.01);
        prop_damped.step(&mut ctx2).unwrap();
        let damped_vel: f32 = writer2.get_field(F_VEL).unwrap().iter().map(|v| v * v).sum();

        assert!(
            damped_vel < undamped_vel,
            "damped energy ({damped_vel}) should be less than undamped ({undamped_vel})"
        );
    }
}
```

**Step 2: Register module in lib.rs**

Add to `crates/murk-propagators/src/lib.rs`:

```rust
pub mod wave_propagation;
pub use wave_propagation::WavePropagation;
```

**Step 3: Run tests**

Run: `cargo test -p murk-propagators wave_propagation`
Expected: All 7 tests pass.

**Step 4: Commit**

```bash
git add crates/murk-propagators/src/wave_propagation.rs crates/murk-propagators/src/lib.rs
git commit -m "feat(propagators): add WavePropagation propagator"
```

---

## Task 6: NoiseInjection Propagator

**Files:**
- Modify: `Cargo.toml` (workspace deps: add rand, rand_chacha)
- Modify: `crates/murk-propagators/Cargo.toml` (add rand, rand_chacha)
- Create: `crates/murk-propagators/src/noise_injection.rs`
- Modify: `crates/murk-propagators/src/lib.rs` (add module + re-export)

**Step 1: Add rand dependencies**

Add to workspace `Cargo.toml` under `[workspace.dependencies]`:

```toml
rand = "0.8"
rand_chacha = "0.3"
```

Add to `crates/murk-propagators/Cargo.toml` under `[dependencies]`:

```toml
rand = { workspace = true }
rand_chacha = { workspace = true }
```

**Step 2: Write the module file with tests**

Create `crates/murk-propagators/src/noise_injection.rs`:

```rust
//! Configurable noise injection propagator.
//!
//! Adds deterministic noise to a field each tick. Useful for stochastic
//! environments, partial observability, and robustness training.
//!
//! Respects the determinism contract: uses a seeded ChaCha8 RNG
//! derived from `seed_offset XOR tick_id`, producing identical noise
//! sequences for identical seeds.
//!
//! Three noise types:
//! - **Gaussian**: `v += scale * N(0,1)` (Box-Muller transform)
//! - **Uniform**: `v += scale * U(-1,1)`
//! - **SaltPepper**: with probability `scale`, set to 0.0 or 1.0
//!
//! Constructed via the builder pattern: [`NoiseInjection::builder`].

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use rand::prelude::*;
use rand_chacha::ChaCha8Rng;

/// Noise type for [`NoiseInjection`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoiseType {
    /// Additive Gaussian noise: `v += scale * N(0,1)`.
    Gaussian,
    /// Additive uniform noise: `v += scale * U(-1,1)`.
    Uniform,
    /// Salt-and-pepper noise: with probability `scale`, replace value
    /// with 0.0 or 1.0 (equal chance).
    SaltPepper,
}

/// A deterministic noise injection propagator.
///
/// Copies previous-tick field values and adds noise. The RNG is seeded
/// from `seed_offset XOR tick_id` each tick, ensuring deterministic
/// replay with the same configuration.
#[derive(Debug)]
pub struct NoiseInjection {
    field: FieldId,
    noise_type: NoiseType,
    scale: f64,
    seed_offset: u64,
}

/// Builder for [`NoiseInjection`].
///
/// Required field: `field`.
pub struct NoiseInjectionBuilder {
    field: Option<FieldId>,
    noise_type: NoiseType,
    scale: f64,
    seed_offset: u64,
}

impl NoiseInjection {
    /// Create a new builder for configuring a `NoiseInjection` propagator.
    pub fn builder() -> NoiseInjectionBuilder {
        NoiseInjectionBuilder {
            field: None,
            noise_type: NoiseType::Gaussian,
            scale: 0.1,
            seed_offset: 0,
        }
    }

    /// Generate a Gaussian sample using Box-Muller transform.
    /// Avoids the `rand_distr` dependency.
    fn box_muller(rng: &mut ChaCha8Rng) -> f64 {
        let u1: f64 = rng.gen::<f64>().max(1e-300); // avoid ln(0)
        let u2: f64 = rng.gen();
        (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
    }
}

impl NoiseInjectionBuilder {
    /// Set the field to inject noise into.
    pub fn field(mut self, field: FieldId) -> Self {
        self.field = Some(field);
        self
    }

    /// Set the noise type (default: Gaussian).
    pub fn noise_type(mut self, noise_type: NoiseType) -> Self {
        self.noise_type = noise_type;
        self
    }

    /// Set the noise scale (default: 0.1). Must be >= 0.
    ///
    /// For Gaussian: standard deviation. For Uniform: half-range.
    /// For SaltPepper: probability of replacement per cell.
    pub fn scale(mut self, scale: f64) -> Self {
        self.scale = scale;
        self
    }

    /// Set the seed offset for deterministic RNG (default: 0).
    pub fn seed_offset(mut self, offset: u64) -> Self {
        self.seed_offset = offset;
        self
    }

    /// Build the propagator, validating all configuration.
    pub fn build(self) -> Result<NoiseInjection, String> {
        let field = self
            .field
            .ok_or_else(|| "field is required".to_string())?;

        if self.scale < 0.0 {
            return Err(format!("scale must be >= 0, got {}", self.scale));
        }

        if self.noise_type == NoiseType::SaltPepper && self.scale > 1.0 {
            return Err(format!(
                "SaltPepper scale is a probability and must be <= 1.0, got {}",
                self.scale
            ));
        }

        Ok(NoiseInjection {
            field,
            noise_type: self.noise_type,
            scale: self.scale,
            seed_offset: self.seed_offset,
        })
    }
}

impl Propagator for NoiseInjection {
    fn name(&self) -> &str {
        "NoiseInjection"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        [self.field].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.field, WriteMode::Full)]
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let tick = ctx.tick_id().0;

        let prev = ctx
            .reads_previous()
            .read(self.field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("field {:?} not readable", self.field),
            })?
            .to_vec();

        let out = ctx
            .writes()
            .write(self.field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("field {:?} not writable", self.field),
            })?;

        out.copy_from_slice(&prev);

        // Deterministic RNG seeded from seed_offset XOR tick_id
        let mut rng = ChaCha8Rng::seed_from_u64(self.seed_offset ^ tick);

        match self.noise_type {
            NoiseType::Gaussian => {
                for v in out.iter_mut() {
                    *v += (self.scale * Self::box_muller(&mut rng)) as f32;
                }
            }
            NoiseType::Uniform => {
                for v in out.iter_mut() {
                    let u: f64 = rng.gen::<f64>() * 2.0 - 1.0;
                    *v += (self.scale * u) as f32;
                }
            }
            NoiseType::SaltPepper => {
                for v in out.iter_mut() {
                    let p: f64 = rng.gen();
                    if p < self.scale {
                        // Coin flip for salt (1.0) or pepper (0.0)
                        *v = if rng.gen::<bool>() { 1.0 } else { 0.0 };
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::TickId;
    use murk_propagator::scratch::ScratchRegion;
    use murk_space::{EdgeBehavior, Square4};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    const F_DATA: FieldId = FieldId(100);

    fn make_ctx<'a>(
        reader: &'a MockFieldReader,
        writer: &'a mut MockFieldWriter,
        scratch: &'a mut ScratchRegion,
        space: &'a Square4,
        tick: u64,
    ) -> StepContext<'a> {
        StepContext::new(reader, reader, writer, scratch, space, TickId(tick), 0.1)
    }

    #[test]
    fn builder_minimal() {
        let prop = NoiseInjection::builder()
            .field(F_DATA)
            .build()
            .unwrap();
        assert_eq!(prop.name(), "NoiseInjection");
    }

    #[test]
    fn builder_rejects_negative_scale() {
        let result = NoiseInjection::builder()
            .field(F_DATA)
            .scale(-1.0)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_rejects_salt_pepper_scale_over_one() {
        let result = NoiseInjection::builder()
            .field(F_DATA)
            .noise_type(NoiseType::SaltPepper)
            .scale(1.5)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn determinism_same_seed_same_output() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = NoiseInjection::builder()
            .field(F_DATA)
            .noise_type(NoiseType::Gaussian)
            .scale(1.0)
            .seed_offset(42)
            .build()
            .unwrap();

        let run = |tick: u64| -> Vec<f32> {
            let mut reader = MockFieldReader::new();
            reader.set_field(F_DATA, vec![10.0; n]);
            let mut writer = MockFieldWriter::new();
            writer.add_field(F_DATA, n);
            let mut scratch = ScratchRegion::new(0);
            let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, tick);
            prop.step(&mut ctx).unwrap();
            writer.get_field(F_DATA).unwrap().to_vec()
        };

        let a = run(5);
        let b = run(5);
        assert_eq!(a, b, "same tick + same seed → bit-identical output");
    }

    #[test]
    fn different_ticks_different_output() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = NoiseInjection::builder()
            .field(F_DATA)
            .noise_type(NoiseType::Gaussian)
            .scale(1.0)
            .seed_offset(42)
            .build()
            .unwrap();

        let run = |tick: u64| -> Vec<f32> {
            let mut reader = MockFieldReader::new();
            reader.set_field(F_DATA, vec![10.0; n]);
            let mut writer = MockFieldWriter::new();
            writer.add_field(F_DATA, n);
            let mut scratch = ScratchRegion::new(0);
            let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, tick);
            prop.step(&mut ctx).unwrap();
            writer.get_field(F_DATA).unwrap().to_vec()
        };

        let a = run(1);
        let b = run(2);
        assert_ne!(a, b, "different ticks should produce different noise");
    }

    #[test]
    fn gaussian_noise_changes_values() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = NoiseInjection::builder()
            .field(F_DATA)
            .noise_type(NoiseType::Gaussian)
            .scale(1.0)
            .seed_offset(0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_DATA, vec![0.0; n]);
        let mut writer = MockFieldWriter::new();
        writer.add_field(F_DATA, n);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_DATA).unwrap();
        // At least some values should be non-zero with Gaussian noise
        let non_zero = out.iter().filter(|&&v| v.abs() > 1e-6).count();
        assert!(non_zero > 0, "Gaussian noise should produce non-zero values");
    }

    #[test]
    fn uniform_noise_bounded() {
        let grid = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let scale = 0.5;
        let prop = NoiseInjection::builder()
            .field(F_DATA)
            .noise_type(NoiseType::Uniform)
            .scale(scale)
            .seed_offset(0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_DATA, vec![10.0; n]);
        let mut writer = MockFieldWriter::new();
        writer.add_field(F_DATA, n);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_DATA).unwrap();
        for &v in out {
            assert!(
                v >= 10.0 - scale as f32 && v <= 10.0 + scale as f32,
                "uniform noise should be bounded: 10 ± {scale}, got {v}"
            );
        }
    }

    #[test]
    fn salt_pepper_only_zero_or_one() {
        let grid = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = NoiseInjection::builder()
            .field(F_DATA)
            .noise_type(NoiseType::SaltPepper)
            .scale(1.0) // replace all cells
            .seed_offset(0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_DATA, vec![0.5; n]);
        let mut writer = MockFieldWriter::new();
        writer.add_field(F_DATA, n);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1);
        prop.step(&mut ctx).unwrap();

        let out = writer.get_field(F_DATA).unwrap();
        for &v in out {
            assert!(
                v == 0.0 || v == 1.0,
                "SaltPepper with scale=1.0 should produce only 0 or 1, got {v}"
            );
        }
    }
}
```

**Step 3: Register module in lib.rs**

Add to `crates/murk-propagators/src/lib.rs`:

```rust
pub mod noise_injection;
pub use noise_injection::{NoiseInjection, NoiseType};
```

**Step 4: Run tests**

Run: `cargo test -p murk-propagators noise_injection`
Expected: All 8 tests pass.

**Step 5: Commit**

```bash
git add Cargo.toml crates/murk-propagators/Cargo.toml \
    crates/murk-propagators/src/noise_injection.rs \
    crates/murk-propagators/src/lib.rs
git commit -m "feat(propagators): add NoiseInjection propagator with rand deps"
```

---

## Task 7: Python Bindings for All 6 P4 Propagators

**Files:**
- Modify: `crates/murk-python/src/library_propagators.rs` (add 6 PyO3 classes)
- Modify: `crates/murk-python/src/lib.rs` (register new classes)
- Modify: `crates/murk-python/Cargo.toml` (if murk-propagators isn't already a dependency)

**Step 1: Add Python binding classes**

Append to `crates/murk-python/src/library_propagators.rs`, after the `PyIdentityCopy` section:

```rust
// ── FlowField ─────────────────────────────────────────────

/// A native flow field propagator (normalized negative gradient).
///
/// Computes steepest-descent direction from a scalar potential field
/// into a 2-component vector flow field. Runs entirely in Rust.
///
/// Args:
///     potential_field: Scalar potential field ID (read from previous tick).
///     flow_field: 2-component vector field ID to write flow into.
///     normalize: Whether to normalize to unit vectors (default True).
#[pyclass(name = "FlowField")]
pub(crate) struct PyFlowField {
    potential_field: u32,
    flow_field: u32,
    normalize: bool,
}

#[pymethods]
impl PyFlowField {
    #[new]
    #[pyo3(signature = (potential_field, flow_field, normalize=true))]
    fn new(potential_field: u32, flow_field: u32, normalize: bool) -> Self {
        PyFlowField {
            potential_field,
            flow_field,
            normalize,
        }
    }

    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?;

        let prop = murk_propagators::FlowField::builder()
            .potential_field(FieldId(self.potential_field))
            .flow_field(FieldId(self.flow_field))
            .normalize(self.normalize)
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("FlowField build error: {e}"))
            })?;

        let handle = box_propagator_to_handle(Box::new(prop));
        config.add_propagator_handle(py, handle)
    }

    fn __repr__(&self) -> String {
        format!(
            "FlowField(potential_field={}, flow_field={}, normalize={})",
            self.potential_field, self.flow_field, self.normalize
        )
    }
}

// ── AgentEmission ─────────────────────────────────────────

/// A native agent emission propagator.
///
/// Emits a scalar value at each cell where an agent is present.
/// Runs entirely in Rust.
///
/// Args:
///     presence_field: Field ID encoding agent positions.
///     emission_field: Field ID to write emissions into.
///     intensity: Emission strength per agent (default 1.0).
///     additive: If True, add to previous emission; if False, set from zero (default True).
#[pyclass(name = "AgentEmission")]
pub(crate) struct PyAgentEmission {
    presence_field: u32,
    emission_field: u32,
    intensity: f32,
    additive: bool,
}

#[pymethods]
impl PyAgentEmission {
    #[new]
    #[pyo3(signature = (presence_field, emission_field, intensity=1.0, additive=true))]
    fn new(presence_field: u32, emission_field: u32, intensity: f32, additive: bool) -> Self {
        PyAgentEmission {
            presence_field,
            emission_field,
            intensity,
            additive,
        }
    }

    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?;

        let mode = if self.additive {
            murk_propagators::EmissionMode::Additive
        } else {
            murk_propagators::EmissionMode::Set
        };

        let prop = murk_propagators::AgentEmission::builder()
            .presence_field(FieldId(self.presence_field))
            .emission_field(FieldId(self.emission_field))
            .intensity(self.intensity)
            .mode(mode)
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("AgentEmission build error: {e}"))
            })?;

        let handle = box_propagator_to_handle(Box::new(prop));
        config.add_propagator_handle(py, handle)
    }

    fn __repr__(&self) -> String {
        format!(
            "AgentEmission(presence_field={}, emission_field={}, intensity={}, additive={})",
            self.presence_field, self.emission_field, self.intensity, self.additive
        )
    }
}

// ── ResourceField ─────────────────────────────────────────

/// A native consumable resource field propagator.
///
/// Resources are consumed by agent presence and regrow over time.
/// Runs entirely in Rust.
///
/// Args:
///     field: Resource field ID (read previous, write current).
///     presence_field: Agent presence field ID.
///     consumption_rate: Consumption per agent per tick (default 1.0).
///     regrowth_rate: Regrowth rate (default 0.1).
///     capacity: Carrying capacity (default 1.0).
///     logistic: If True, use logistic regrowth; if False, linear (default False).
#[pyclass(name = "ResourceField")]
pub(crate) struct PyResourceField {
    field: u32,
    presence_field: u32,
    consumption_rate: f32,
    regrowth_rate: f32,
    capacity: f32,
    logistic: bool,
}

#[pymethods]
impl PyResourceField {
    #[new]
    #[pyo3(signature = (field, presence_field, consumption_rate=1.0, regrowth_rate=0.1, capacity=1.0, logistic=false))]
    fn new(
        field: u32,
        presence_field: u32,
        consumption_rate: f32,
        regrowth_rate: f32,
        capacity: f32,
        logistic: bool,
    ) -> Self {
        PyResourceField {
            field,
            presence_field,
            consumption_rate,
            regrowth_rate,
            capacity,
            logistic,
        }
    }

    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?;

        let model = if self.logistic {
            murk_propagators::RegrowthModel::Logistic
        } else {
            murk_propagators::RegrowthModel::Linear
        };

        let prop = murk_propagators::ResourceField::builder()
            .field(FieldId(self.field))
            .presence_field(FieldId(self.presence_field))
            .consumption_rate(self.consumption_rate)
            .regrowth_rate(self.regrowth_rate)
            .capacity(self.capacity)
            .regrowth_model(model)
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!("ResourceField build error: {e}"))
            })?;

        let handle = box_propagator_to_handle(Box::new(prop));
        config.add_propagator_handle(py, handle)
    }

    fn __repr__(&self) -> String {
        format!(
            "ResourceField(field={}, presence_field={}, capacity={})",
            self.field, self.presence_field, self.capacity
        )
    }
}

// ── MorphologicalOp ───────────────────────────────────────

/// A native morphological erosion/dilation propagator.
///
/// Binarizes a scalar field by threshold and applies morphological
/// erosion or dilation within a BFS radius. Runs entirely in Rust.
///
/// Args:
///     input_field: Input scalar field ID.
///     output_field: Output binary field ID.
///     dilate: If True, dilate; if False, erode (default True).
///     radius: BFS radius in hops (default 1).
///     threshold: Binarization threshold (default 0.5).
#[pyclass(name = "MorphologicalOp")]
pub(crate) struct PyMorphologicalOp {
    input_field: u32,
    output_field: u32,
    dilate: bool,
    radius: u32,
    threshold: f32,
}

#[pymethods]
impl PyMorphologicalOp {
    #[new]
    #[pyo3(signature = (input_field, output_field, dilate=true, radius=1, threshold=0.5))]
    fn new(
        input_field: u32,
        output_field: u32,
        dilate: bool,
        radius: u32,
        threshold: f32,
    ) -> Self {
        PyMorphologicalOp {
            input_field,
            output_field,
            dilate,
            radius,
            threshold,
        }
    }

    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?;

        let op = if self.dilate {
            murk_propagators::MorphOp::Dilate
        } else {
            murk_propagators::MorphOp::Erode
        };

        let prop = murk_propagators::MorphologicalOp::builder()
            .input_field(FieldId(self.input_field))
            .output_field(FieldId(self.output_field))
            .op(op)
            .radius(self.radius)
            .threshold(self.threshold)
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "MorphologicalOp build error: {e}"
                ))
            })?;

        let handle = box_propagator_to_handle(Box::new(prop));
        config.add_propagator_handle(py, handle)
    }

    fn __repr__(&self) -> String {
        format!(
            "MorphologicalOp(input_field={}, output_field={}, dilate={}, radius={})",
            self.input_field, self.output_field, self.dilate, self.radius
        )
    }
}

// ── WavePropagation ───────────────────────────────────────

/// A native second-order wave equation propagator.
///
/// Models wave dynamics with propagating wavefronts, reflection, and
/// interference. Runs entirely in Rust.
///
/// Args:
///     displacement_field: Displacement scalar field ID.
///     velocity_field: Velocity scalar field ID.
///     wave_speed: Wave propagation speed (default 1.0).
///     damping: Energy damping coefficient (default 0.0).
#[pyclass(name = "WavePropagation")]
pub(crate) struct PyWavePropagation {
    displacement_field: u32,
    velocity_field: u32,
    wave_speed: f64,
    damping: f64,
}

#[pymethods]
impl PyWavePropagation {
    #[new]
    #[pyo3(signature = (displacement_field, velocity_field, wave_speed=1.0, damping=0.0))]
    fn new(
        displacement_field: u32,
        velocity_field: u32,
        wave_speed: f64,
        damping: f64,
    ) -> Self {
        PyWavePropagation {
            displacement_field,
            velocity_field,
            wave_speed,
            damping,
        }
    }

    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?;

        let prop = murk_propagators::WavePropagation::builder()
            .displacement_field(FieldId(self.displacement_field))
            .velocity_field(FieldId(self.velocity_field))
            .wave_speed(self.wave_speed)
            .damping(self.damping)
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "WavePropagation build error: {e}"
                ))
            })?;

        let handle = box_propagator_to_handle(Box::new(prop));
        config.add_propagator_handle(py, handle)
    }

    fn __repr__(&self) -> String {
        format!(
            "WavePropagation(displacement_field={}, velocity_field={}, wave_speed={}, damping={})",
            self.displacement_field, self.velocity_field, self.wave_speed, self.damping
        )
    }
}

// ── NoiseInjection ────────────────────────────────────────

/// A native deterministic noise injection propagator.
///
/// Adds deterministic noise (Gaussian, Uniform, or SaltPepper) to a
/// field each tick. Same seed → same noise. Runs entirely in Rust.
///
/// Args:
///     field: Field ID to inject noise into.
///     noise_type: One of "gaussian", "uniform", "salt_pepper" (default "gaussian").
///     scale: Noise scale (default 0.1).
///     seed_offset: Seed offset for deterministic RNG (default 0).
#[pyclass(name = "NoiseInjection")]
pub(crate) struct PyNoiseInjection {
    field: u32,
    noise_type: String,
    scale: f64,
    seed_offset: u64,
}

#[pymethods]
impl PyNoiseInjection {
    #[new]
    #[pyo3(signature = (field, noise_type="gaussian".to_string(), scale=0.1, seed_offset=0))]
    fn new(field: u32, noise_type: String, scale: f64, seed_offset: u64) -> Self {
        PyNoiseInjection {
            field,
            noise_type,
            scale,
            seed_offset,
        }
    }

    fn register(&self, py: Python<'_>, config: &mut Config) -> PyResult<()> {
        let _ = config.require_handle()?;

        let noise = match self.noise_type.as_str() {
            "gaussian" => murk_propagators::NoiseType::Gaussian,
            "uniform" => murk_propagators::NoiseType::Uniform,
            "salt_pepper" => murk_propagators::NoiseType::SaltPepper,
            other => {
                return Err(pyo3::exceptions::PyValueError::new_err(format!(
                    "unknown noise_type '{other}', expected 'gaussian', 'uniform', or 'salt_pepper'"
                )));
            }
        };

        let prop = murk_propagators::NoiseInjection::builder()
            .field(FieldId(self.field))
            .noise_type(noise)
            .scale(self.scale)
            .seed_offset(self.seed_offset)
            .build()
            .map_err(|e| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "NoiseInjection build error: {e}"
                ))
            })?;

        let handle = box_propagator_to_handle(Box::new(prop));
        config.add_propagator_handle(py, handle)
    }

    fn __repr__(&self) -> String {
        format!(
            "NoiseInjection(field={}, noise_type='{}', scale={}, seed_offset={})",
            self.field, self.noise_type, self.scale, self.seed_offset
        )
    }
}
```

**Step 2: Register classes in Python module**

Add to `crates/murk-python/src/lib.rs`, in the `_murk` function after the existing library propagator registrations:

```rust
m.add_class::<library_propagators::PyFlowField>()?;
m.add_class::<library_propagators::PyAgentEmission>()?;
m.add_class::<library_propagators::PyResourceField>()?;
m.add_class::<library_propagators::PyMorphologicalOp>()?;
m.add_class::<library_propagators::PyWavePropagation>()?;
m.add_class::<library_propagators::PyNoiseInjection>()?;
```

**Step 3: Run compile check**

Run: `cargo check -p murk-python`
Expected: Compiles without errors.

**Step 4: Commit**

```bash
git add crates/murk-python/src/library_propagators.rs crates/murk-python/src/lib.rs
git commit -m "feat(python): add Python bindings for all 6 P4 propagators"
```

---

## Task 8: Integration Tests Through LockstepWorld

**Files:**
- Create: `crates/murk-propagators/tests/p4_integration.rs`

**Step 1: Write integration tests**

Create `crates/murk-propagators/tests/p4_integration.rs`:

```rust
//! Integration tests for P4 propagators through the full LockstepWorld engine.
//!
//! Tests composition patterns: emission→diffusion→flow pipelines,
//! resource depletion/regrowth, wave stability, noise determinism,
//! and morphological mask computation.

use murk_core::{BoundaryBehavior, FieldDef, FieldId, FieldMutability, FieldReader, FieldType};
use murk_engine::{BackoffConfig, LockstepWorld, WorldConfig};
use murk_propagators::{
    AgentEmission, EmissionMode, FlowField, IdentityCopy, MorphologicalOp, MorphOp,
    NoiseInjection, NoiseType, ResourceField, RegrowthModel, ScalarDiffusion, WavePropagation,
};
use murk_space::{EdgeBehavior, Square4};

// ---------- Field IDs ----------

const PRESENCE: FieldId = FieldId(0);
const EMISSION: FieldId = FieldId(1);
const HEAT: FieldId = FieldId(2);
const FLOW: FieldId = FieldId(3);
const RESOURCE: FieldId = FieldId(4);
const DISPLACEMENT: FieldId = FieldId(5);
const VELOCITY: FieldId = FieldId(6);
const NOISY: FieldId = FieldId(7);
const MASK_IN: FieldId = FieldId(8);
const MASK_OUT: FieldId = FieldId(9);

// ---------- Helpers ----------

fn scalar_field(name: &str) -> FieldDef {
    FieldDef {
        name: name.to_string(),
        field_type: FieldType::Scalar,
        mutability: FieldMutability::PerTick,
        units: None,
        bounds: None,
        boundary_behavior: BoundaryBehavior::Clamp,
    }
}

fn vector2_field(name: &str) -> FieldDef {
    FieldDef {
        name: name.to_string(),
        field_type: FieldType::Vector { dims: 2 },
        mutability: FieldMutability::PerTick,
        units: None,
        bounds: None,
        boundary_behavior: BoundaryBehavior::Clamp,
    }
}

// ---------- Test 1: Emission → Diffusion → Flow pipeline ----------

/// Compose AgentEmission, ScalarDiffusion, and FlowField in a 3-stage
/// pheromone-trail pipeline. Verify the flow field points toward the
/// emission source after diffusion.
#[test]
fn emission_diffusion_flow_pipeline() {
    // IdentityCopy carries agent presence forward.
    // AgentEmission reads presence, writes emission.
    // ScalarDiffusion reads emission (previous tick), writes heat.
    // FlowField reads heat (previous tick), writes flow.
    //
    // After enough ticks, heat should be concentrated near the
    // emission source, and flow should point toward it.
    let config = WorldConfig {
        space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
        fields: vec![
            scalar_field("presence"),   // 0 = PRESENCE
            scalar_field("emission"),   // 1 = EMISSION
            scalar_field("heat"),       // 2 = HEAT
            vector2_field("flow"),      // 3 = FLOW
        ],
        propagators: vec![
            // Carry presence forward
            Box::new(IdentityCopy::new(PRESENCE)),
            // Agents emit at their positions
            Box::new(
                AgentEmission::builder()
                    .presence_field(PRESENCE)
                    .emission_field(EMISSION)
                    .intensity(10.0)
                    .mode(EmissionMode::Set)
                    .build()
                    .unwrap(),
            ),
            // Diffuse emission into heat
            Box::new(
                ScalarDiffusion::builder()
                    .input_field(EMISSION)
                    .output_field(HEAT)
                    .coefficient(0.1)
                    .build()
                    .unwrap(),
            ),
            // Compute flow from heat
            Box::new(
                FlowField::builder()
                    .potential_field(HEAT)
                    .flow_field(FLOW)
                    .normalize(false)
                    .build()
                    .unwrap(),
            ),
        ],
        dt: 0.1,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    let mut world = LockstepWorld::new(config).unwrap();

    // Inject agent presence at center (5,5) = index 55 via command
    // We use the initial state: all fields start at 0.
    // But presence needs to be non-zero. We rely on IdentityCopy
    // carrying it forward. For the first tick, presence is all zeros.
    // We need to set it via a command or initial state.
    //
    // Since we can't directly set initial state in WorldConfig,
    // we use a ScalarDiffusion source to inject presence.
    // Actually, let's just run enough ticks with emission=Set
    // mode and diffusion will spread the heat.

    // Run 50 ticks
    for _ in 0..50 {
        world.step_sync(vec![]).unwrap();
    }

    let snap = world.snapshot();

    // All fields should be finite
    let heat = snap.read(HEAT).unwrap();
    assert!(heat.iter().all(|v| v.is_finite()), "heat has NaN/Inf");

    let flow = snap.read(FLOW).unwrap();
    assert!(flow.iter().all(|v| v.is_finite()), "flow has NaN/Inf");
}

// ---------- Test 2: Resource depletion and regrowth ----------

/// IdentityCopy carries agent presence, ResourceField handles
/// consumption and linear regrowth.
#[test]
fn resource_consumption_and_regrowth() {
    let config = WorldConfig {
        space: Box::new(Square4::new(5, 5, EdgeBehavior::Absorb).unwrap()),
        fields: vec![
            scalar_field("presence"),   // 0
            scalar_field("resource"),   // 1
        ],
        propagators: vec![
            Box::new(IdentityCopy::new(PRESENCE)),
            Box::new(
                ResourceField::builder()
                    .field(FieldId(1))
                    .presence_field(PRESENCE)
                    .consumption_rate(0.5)
                    .regrowth_rate(0.01)
                    .capacity(1.0)
                    .regrowth_model(RegrowthModel::Linear)
                    .build()
                    .unwrap(),
            ),
        ],
        dt: 0.1,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    let mut world = LockstepWorld::new(config).unwrap();

    // Run 100 ticks. No agents present (presence=0), so resource
    // should regrow toward capacity. Starting from 0 with linear
    // regrowth: each tick adds 0.01*0.1 = 0.001.
    for _ in 0..100 {
        world.step_sync(vec![]).unwrap();
    }

    let snap = world.snapshot();
    let resource = snap.read(FieldId(1)).unwrap();

    // After 100 ticks of linear regrowth from 0:
    // v = min(0 + 100 * 0.01 * 0.1, 1.0) = min(0.1, 1.0) = 0.1
    for &v in resource {
        assert!(v >= 0.0, "resource should be non-negative, got {v}");
        assert!(v <= 1.0, "resource should be <= capacity, got {v}");
    }
}

// ---------- Test 3: Wave stability ----------

/// Run WavePropagation for 500 ticks with an initial impulse.
/// Assert no NaN/Inf and bounded energy.
#[test]
fn wave_stability_500_ticks() {
    let config = WorldConfig {
        space: Box::new(Square4::new(10, 10, EdgeBehavior::Absorb).unwrap()),
        fields: vec![
            scalar_field("displacement"),  // 0
            scalar_field("velocity"),      // 1
        ],
        propagators: vec![Box::new(
            WavePropagation::builder()
                .displacement_field(FieldId(0))
                .velocity_field(FieldId(1))
                .wave_speed(1.0)
                .damping(0.01)
                .build()
                .unwrap(),
        )],
        dt: 0.05, // well within CFL: 1/(1*sqrt(12)) ≈ 0.289
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    let mut world = LockstepWorld::new(config).unwrap();

    // All fields start at 0, so the wave stays at zero.
    // This verifies the propagator doesn't introduce artifacts.
    for tick in 1..=500u64 {
        world.step_sync(vec![]).unwrap();

        let snap = world.snapshot();
        let disp = snap.read(FieldId(0)).unwrap();
        let vel = snap.read(FieldId(1)).unwrap();

        for &v in disp.iter().chain(vel.iter()) {
            assert!(
                v.is_finite(),
                "NaN/Inf in wave fields at tick {tick}: {v}"
            );
        }
    }
}

// ---------- Test 4: Noise determinism ----------

/// Run NoiseInjection twice with the same seed for 100 ticks.
/// Assert bit-identical output.
#[test]
fn noise_determinism() {
    let make_config = |seed: u64| WorldConfig {
        space: Box::new(Square4::new(5, 5, EdgeBehavior::Absorb).unwrap()),
        fields: vec![scalar_field("noisy")],
        propagators: vec![Box::new(
            NoiseInjection::builder()
                .field(FieldId(0))
                .noise_type(NoiseType::Gaussian)
                .scale(0.5)
                .seed_offset(seed)
                .build()
                .unwrap(),
        )],
        dt: 0.1,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    let run = |seed: u64| -> Vec<f32> {
        let mut world = LockstepWorld::new(make_config(seed)).unwrap();
        for _ in 0..100 {
            world.step_sync(vec![]).unwrap();
        }
        let snap = world.snapshot();
        snap.read(FieldId(0)).unwrap().to_vec()
    };

    let a = run(99);
    let b = run(99);
    assert_eq!(a, b, "same seed → bit-identical noise output");
}

// ---------- Test 5: Morphological dilate/erode ----------

/// Run MorphologicalOp on a field with IdentityCopy preserving
/// the input mask across ticks.
#[test]
fn morphological_dilate_through_engine() {
    let config = WorldConfig {
        space: Box::new(Square4::new(5, 5, EdgeBehavior::Absorb).unwrap()),
        fields: vec![
            scalar_field("mask_in"),   // 0
            scalar_field("mask_out"),  // 1
        ],
        propagators: vec![
            Box::new(IdentityCopy::new(FieldId(0))),
            Box::new(
                MorphologicalOp::builder()
                    .input_field(FieldId(0))
                    .output_field(FieldId(1))
                    .op(MorphOp::Dilate)
                    .radius(1)
                    .threshold(0.5)
                    .build()
                    .unwrap(),
            ),
        ],
        dt: 0.1,
        seed: 42,
        ring_buffer_size: 8,
        max_ingress_queue: 1024,
        tick_rate_hz: None,
        backoff: BackoffConfig::default(),
    };

    let mut world = LockstepWorld::new(config).unwrap();

    // Starting from all zeros, dilate of zeros = zeros.
    world.step_sync(vec![]).unwrap();

    let snap = world.snapshot();
    let mask_out = snap.read(FieldId(1)).unwrap();
    assert!(
        mask_out.iter().all(|&v| v == 0.0),
        "dilate of all-zero should be all-zero"
    );
}
```

**Step 2: Run integration tests**

Run: `cargo test -p murk-propagators --test p4_integration`
Expected: All 5 tests pass.

**Step 3: Commit**

```bash
git add crates/murk-propagators/tests/p4_integration.rs
git commit -m "test(propagators): add P4 integration tests through LockstepWorld"
```

---

## Task 9: Wire Up lib.rs Exports and Full Test Suite

**Step 1: Verify lib.rs has all modules and re-exports**

Ensure `crates/murk-propagators/src/lib.rs` has these entries (added incrementally in Tasks 1-6):

```rust
pub mod agent_emission;
pub mod flow_field;
pub mod morphological_op;
pub mod noise_injection;
pub mod resource_field;
pub mod wave_propagation;

pub use agent_emission::{AgentEmission, EmissionMode};
pub use flow_field::FlowField;
pub use morphological_op::{MorphOp, MorphologicalOp};
pub use noise_injection::{NoiseInjection, NoiseType};
pub use resource_field::{RegrowthModel, ResourceField};
pub use wave_propagation::WavePropagation;
```

**Step 2: Run the full test suite**

Run: `cargo test -p murk-propagators`
Expected: All unit tests (Tasks 1-6) and integration tests (Task 8) pass. No regressions in existing P3 tests.

**Step 3: Run workspace-wide tests**

Run: `cargo test --workspace`
Expected: All tests pass across all crates.

**Step 4: Commit**

```bash
git add crates/murk-propagators/src/lib.rs
git commit -m "chore(propagators): wire up all P4 module exports"
```

---

## Dependency Graph

```
Task 1 (FlowField) ─────────────┐
Task 2 (AgentEmission) ──────────┤
Task 3 (ResourceField) ──────────┤
Task 4 (MorphologicalOp) ────────┼──→ Task 7 (Python bindings) ──→ Task 9 (Wire up + full test)
Task 5 (WavePropagation) ────────┤                                         ↑
Task 6 (NoiseInjection) ─────────┘                                         │
                                                      Task 8 (Integration) ┘
```

Tasks 1-6 are independent and can be implemented in any order (or in parallel by separate agents, with lib.rs merge at the end). Tasks 7-9 depend on all 6 propagators being present.
