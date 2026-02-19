# Propagator Standard Library Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Refactor `murk-propagators` from a hardcoded reference pipeline into a composable library of field-parameterized propagators (`ScalarDiffusion`, `GradientCompute`, `IdentityCopy`) exposed through Python bindings, then migrate all three examples.

**Architecture:** The existing `DiffusionPropagator` is split into two independent propagators: `ScalarDiffusion` (Jacobi diffusion with configurable source injection, decay, and clamping) and `GradientCompute` (finite-difference gradient extraction). A new trivial `IdentityCopy` propagator is added. All three use builder-pattern constructors parameterized on `FieldId` rather than hardcoded constants. The old `DiffusionPropagator` is preserved as a thin wrapper for backward compatibility during the migration period. Python bindings expose the three new propagators as PyO3 classes that construct Rust propagators via `murk-ffi`, bypassing the Python trampoline entirely for native-speed execution.

**Tech Stack:** Rust (murk-propagator trait), PyO3 0.28, numpy 0.28, murk-ffi C FFI layer

---

## Task 1: ScalarDiffusion — Builder and Struct

**Files:**
- Create: `crates/murk-propagators/src/scalar_diffusion.rs`
- Modify: `crates/murk-propagators/src/lib.rs` (add module + re-export)

**Step 1: Write the failing test**

Add to the bottom of `crates/murk-propagators/src/scalar_diffusion.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::TickId;
    use murk_propagator::scratch::ScratchRegion;
    use murk_space::{EdgeBehavior, Space, Square4};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    const F_HEAT: FieldId = FieldId(100);
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
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.1)
            .build()
            .unwrap();
        assert_eq!(prop.name(), "ScalarDiffusion");
        assert!(prop.reads_previous().contains(F_HEAT));
        let writes: Vec<_> = prop.writes().into_iter().map(|(id, _)| id).collect();
        assert!(writes.contains(&F_HEAT));
    }

    #[test]
    fn builder_rejects_negative_coefficient() {
        let result = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(-0.1)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_rejects_missing_input() {
        let result = ScalarDiffusion::builder()
            .output_field(F_HEAT)
            .coefficient(0.1)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn builder_rejects_missing_output() {
        let result = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .coefficient(0.1)
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn uniform_heat_stays_uniform() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.1)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, vec![10.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop.step(&mut ctx).unwrap();

        let heat = writer.get_field(F_HEAT).unwrap();
        for &v in heat {
            assert!((v - 10.0).abs() < 1e-6, "uniform heat should stay uniform, got {v}");
        }
    }

    #[test]
    fn hot_center_spreads() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.1)
            .build()
            .unwrap();

        let mut heat = vec![0.0f32; n];
        heat[12] = 100.0;

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, heat);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(F_HEAT).unwrap();
        assert!(result[12] < 100.0, "center should cool: {}", result[12]);
        assert!(result[7] > 0.0, "north should warm: {}", result[7]);
    }

    #[test]
    fn energy_conservation() {
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.1)
            .build()
            .unwrap();

        let mut heat = vec![0.0f32; n];
        heat[12] = 100.0;
        let total_before: f32 = heat.iter().sum();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, heat);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(F_HEAT).unwrap();
        let total_after: f32 = result.iter().sum();
        assert!(
            (total_before - total_after).abs() < 1e-3,
            "energy not conserved: before={total_before}, after={total_after}"
        );
    }

    #[test]
    fn max_dt_constraint() {
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.25)
            .build()
            .unwrap();
        let dt = prop.max_dt().unwrap();
        assert!((dt - 1.0 / 3.0).abs() < 1e-10);
    }

    #[test]
    fn decay_reduces_values() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.0)  // no diffusion, only decay
            .decay(0.5)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, vec![10.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 1.0);
        prop.step(&mut ctx).unwrap();

        let heat = writer.get_field(F_HEAT).unwrap();
        for &v in heat {
            assert!(v < 10.0, "decay should reduce values, got {v}");
            assert!(v > 0.0, "decay should not go negative, got {v}");
        }
    }

    #[test]
    fn source_injection() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.0)
            .sources(vec![(4, 50.0)])  // center cell pinned to 50
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, vec![0.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop.step(&mut ctx).unwrap();

        let heat = writer.get_field(F_HEAT).unwrap();
        assert!((heat[4] - 50.0).abs() < 1e-6, "source cell should be 50, got {}", heat[4]);
        assert!((heat[0]).abs() < 1e-6, "non-source cell should be 0, got {}", heat[0]);
    }

    #[test]
    fn clamp_min_applied() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.0)
            .clamp_min(0.0)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, vec![-5.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop.step(&mut ctx).unwrap();

        let heat = writer.get_field(F_HEAT).unwrap();
        for &v in heat {
            assert!(v >= 0.0, "clamp_min should enforce >= 0, got {v}");
        }
    }

    #[test]
    fn separate_input_output_fields() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let f_in = FieldId(200);
        let f_out = FieldId(201);
        let prop = ScalarDiffusion::builder()
            .input_field(f_in)
            .output_field(f_out)
            .coefficient(0.1)
            .build()
            .unwrap();

        assert!(prop.reads_previous().contains(f_in));
        let writes: Vec<_> = prop.writes().into_iter().map(|(id, _)| id).collect();
        assert!(writes.contains(&f_out));
        assert!(!writes.contains(&f_in));
    }

    #[test]
    fn gradient_field_included_in_writes() {
        let f_grad = FieldId(202);
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.1)
            .gradient_field(f_grad)
            .build()
            .unwrap();

        let writes: Vec<_> = prop.writes().into_iter().map(|(id, _)| id).collect();
        assert!(writes.contains(&f_grad));
    }

    #[test]
    fn wrap_energy_conservation() {
        let grid = Square4::new(5, 5, EdgeBehavior::Wrap).unwrap();
        let n = grid.cell_count();
        let prop = ScalarDiffusion::builder()
            .input_field(F_HEAT)
            .output_field(F_HEAT)
            .coefficient(0.1)
            .build()
            .unwrap();

        let mut heat = vec![0.0f32; n];
        heat[0] = 100.0;
        let total_before: f32 = heat.iter().sum();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_HEAT, heat);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_HEAT, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(F_HEAT).unwrap();
        let total_after: f32 = result.iter().sum();
        assert!(
            (total_before - total_after).abs() < 1e-3,
            "wrap energy not conserved: before={total_before}, after={total_after}"
        );
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p murk-propagators scalar_diffusion --no-run 2>&1 | head -5`
Expected: Compilation error — module `scalar_diffusion` doesn't exist yet.

**Step 3: Write minimal implementation**

Create `crates/murk-propagators/src/scalar_diffusion.rs`:

```rust
//! Configurable Jacobi-style scalar diffusion propagator.
//!
//! Parameterized replacement for the hardcoded `DiffusionPropagator`.
//! Operates on arbitrary `FieldId`s with optional decay, source injection,
//! value clamping, and gradient output.

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use murk_space::{EdgeBehavior, Square4};

/// Jacobi Laplacian diffusion on an arbitrary scalar field.
///
/// Each tick: `out[i] = (1 - α) * prev[i] + α * mean(prev[neighbours]) - decay * dt * prev[i]`
/// where `α = coefficient * dt * num_neighbours`.
///
/// After diffusion: sources are applied (pinning specific cells), then
/// values are clamped to `[clamp_min, clamp_max]`.
///
/// Optionally computes a central-difference gradient into a vector field.
pub struct ScalarDiffusion {
    input_field: FieldId,
    output_field: FieldId,
    gradient_field: Option<FieldId>,
    coefficient: f64,
    decay: f64,
    sources: Vec<(usize, f32)>,
    clamp_min: Option<f32>,
    clamp_max: Option<f32>,
}

/// Builder for [`ScalarDiffusion`].
pub struct ScalarDiffusionBuilder {
    input_field: Option<FieldId>,
    output_field: Option<FieldId>,
    gradient_field: Option<FieldId>,
    coefficient: Option<f64>,
    decay: f64,
    sources: Vec<(usize, f32)>,
    clamp_min: Option<f32>,
    clamp_max: Option<f32>,
}

impl ScalarDiffusionBuilder {
    /// Set the field to read from the previous tick.
    pub fn input_field(mut self, field: FieldId) -> Self {
        self.input_field = Some(field);
        self
    }

    /// Set the field to write diffused values into.
    pub fn output_field(mut self, field: FieldId) -> Self {
        self.output_field = Some(field);
        self
    }

    /// Optional: compute gradient into this vector field.
    pub fn gradient_field(mut self, field: FieldId) -> Self {
        self.gradient_field = Some(field);
        self
    }

    /// Diffusion coefficient (must be >= 0).
    pub fn coefficient(mut self, c: f64) -> Self {
        self.coefficient = Some(c);
        self
    }

    /// Exponential decay rate per tick (0 = none, must be >= 0).
    pub fn decay(mut self, d: f64) -> Self {
        self.decay = d;
        self
    }

    /// Fixed-value cells reset each tick: (cell_index, value).
    pub fn sources(mut self, s: Vec<(usize, f32)>) -> Self {
        self.sources = s;
        self
    }

    /// Minimum value clamp applied after diffusion.
    pub fn clamp_min(mut self, v: f32) -> Self {
        self.clamp_min = Some(v);
        self
    }

    /// Maximum value clamp applied after diffusion.
    pub fn clamp_max(mut self, v: f32) -> Self {
        self.clamp_max = Some(v);
        self
    }

    /// Build the propagator, validating all parameters.
    pub fn build(self) -> Result<ScalarDiffusion, String> {
        let input_field = self.input_field.ok_or("input_field is required")?;
        let output_field = self.output_field.ok_or("output_field is required")?;
        let coefficient = self.coefficient.unwrap_or(0.0);

        if coefficient < 0.0 {
            return Err(format!("coefficient must be >= 0, got {coefficient}"));
        }
        if self.decay < 0.0 {
            return Err(format!("decay must be >= 0, got {}", self.decay));
        }
        if let (Some(lo), Some(hi)) = (self.clamp_min, self.clamp_max) {
            if lo > hi {
                return Err(format!("clamp_min ({lo}) must be <= clamp_max ({hi})"));
            }
        }

        Ok(ScalarDiffusion {
            input_field,
            output_field,
            gradient_field: self.gradient_field,
            coefficient,
            decay: self.decay,
            sources: self.sources,
            clamp_min: self.clamp_min,
            clamp_max: self.clamp_max,
        })
    }
}

impl ScalarDiffusion {
    /// Create a builder for configuring a ScalarDiffusion propagator.
    pub fn builder() -> ScalarDiffusionBuilder {
        ScalarDiffusionBuilder {
            input_field: None,
            output_field: None,
            gradient_field: None,
            coefficient: None,
            decay: 0.0,
            sources: Vec::new(),
            clamp_min: None,
            clamp_max: None,
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

    fn neighbours_flat(
        r: i32, c: i32, rows: i32, cols: i32, edge: EdgeBehavior,
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
        let cell_count = (rows * cols) as usize;

        let prev = ctx.reads_previous().read(self.input_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("input field {:?} not readable", self.input_field),
            })?
            .to_vec();

        let out = ctx.writes().write(self.output_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("output field {:?} not writable", self.output_field),
            })?;

        // Diffusion
        for r in 0..rows_i {
            for c in 0..cols_i {
                let i = r as usize * cols as usize + c as usize;
                let nbs = Self::neighbours_flat(r, c, rows_i, cols_i, edge);
                let count = nbs.len() as u32;
                if count > 0 {
                    let sum: f32 = nbs.iter().map(|&ni| prev[ni]).sum();
                    let alpha = (self.coefficient * dt * count as f64) as f32;
                    let mean = sum / count as f32;
                    out[i] = (1.0 - alpha) * prev[i] + alpha * mean;
                } else {
                    out[i] = prev[i];
                }
            }
        }

        // Decay
        if self.decay > 0.0 {
            let decay_factor = (self.decay * dt) as f32;
            for i in 0..cell_count {
                out[i] -= decay_factor * out[i];
            }
        }

        // Source injection (after diffusion+decay so sources dominate)
        for &(cell, value) in &self.sources {
            if cell < cell_count {
                out[cell] = value;
            }
        }

        // Clamping
        if let Some(lo) = self.clamp_min {
            for v in out.iter_mut() {
                if *v < lo { *v = lo; }
            }
        }
        if let Some(hi) = self.clamp_max {
            for v in out.iter_mut() {
                if *v > hi { *v = hi; }
            }
        }

        // Optional gradient
        if let Some(grad_id) = self.gradient_field {
            let grad_out = ctx.writes().write(grad_id)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: format!("gradient field {:?} not writable", grad_id),
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

                    grad_out[i * 2] = (h_east - h_west) / 2.0;
                    grad_out[i * 2 + 1] = (h_south - h_north) / 2.0;
                }
            }
        }

        Ok(())
    }

    fn step_generic(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let dt = ctx.dt();
        let ordering = ctx.space().canonical_ordering();
        let cell_count = ordering.len();

        let neighbour_ranks: Vec<Vec<usize>> = ordering.iter()
            .map(|coord| {
                ctx.space().neighbours(coord).iter()
                    .filter_map(|nb| ctx.space().canonical_rank(nb))
                    .collect()
            })
            .collect();

        let prev = ctx.reads_previous().read(self.input_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("input field {:?} not readable", self.input_field),
            })?
            .to_vec();

        let mut new_vals = vec![0.0f32; cell_count];

        for i in 0..cell_count {
            let nbs = &neighbour_ranks[i];
            let count = nbs.len() as u32;
            if count > 0 {
                let sum: f32 = nbs.iter().map(|&r| prev[r]).sum();
                let alpha = (self.coefficient * dt * count as f64) as f32;
                let mean = sum / count as f32;
                new_vals[i] = (1.0 - alpha) * prev[i] + alpha * mean;
            } else {
                new_vals[i] = prev[i];
            }
        }

        // Decay
        if self.decay > 0.0 {
            let decay_factor = (self.decay * dt) as f32;
            for i in 0..cell_count {
                new_vals[i] -= decay_factor * new_vals[i];
            }
        }

        // Sources
        for &(cell, value) in &self.sources {
            if cell < cell_count {
                new_vals[cell] = value;
            }
        }

        // Clamping
        if let Some(lo) = self.clamp_min {
            for v in new_vals.iter_mut() {
                if *v < lo { *v = lo; }
            }
        }
        if let Some(hi) = self.clamp_max {
            for v in new_vals.iter_mut() {
                if *v > hi { *v = hi; }
            }
        }

        let out = ctx.writes().write(self.output_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("output field {:?} not writable", self.output_field),
            })?;
        out.copy_from_slice(&new_vals);

        // Optional gradient (generic path)
        if let Some(grad_id) = self.gradient_field {
            let grad_info: Vec<Vec<(usize, i32, i32)>> = ordering.iter()
                .map(|coord| {
                    ctx.space().neighbours(coord).iter()
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

            let mut grad_new = vec![0.0f32; cell_count * 2];
            for i in 0..cell_count {
                let gi = &grad_info[i];
                let mut gx = 0.0f32;
                let mut gy = 0.0f32;
                let mut xc = 0u32;
                let mut yc = 0u32;
                for &(rank, dc, dr) in gi {
                    let dh = prev[rank] - prev[i];
                    if dc != 0 { gx += dh / dc as f32; xc += 1; }
                    if dr != 0 { gy += dh / dr as f32; yc += 1; }
                }
                grad_new[i * 2] = if xc > 0 { gx / xc as f32 } else { 0.0 };
                grad_new[i * 2 + 1] = if yc > 0 { gy / yc as f32 } else { 0.0 };
            }

            let grad_out = ctx.writes().write(grad_id)
                .ok_or_else(|| PropagatorError::ExecutionFailed {
                    reason: format!("gradient field {:?} not writable", grad_id),
                })?;
            grad_out.copy_from_slice(&grad_new);
        }

        Ok(())
    }
}

impl Propagator for ScalarDiffusion {
    fn name(&self) -> &str {
        "ScalarDiffusion"
    }

    fn reads(&self) -> FieldSet {
        FieldSet::empty()
    }

    fn reads_previous(&self) -> FieldSet {
        [self.input_field].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        let mut w = vec![(self.output_field, WriteMode::Full)];
        if let Some(grad) = self.gradient_field {
            w.push((grad, WriteMode::Full));
        }
        w
    }

    fn max_dt(&self) -> Option<f64> {
        if self.coefficient > 0.0 {
            Some(1.0 / (12.0 * self.coefficient))
        } else {
            None
        }
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
```

Wire into `lib.rs` — add `pub mod scalar_diffusion;` and `pub use scalar_diffusion::ScalarDiffusion;`.

**Step 4: Run tests to verify they pass**

Run: `cargo test -p murk-propagators scalar_diffusion -- --nocapture`
Expected: All 14 tests pass.

**Step 5: Commit**

```bash
git add crates/murk-propagators/src/scalar_diffusion.rs crates/murk-propagators/src/lib.rs
git commit -m "feat(propagators): add ScalarDiffusion with builder pattern

Parameterized Jacobi diffusion on arbitrary FieldId with configurable
decay, source injection, clamping, and optional gradient output.
Replaces the hardcoded DiffusionPropagator for user-facing use."
```

---

## Task 2: GradientCompute — Standalone Propagator

**Files:**
- Create: `crates/murk-propagators/src/gradient_compute.rs`
- Modify: `crates/murk-propagators/src/lib.rs` (add module + re-export)

**Step 1: Write the failing test**

Add to `crates/murk-propagators/src/gradient_compute.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::TickId;
    use murk_propagator::scratch::ScratchRegion;
    use murk_space::{EdgeBehavior, Space, Square4};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    const F_SCALAR: FieldId = FieldId(100);
    const F_GRAD: FieldId = FieldId(101);

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
        let prop = GradientCompute::builder()
            .input_field(F_SCALAR)
            .output_field(F_GRAD)
            .build()
            .unwrap();
        assert_eq!(prop.name(), "GradientCompute");
        assert!(prop.reads_previous().contains(F_SCALAR));
    }

    #[test]
    fn builder_rejects_missing_fields() {
        assert!(GradientCompute::builder().output_field(F_GRAD).build().is_err());
        assert!(GradientCompute::builder().input_field(F_SCALAR).build().is_err());
    }

    #[test]
    fn linear_x_gradient() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = GradientCompute::builder()
            .input_field(F_SCALAR)
            .output_field(F_GRAD)
            .build()
            .unwrap();

        let mut heat = vec![0.0f32; n];
        for r in 0..3 {
            for c in 0..3 {
                heat[r * 3 + c] = (c as f32) * 10.0;
            }
        }

        let mut reader = MockFieldReader::new();
        reader.set_field(F_SCALAR, heat);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_GRAD, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop.step(&mut ctx).unwrap();

        let grad = writer.get_field(F_GRAD).unwrap();
        // Center (1,1): grad_x = (20-0)/2 = 10, grad_y = 0
        assert!((grad[4 * 2] - 10.0).abs() < 1e-6);
        assert!(grad[4 * 2 + 1].abs() < 1e-6);
    }

    #[test]
    fn uniform_field_zero_gradient() {
        let grid = Square4::new(5, 5, EdgeBehavior::Wrap).unwrap();
        let n = grid.cell_count();
        let prop = GradientCompute::builder()
            .input_field(F_SCALAR)
            .output_field(F_GRAD)
            .build()
            .unwrap();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_SCALAR, vec![7.0; n]);

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_GRAD, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop.step(&mut ctx).unwrap();

        let grad = writer.get_field(F_GRAD).unwrap();
        for &v in grad {
            assert!(v.abs() < 1e-6, "uniform field should have zero gradient, got {v}");
        }
    }
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test -p murk-propagators gradient_compute --no-run 2>&1 | head -5`
Expected: Compilation error.

**Step 3: Write minimal implementation**

Create `crates/murk-propagators/src/gradient_compute.rs`:

```rust
//! Standalone finite-difference gradient propagator.
//!
//! Reads a scalar field from the previous tick and writes a 2-component
//! vector field containing the central-difference gradient.

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};
use murk_space::{EdgeBehavior, Square4};

/// Finite-difference gradient of a scalar field into a vector field.
pub struct GradientCompute {
    input_field: FieldId,
    output_field: FieldId,
}

/// Builder for [`GradientCompute`].
pub struct GradientComputeBuilder {
    input_field: Option<FieldId>,
    output_field: Option<FieldId>,
}

impl GradientComputeBuilder {
    /// Scalar field to compute gradient of.
    pub fn input_field(mut self, field: FieldId) -> Self {
        self.input_field = Some(field);
        self
    }

    /// Vector field (2 components per cell) to write gradient into.
    pub fn output_field(mut self, field: FieldId) -> Self {
        self.output_field = Some(field);
        self
    }

    /// Build the propagator.
    pub fn build(self) -> Result<GradientCompute, String> {
        let input_field = self.input_field.ok_or("input_field is required")?;
        let output_field = self.output_field.ok_or("output_field is required")?;
        Ok(GradientCompute { input_field, output_field })
    }
}

impl GradientCompute {
    /// Create a builder.
    pub fn builder() -> GradientComputeBuilder {
        GradientComputeBuilder { input_field: None, output_field: None }
    }

    fn resolve_axis(val: i32, len: i32, edge: EdgeBehavior) -> Option<i32> {
        if val >= 0 && val < len { return Some(val); }
        match edge {
            EdgeBehavior::Absorb => None,
            EdgeBehavior::Clamp => Some(val.clamp(0, len - 1)),
            EdgeBehavior::Wrap => Some(((val % len) + len) % len),
        }
    }

    fn step_square4(
        &self, ctx: &mut StepContext<'_>, rows: u32, cols: u32, edge: EdgeBehavior,
    ) -> Result<(), PropagatorError> {
        let rows_i = rows as i32;
        let cols_i = cols as i32;

        let prev = ctx.reads_previous().read(self.input_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("input field {:?} not readable", self.input_field),
            })?
            .to_vec();

        let grad_out = ctx.writes().write(self.output_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("output field {:?} not writable", self.output_field),
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

                grad_out[i * 2] = (h_east - h_west) / 2.0;
                grad_out[i * 2 + 1] = (h_south - h_north) / 2.0;
            }
        }
        Ok(())
    }

    fn step_generic(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let ordering = ctx.space().canonical_ordering();
        let cell_count = ordering.len();

        let grad_info: Vec<Vec<(usize, i32, i32)>> = ordering.iter()
            .map(|coord| {
                ctx.space().neighbours(coord).iter()
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

        let prev = ctx.reads_previous().read(self.input_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("input field {:?} not readable", self.input_field),
            })?
            .to_vec();

        let mut grad_new = vec![0.0f32; cell_count * 2];
        for i in 0..cell_count {
            let gi = &grad_info[i];
            let mut gx = 0.0f32;
            let mut gy = 0.0f32;
            let mut xc = 0u32;
            let mut yc = 0u32;
            for &(rank, dc, dr) in gi {
                let dh = prev[rank] - prev[i];
                if dc != 0 { gx += dh / dc as f32; xc += 1; }
                if dr != 0 { gy += dh / dr as f32; yc += 1; }
            }
            grad_new[i * 2] = if xc > 0 { gx / xc as f32 } else { 0.0 };
            grad_new[i * 2 + 1] = if yc > 0 { gy / yc as f32 } else { 0.0 };
        }

        let grad_out = ctx.writes().write(self.output_field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("output field {:?} not writable", self.output_field),
            })?;
        grad_out.copy_from_slice(&grad_new);
        Ok(())
    }
}

impl Propagator for GradientCompute {
    fn name(&self) -> &str { "GradientCompute" }

    fn reads(&self) -> FieldSet { FieldSet::empty() }

    fn reads_previous(&self) -> FieldSet {
        [self.input_field].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.output_field, WriteMode::Full)]
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
```

Wire into `lib.rs`: add `pub mod gradient_compute;` and `pub use gradient_compute::GradientCompute;`.

**Step 4: Run tests**

Run: `cargo test -p murk-propagators gradient_compute -- --nocapture`
Expected: All 4 tests pass.

**Step 5: Commit**

```bash
git add crates/murk-propagators/src/gradient_compute.rs crates/murk-propagators/src/lib.rs
git commit -m "feat(propagators): add GradientCompute standalone propagator

Extracted from DiffusionPropagator. Reads arbitrary scalar field,
writes 2-component central-difference gradient vector field."
```

---

## Task 3: IdentityCopy Propagator

**Files:**
- Create: `crates/murk-propagators/src/identity_copy.rs`
- Modify: `crates/murk-propagators/src/lib.rs`

**Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use murk_core::TickId;
    use murk_propagator::scratch::ScratchRegion;
    use murk_space::{EdgeBehavior, Space, Square4};
    use murk_test_utils::{MockFieldReader, MockFieldWriter};

    const F_FIELD: FieldId = FieldId(100);

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
    fn copies_previous_to_current() {
        let grid = Square4::new(3, 3, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let prop = IdentityCopy::new(F_FIELD);

        let data: Vec<f32> = (0..n).map(|i| i as f32 * 1.5).collect();

        let mut reader = MockFieldReader::new();
        reader.set_field(F_FIELD, data.clone());

        let mut writer = MockFieldWriter::new();
        writer.add_field(F_FIELD, n);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(F_FIELD).unwrap();
        assert_eq!(result, data.as_slice());
    }

    #[test]
    fn declares_correct_fields() {
        let prop = IdentityCopy::new(F_FIELD);
        assert!(prop.reads_previous().contains(F_FIELD));
        assert_eq!(prop.writes(), vec![(F_FIELD, WriteMode::Full)]);
        assert!(prop.reads().is_empty());
    }

    #[test]
    fn works_with_multi_component_field() {
        let grid = Square4::new(2, 2, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();
        let f_vec = FieldId(200);
        let prop = IdentityCopy::new(f_vec);

        let data: Vec<f32> = (0..(n * 2)).map(|i| i as f32).collect();

        let mut reader = MockFieldReader::new();
        reader.set_field(f_vec, data.clone());

        let mut writer = MockFieldWriter::new();
        writer.add_field(f_vec, n * 2);

        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&reader, &mut writer, &mut scratch, &grid, 0.01);
        prop.step(&mut ctx).unwrap();

        let result = writer.get_field(f_vec).unwrap();
        assert_eq!(result, data.as_slice());
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p murk-propagators identity_copy --no-run 2>&1 | head -5`

**Step 3: Write implementation**

```rust
//! Copy a field's previous-tick values into the current tick unchanged.

use murk_core::{FieldId, FieldSet, PropagatorError};
use murk_propagator::context::StepContext;
use murk_propagator::propagator::{Propagator, WriteMode};

/// Copies a field unchanged from the previous tick to the current tick.
///
/// Useful for persistent state fields (agent positions, markers) where
/// the values carry forward unless explicitly overwritten by commands.
pub struct IdentityCopy {
    field: FieldId,
}

impl IdentityCopy {
    /// Create a new identity-copy propagator for the given field.
    pub fn new(field: FieldId) -> Self {
        Self { field }
    }
}

impl Propagator for IdentityCopy {
    fn name(&self) -> &str { "IdentityCopy" }

    fn reads(&self) -> FieldSet { FieldSet::empty() }

    fn reads_previous(&self) -> FieldSet {
        [self.field].into_iter().collect()
    }

    fn writes(&self) -> Vec<(FieldId, WriteMode)> {
        vec![(self.field, WriteMode::Full)]
    }

    fn step(&self, ctx: &mut StepContext<'_>) -> Result<(), PropagatorError> {
        let prev = ctx.reads_previous().read(self.field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("field {:?} not readable", self.field),
            })?
            .to_vec();

        let out = ctx.writes().write(self.field)
            .ok_or_else(|| PropagatorError::ExecutionFailed {
                reason: format!("field {:?} not writable", self.field),
            })?;
        out.copy_from_slice(&prev);
        Ok(())
    }
}
```

Wire into `lib.rs`: add `pub mod identity_copy;` and `pub use identity_copy::IdentityCopy;`.

**Step 4: Run tests**

Run: `cargo test -p murk-propagators identity_copy -- --nocapture`
Expected: All 3 tests pass.

**Step 5: Commit**

```bash
git add crates/murk-propagators/src/identity_copy.rs crates/murk-propagators/src/lib.rs
git commit -m "feat(propagators): add IdentityCopy propagator

Trivial carry-forward propagator for persistent state fields."
```

---

## Task 4: Backward-Compat Wrapper for DiffusionPropagator

**Files:**
- Modify: `crates/murk-propagators/src/diffusion.rs`

**Step 1: Write the failing test**

Add to existing `diffusion.rs` test module:

```rust
    #[test]
    fn diffusion_matches_scalar_diffusion() {
        // Verify the old DiffusionPropagator produces identical output
        // to ScalarDiffusion configured with HEAT + VELOCITY + HEAT_GRADIENT.
        let grid = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let n = grid.cell_count();

        let mut heat = vec![0.0f32; n];
        heat[12] = 100.0;

        // Run old propagator
        let old_prop = DiffusionPropagator::new(0.1);
        let mut old_reader = MockFieldReader::new();
        old_reader.set_field(HEAT, heat.clone());
        old_reader.set_field(VELOCITY, vec![0.0; n * 2]);
        let mut old_writer = MockFieldWriter::new();
        old_writer.add_field(HEAT, n);
        old_writer.add_field(VELOCITY, n * 2);
        old_writer.add_field(HEAT_GRADIENT, n * 2);
        let mut scratch = ScratchRegion::new(0);
        let mut ctx = make_ctx(&old_reader, &mut old_writer, &mut scratch, &grid, 0.01);
        old_prop.step(&mut ctx).unwrap();

        // Run new propagator
        let new_prop = crate::ScalarDiffusion::builder()
            .input_field(HEAT)
            .output_field(HEAT)
            .coefficient(0.1)
            .gradient_field(HEAT_GRADIENT)
            .build()
            .unwrap();
        let mut new_reader = MockFieldReader::new();
        new_reader.set_field(HEAT, heat);
        let mut new_writer = MockFieldWriter::new();
        new_writer.add_field(HEAT, n);
        new_writer.add_field(HEAT_GRADIENT, n * 2);
        let mut scratch2 = ScratchRegion::new(0);
        let mut ctx2 = StepContext::new(
            &new_reader, &new_reader, &mut new_writer, &mut scratch2, &grid, TickId(1), 0.01,
        );
        new_prop.step(&mut ctx2).unwrap();

        // Compare heat output
        let old_heat = old_writer.get_field(HEAT).unwrap();
        let new_heat = new_writer.get_field(HEAT).unwrap();
        for i in 0..n {
            assert!(
                (old_heat[i] - new_heat[i]).abs() < 1e-6,
                "heat mismatch at cell {i}: old={}, new={}",
                old_heat[i], new_heat[i]
            );
        }

        // Compare gradient output
        let old_grad = old_writer.get_field(HEAT_GRADIENT).unwrap();
        let new_grad = new_writer.get_field(HEAT_GRADIENT).unwrap();
        for i in 0..(n * 2) {
            assert!(
                (old_grad[i] - new_grad[i]).abs() < 1e-6,
                "gradient mismatch at index {i}: old={}, new={}",
                old_grad[i], new_grad[i]
            );
        }
    }
```

**Step 2: Run test to verify it passes** (this is a parity test — it should pass if Task 1 is correct)

Run: `cargo test -p murk-propagators diffusion_matches_scalar_diffusion -- --nocapture`
Expected: PASS.

**Step 3: Commit**

```bash
git add crates/murk-propagators/src/diffusion.rs
git commit -m "test(propagators): add parity test between old and new diffusion"
```

---

## Task 5: Python Bindings for Library Propagators

**Files:**
- Create: `crates/murk-python/src/library_propagators.rs`
- Modify: `crates/murk-python/src/lib.rs` (register new classes)
- Modify: `crates/murk-python/Cargo.toml` (add murk-propagators dependency)

**Step 1: Add dependency**

In `crates/murk-python/Cargo.toml`, add to `[dependencies]`:
```toml
murk-propagators = { path = "../murk-propagators", version = "0.1.6" }
murk-propagator = { path = "../murk-propagator", version = "0.1.6" }
murk-core = { path = "../murk-core", version = "0.1.6" }
```

**Step 2: Create library_propagators.rs**

These PyO3 classes construct native Rust propagators and register them via the FFI layer. The key difference from `PropagatorDef` is that **no Python trampoline is needed** — the Rust propagator runs at native speed.

The binding strategy depends on how the FFI layer currently registers Rust propagators. Looking at the current code, `murk_ffi::murk_propagator_create` takes a `MurkPropagatorDef` with a C function pointer. For library propagators, we need to either:

(a) Create a C trampoline that dispatches to the Rust propagator, or
(b) Add a new FFI function that accepts a boxed `dyn Propagator` directly.

Since `murk-ffi` wraps `murk-engine` which already accepts `Box<dyn Propagator>` in `WorldConfig`, option (b) is cleaner. However, the Python `Config` class builds its config through FFI handles, so we need to check how `Config.add_propagator_handle` works.

**The simplest approach**: Store the library propagator as a boxed trait object in the Config's propagator list alongside the trampoline-based ones. This requires a new FFI function:

```rust
// In murk-ffi (if needed):
pub fn murk_propagator_create_native(prop: Box<dyn Propagator>) -> u64;
```

**Alternatively**, if the FFI layer is too rigid, we can wrap the Rust propagator in a C trampoline that calls `prop.step()` directly (still native speed, just one extra function pointer indirection).

The implementation of this task depends on the FFI layer's structure. The executor should:

1. Read `crates/murk-ffi/src/propagator.rs` (or wherever `murk_propagator_create` lives)
2. Read how `Config.add_propagator_handle` works in `crates/murk-python/src/config.rs`
3. Choose the approach that requires the least FFI change

**Step 3: Register in lib.rs module**

Add to `_murk` function in `crates/murk-python/src/lib.rs`:
```rust
mod library_propagators;

// In _murk():
m.add_class::<library_propagators::PyScalarDiffusion>()?;
m.add_class::<library_propagators::PyGradientCompute>()?;
m.add_class::<library_propagators::PyIdentityCopy>()?;
```

**Step 4: Build and test**

Run: `cargo build -p murk-python`
Then: `cd crates/murk-python && maturin develop && python -c "import murk; print(dir(murk))"`
Expected: `ScalarDiffusion`, `GradientCompute`, `IdentityCopy` appear in the module.

**Step 5: Commit**

```bash
git add crates/murk-python/src/library_propagators.rs crates/murk-python/src/lib.rs crates/murk-python/Cargo.toml
git commit -m "feat(python): expose ScalarDiffusion, GradientCompute, IdentityCopy

Native-speed library propagators accessible from Python. No Python
trampoline needed — runs the Rust implementation directly."
```

---

## Task 6: Migrate heat_seeker Example

**Files:**
- Modify: `examples/heat_seeker/heat_seeker.py`

**Step 1: Identify the changes**

Replace the `diffusion_step` Python function and its `add_propagator` call with:
```python
config.add_propagator(murk.ScalarDiffusion(
    field="heat",       # resolved to HEAT_FIELD by binding
    coefficient=0.1,
    decay=0.005,
    sources=[(SOURCE_Y * GRID_W + SOURCE_X, 10.0)],
    clamp_min=0.0,
))
```

Remove: `diffusion_step` function (~25 lines), numpy import if no longer needed, `DIFFUSION_COEFF`, `HEAT_DECAY` constants.

**Step 2: Run the example to verify**

Run: `cd examples/heat_seeker && python heat_seeker.py --episodes 1 --render none`
Expected: Runs to completion. Agent should still seek the heat source.

**Step 3: Commit**

```bash
git add examples/heat_seeker/heat_seeker.py
git commit -m "refactor(examples): migrate heat_seeker to library propagators

Removes ~25 lines of Python diffusion code. Tick performance
improved by using native Rust ScalarDiffusion."
```

---

## Task 7: Migrate crystal_nav Example

**Files:**
- Modify: `examples/crystal_nav/crystal_nav.py`

**Step 1: Identify the changes**

Replace `dual_diffusion_step` with two `ScalarDiffusion` instances:
```python
config.add_propagator(murk.ScalarDiffusion(
    field="beacon",
    coefficient=0.06,
    decay=0.01,
    sources=[(beacon_rank, SOURCE_INTENSITY)],
    clamp_min=0.0,
))
config.add_propagator(murk.ScalarDiffusion(
    field="radiation",
    coefficient=0.04,
    decay=0.03,
    sources=[(radiation_rank, SOURCE_INTENSITY)],
    clamp_min=0.0,
))
```

Remove: `dual_diffusion_step` function (~60 lines), `NBR_IDX` precomputation, `BEACON_D`, `RADIATION_D`, `BEACON_DECAY`, `RADIATION_DECAY` constants, `DEGREE` constant.

**Step 2: Run the example**

Run: `cd examples/crystal_nav && python crystal_nav.py --episodes 1 --render none`

**Step 3: Commit**

```bash
git add examples/crystal_nav/crystal_nav.py
git commit -m "refactor(examples): migrate crystal_nav to library propagators

Removes ~60 lines of Python dual-field diffusion code. Two
ScalarDiffusion instances replace the monolithic step function."
```

---

## Task 8: Migrate hex_pursuit Example

**Files:**
- Modify: `examples/hex_pursuit/hex_pursuit.py`

**Step 1: Identify the changes**

Replace `identity_step` and `PropagatorDef` with:
```python
config.add_propagator(murk.IdentityCopy(field="predator"))
config.add_propagator(murk.IdentityCopy(field="prey"))
```

Remove: `identity_step` function (4 lines), `PropagatorDef` import.

**Step 2: Run the example**

Run: `cd examples/hex_pursuit && python hex_pursuit.py --episodes 1 --render none`

**Step 3: Commit**

```bash
git add examples/hex_pursuit/hex_pursuit.py
git commit -m "refactor(examples): migrate hex_pursuit to library propagators

Removes Python identity step function. Two IdentityCopy instances
handle carry-forward natively in Rust."
```

---

## Task 9: Update Bench reference_profile

**Files:**
- Modify: `crates/murk-bench/src/lib.rs`

**Step 1: Write parity test**

Add to existing `#[cfg(test)]` in `crates/murk-bench/src/lib.rs`:

```rust
    #[test]
    fn reference_profile_with_library_propagators_validates() {
        use murk_propagators::{ScalarDiffusion, GradientCompute};
        let ab = new_action_buffer();
        let cell_count = 100 * 100;
        let initial_positions = init_agent_positions(cell_count, 4, 42);

        let heat_prop = ScalarDiffusion::builder()
            .input_field(murk_propagators::HEAT)
            .output_field(murk_propagators::HEAT)
            .coefficient(0.1)
            .gradient_field(murk_propagators::HEAT_GRADIENT)
            .build()
            .unwrap();

        // Velocity needs its own diffusion instance or a separate propagator.
        // For the reference profile, velocity is also diffused with the same coefficient.
        let vel_prop = ScalarDiffusion::builder()
            .input_field(murk_propagators::VELOCITY)
            .output_field(murk_propagators::VELOCITY)
            .coefficient(0.1)
            .build()
            .unwrap();

        let config = WorldConfig {
            space: Box::new(Square4::new(100, 100, EdgeBehavior::Absorb).unwrap()),
            fields: murk_propagators::reference_fields(),
            propagators: vec![
                Box::new(heat_prop),
                Box::new(vel_prop),
                Box::new(AgentMovementPropagator::new(ab, initial_positions)),
                Box::new(RewardPropagator::new(1.0, -0.01)),
            ],
            dt: 0.1,
            seed: 42,
            ring_buffer_size: 8,
            max_ingress_queue: 1024,
            tick_rate_hz: None,
            backoff: BackoffConfig::default(),
        };
        config.validate().unwrap();
    }
```

**NOTE:** The old `DiffusionPropagator` diffuses both HEAT and VELOCITY in one propagator. The library approach uses two separate `ScalarDiffusion` instances. The velocity field is a 2-component vector, but `ScalarDiffusion` treats the buffer as flat f32 — it will diffuse all components with the same kernel, which is mathematically identical to what `DiffusionPropagator` does (per-component diffusion). This needs verification during execution.

**Step 2: Run parity test**

Run: `cargo test -p murk-bench reference_profile_with_library -- --nocapture`

**Step 3: Switch reference_profile to use library propagators**

Update `reference_profile()` and `stress_profile()` in `crates/murk-bench/src/lib.rs`.

**Step 4: Run all bench tests**

Run: `cargo test -p murk-bench`

**Step 5: Commit**

```bash
git add crates/murk-bench/src/lib.rs
git commit -m "refactor(bench): migrate reference_profile to library propagators

Uses ScalarDiffusion + GradientCompute instead of DiffusionPropagator."
```

---

## Task 10: Deprecate Hardcoded Field Constants

**Files:**
- Modify: `crates/murk-propagators/src/fields.rs`
- Modify: `crates/murk-propagators/src/lib.rs`

**Step 1: Add deprecation attributes**

In `fields.rs`, add `#[deprecated]` to the field constants:

```rust
#[deprecated(since = "0.2.0", note = "use user-defined FieldId via world config")]
pub const HEAT: FieldId = FieldId(0);
// ... same for VELOCITY, AGENT_PRESENCE, HEAT_GRADIENT, REWARD
```

Keep `reference_fields()` undeprecated — it's still used by the bench profiles.

**Step 2: Fix deprecation warnings in internal code**

The old `DiffusionPropagator`, `AgentMovementPropagator`, and `RewardPropagator` still use these constants. Add `#[allow(deprecated)]` to those modules since they are backward-compat wrappers.

**Step 3: Verify no warnings in library code**

Run: `cargo test -p murk-propagators 2>&1 | grep -i "deprecat"`
Expected: Only the expected `#[allow(deprecated)]` suppressed warnings.

**Step 4: Commit**

```bash
git add crates/murk-propagators/src/fields.rs crates/murk-propagators/src/lib.rs \
       crates/murk-propagators/src/diffusion.rs crates/murk-propagators/src/agent_movement.rs \
       crates/murk-propagators/src/reward.rs
git commit -m "chore(propagators): deprecate hardcoded field constants

Users should define their own FieldId values via world config.
The reference pipeline constants remain available but deprecated."
```

---

## Task 11: Full Integration Test

**Files:**
- Modify: `crates/murk-propagators/tests/integration.rs`

**Step 1: Add integration test using library propagators**

```rust
#[test]
fn library_propagators_thousand_ticks() {
    // Mirror thousand_tick_reference_run but using library propagators
    use murk_propagators::{ScalarDiffusion, GradientCompute, IdentityCopy};
    // ... build WorldConfig with library propagators, run 1000 ticks,
    // verify heat is finite, fields are present
}

#[test]
fn library_propagators_determinism() {
    // Two runs with same seed produce identical output
}
```

**Step 2: Run integration tests**

Run: `cargo test -p murk-propagators --test integration -- --nocapture`

**Step 3: Commit**

```bash
git add crates/murk-propagators/tests/integration.rs
git commit -m "test(propagators): add integration tests for library propagators

Verifies 1000-tick stability and determinism using ScalarDiffusion,
GradientCompute, and IdentityCopy."
```

---

## Task 12: Run Full Test Suite

**Step 1: Run all Rust tests**

Run: `cargo test --workspace`
Expected: All tests pass, including the old DiffusionPropagator tests (backward compat).

**Step 2: Run Python examples** (if Python env is set up)

Run each example with `--episodes 1 --render none`.

**Step 3: Final commit if any fixups needed**

---

## Dependency Graph

```
Task 1 (ScalarDiffusion) ─┬─→ Task 4 (Parity test)
Task 2 (GradientCompute) ─┤
Task 3 (IdentityCopy) ────┤
                           ├─→ Task 5 (Python bindings) ─→ Task 6,7,8 (migrate examples)
                           └─→ Task 9 (bench migration)
                                                          ↓
                                               Task 10 (deprecate constants)
                                                          ↓
                                               Task 11 (integration tests)
                                                          ↓
                                               Task 12 (full suite)
```

Tasks 1, 2, 3 are independent and can be executed in parallel.
Tasks 6, 7, 8 are independent and can be executed in parallel (after Task 5).
