//! Observation plan compilation and execution.
//!
//! [`ObsPlan`] is compiled from an [`ObsSpec`] + [`Space`], producing
//! a reusable gather plan that can be executed against any
//! [`SnapshotAccess`] implementor. The "Simple plan class" uses a
//! branch-free flat gather: for each entry, iterate pre-computed
//! `(field_data_index, tensor_index)` pairs, read the field value,
//! optionally transform it, and write to the caller-allocated buffer.

use indexmap::IndexMap;

use murk_core::error::ObsError;
use murk_core::{Coord, FieldId, SnapshotAccess, TickId, WorldGenerationId};
use murk_space::Space;

use crate::geometry::GridGeometry;
use crate::metadata::ObsMetadata;
use crate::pool::pool_2d;
use crate::spec::{ObsDtype, ObsRegion, ObsSpec, ObsTransform, PoolConfig};

/// Coverage threshold: warn if valid_ratio < this.
const COVERAGE_WARN_THRESHOLD: f64 = 0.5;

/// Coverage threshold: error if valid_ratio < this.
const COVERAGE_ERROR_THRESHOLD: f64 = 0.35;

/// Result of compiling an [`ObsSpec`].
#[derive(Debug)]
pub struct ObsPlanResult {
    /// The compiled plan, ready for execution.
    pub plan: ObsPlan,
    /// Total number of f32 elements in the output tensor.
    pub output_len: usize,
    /// Shape per entry (each entry's region bounding shape dimensions).
    pub entry_shapes: Vec<Vec<usize>>,
    /// Length of the validity mask in bytes.
    pub mask_len: usize,
}

/// Compiled observation plan: either Simple or Standard class.
///
/// **Simple** (all `Fixed` regions): pre-computed gather indices, branch-free
/// loop, zero spatial computation at runtime. Use [`execute`](Self::execute).
///
/// **Standard** (any agent-relative region): template-based gather with
/// interior/boundary dispatch. Use [`execute_agents`](Self::execute_agents).
#[derive(Debug)]
pub struct ObsPlan {
    strategy: PlanStrategy,
    /// Total output elements across all entries (per agent for Standard).
    output_len: usize,
    /// Total mask bytes across all entries (per agent for Standard).
    mask_len: usize,
    /// Generation at compile time (for PLAN_INVALIDATED detection).
    compiled_generation: Option<WorldGenerationId>,
}

/// Pre-computed gather instruction for a single cell.
///
/// At execution time, we read `field_data[field_data_idx]` and write
/// the (transformed) value to `output[tensor_idx]`.
#[derive(Debug, Clone)]
struct GatherOp {
    /// Index into the flat field data array (canonical ordering).
    field_data_idx: usize,
    /// Index into the output slice for this entry.
    tensor_idx: usize,
}

/// A single compiled entry ready for gather execution.
#[derive(Debug)]
struct CompiledEntry {
    field_id: FieldId,
    transform: ObsTransform,
    #[allow(dead_code)]
    dtype: ObsDtype,
    /// Offset into the output buffer where this entry starts.
    output_offset: usize,
    /// Offset into the validity mask where this entry starts.
    mask_offset: usize,
    /// Number of elements this entry contributes to the output.
    element_count: usize,
    /// Pre-computed gather operations (one per valid cell in the region).
    gather_ops: Vec<GatherOp>,
    /// Pre-computed validity mask for this entry's bounding box.
    valid_mask: Vec<u8>,
    /// Valid ratio for this entry's region.
    #[allow(dead_code)]
    valid_ratio: f64,
}

/// Relative offset from agent center for template-based gather.
///
/// At compile time, the bounding box of an agent-centered region is
/// decomposed into `TemplateOp`s. At execute time, the agent center
/// is resolved and each op is applied: `field_data[base_rank + stride_offset]`.
#[derive(Debug, Clone)]
struct TemplateOp {
    /// Offset from center per coordinate axis.
    relative: Coord,
    /// Position in the gather bounding-box tensor (row-major).
    tensor_idx: usize,
    /// Precomputed `sum(relative[i] * strides[i])` for interior fast path.
    /// Zero if no `GridGeometry` is available (fallback path only).
    stride_offset: isize,
    /// Whether this cell is within the disk region (always true for AgentRect).
    /// For AgentDisk, cells outside the graph-distance radius are excluded.
    in_disk: bool,
}

/// Compiled agent-relative entry for the Standard plan class.
///
/// Stores template data that is instantiated per-agent at execute time.
/// The bounding box shape comes from the region (e.g., `[2r+1, 2r+1]` for
/// `AgentDisk`/`AgentRect`), and may be reduced by pooling.
#[derive(Debug)]
struct AgentCompiledEntry {
    field_id: FieldId,
    pool: Option<PoolConfig>,
    transform: ObsTransform,
    #[allow(dead_code)]
    dtype: ObsDtype,
    /// Offset into the per-agent output buffer.
    output_offset: usize,
    /// Offset into the per-agent mask buffer.
    mask_offset: usize,
    /// Post-pool output elements (written to output).
    element_count: usize,
    /// Pre-pool bounding-box elements (gather buffer size).
    pre_pool_element_count: usize,
    /// Shape of the pre-pool bounding box (e.g., `[7, 7]`).
    pre_pool_shape: Vec<usize>,
    /// Template operations (one per cell in bounding box).
    template_ops: Vec<TemplateOp>,
    /// Radius for `is_interior` check.
    radius: u32,
}

/// Data for the Standard plan class (agent-centered foveation + pooling).
#[derive(Debug)]
struct StandardPlanData {
    /// Entries with `ObsRegion::Fixed` (same output for all agents).
    fixed_entries: Vec<CompiledEntry>,
    /// Entries with agent-relative regions (resolved per-agent).
    agent_entries: Vec<AgentCompiledEntry>,
    /// Grid geometry for interior/boundary dispatch (`None` → all slow path).
    geometry: Option<GridGeometry>,
}

/// Internal plan strategy: Simple (all-fixed) or Standard (agent-centered).
#[derive(Debug)]
enum PlanStrategy {
    /// All entries are `ObsRegion::Fixed`: pre-computed gather indices.
    Simple(Vec<CompiledEntry>),
    /// At least one entry is agent-relative: template-based gather.
    Standard(StandardPlanData),
}

impl ObsPlan {
    /// Compile an [`ObsSpec`] against a [`Space`].
    ///
    /// Detects whether the spec contains agent-relative regions and
    /// dispatches to the appropriate plan class:
    /// - All `Fixed` → **Simple** (pre-computed gather)
    /// - Any `AgentDisk`/`AgentRect` → **Standard** (template-based)
    pub fn compile(spec: &ObsSpec, space: &dyn Space) -> Result<ObsPlanResult, ObsError> {
        if spec.entries.is_empty() {
            return Err(ObsError::InvalidObsSpec {
                reason: "ObsSpec has no entries".into(),
            });
        }

        // Validate transform parameters.
        for (i, entry) in spec.entries.iter().enumerate() {
            if let ObsTransform::Normalize { min, max } = &entry.transform {
                if !min.is_finite() || !max.is_finite() {
                    return Err(ObsError::InvalidObsSpec {
                        reason: format!(
                            "entry {i}: Normalize min/max must be finite, got min={min}, max={max}"
                        ),
                    });
                }
                if min > max {
                    return Err(ObsError::InvalidObsSpec {
                        reason: format!(
                            "entry {i}: Normalize min ({min}) must be <= max ({max})"
                        ),
                    });
                }
            }
        }

        let has_agent = spec.entries.iter().any(|e| {
            matches!(
                e.region,
                ObsRegion::AgentDisk { .. } | ObsRegion::AgentRect { .. }
            )
        });

        if has_agent {
            Self::compile_standard(spec, space)
        } else {
            Self::compile_simple(spec, space)
        }
    }

    /// Compile a Simple plan (all `Fixed` regions, no agent-relative entries).
    fn compile_simple(spec: &ObsSpec, space: &dyn Space) -> Result<ObsPlanResult, ObsError> {
        let canonical = space.canonical_ordering();
        let coord_to_field_idx: IndexMap<Coord, usize> = canonical
            .into_iter()
            .enumerate()
            .map(|(idx, coord)| (coord, idx))
            .collect();

        let mut entries = Vec::with_capacity(spec.entries.len());
        let mut output_offset = 0usize;
        let mut mask_offset = 0usize;
        let mut entry_shapes = Vec::with_capacity(spec.entries.len());

        for (i, entry) in spec.entries.iter().enumerate() {
            let fixed_region = match &entry.region {
                ObsRegion::Fixed(spec) => spec,
                ObsRegion::AgentDisk { .. } | ObsRegion::AgentRect { .. } => {
                    return Err(ObsError::InvalidObsSpec {
                        reason: format!("entry {i}: agent-relative region in Simple plan"),
                    });
                }
            };
            if entry.pool.is_some() {
                return Err(ObsError::InvalidObsSpec {
                    reason: format!(
                        "entry {i}: pooling requires a Standard plan (use agent-relative region)"
                    ),
                });
            }

            let mut region_plan =
                space
                    .compile_region(fixed_region)
                    .map_err(|e| ObsError::InvalidObsSpec {
                        reason: format!("entry {i}: region compile failed: {e}"),
                    })?;

            let ratio = region_plan.valid_ratio();
            if ratio < COVERAGE_ERROR_THRESHOLD {
                return Err(ObsError::InvalidComposition {
                    reason: format!(
                        "entry {i}: valid_ratio {ratio:.3} < {COVERAGE_ERROR_THRESHOLD}"
                    ),
                });
            }
            if ratio < COVERAGE_WARN_THRESHOLD {
                eprintln!(
                    "murk-obs: warning: entry {i} valid_ratio {ratio:.3} < {COVERAGE_WARN_THRESHOLD}"
                );
            }

            let mut gather_ops = Vec::with_capacity(region_plan.coords().len());
            for (coord_idx, coord) in region_plan.coords().iter().enumerate() {
                let field_data_idx =
                    *coord_to_field_idx
                        .get(coord)
                        .ok_or_else(|| ObsError::InvalidObsSpec {
                            reason: format!("entry {i}: coord {coord:?} not in canonical ordering"),
                        })?;
                let tensor_idx = region_plan.tensor_indices()[coord_idx];
                gather_ops.push(GatherOp {
                    field_data_idx,
                    tensor_idx,
                });
            }

            let element_count = region_plan.bounding_shape().total_elements();
            let shape = match region_plan.bounding_shape() {
                murk_space::BoundingShape::Rect(dims) => dims.clone(),
            };
            entry_shapes.push(shape);

            entries.push(CompiledEntry {
                field_id: entry.field_id,
                transform: entry.transform.clone(),
                dtype: entry.dtype,
                output_offset,
                mask_offset,
                element_count,
                gather_ops,
                valid_mask: region_plan.take_valid_mask(),
                valid_ratio: ratio,
            });

            output_offset += element_count;
            mask_offset += element_count;
        }

        let plan = ObsPlan {
            strategy: PlanStrategy::Simple(entries),
            output_len: output_offset,
            mask_len: mask_offset,
            compiled_generation: None,
        };

        Ok(ObsPlanResult {
            output_len: plan.output_len,
            mask_len: plan.mask_len,
            entry_shapes,
            plan,
        })
    }

    /// Compile a Standard plan (has agent-relative entries).
    ///
    /// Fixed entries are compiled with pre-computed gather (same for all agents).
    /// Agent entries are compiled as templates (resolved per-agent at execute time).
    fn compile_standard(spec: &ObsSpec, space: &dyn Space) -> Result<ObsPlanResult, ObsError> {
        let canonical = space.canonical_ordering();
        let coord_to_field_idx: IndexMap<Coord, usize> = canonical
            .into_iter()
            .enumerate()
            .map(|(idx, coord)| (coord, idx))
            .collect();

        let geometry = GridGeometry::from_space(space);
        let ndim = space.ndim();

        let mut fixed_entries = Vec::new();
        let mut agent_entries = Vec::new();
        let mut output_offset = 0usize;
        let mut mask_offset = 0usize;
        let mut entry_shapes = Vec::new();

        for (i, entry) in spec.entries.iter().enumerate() {
            match &entry.region {
                ObsRegion::Fixed(region_spec) => {
                    if entry.pool.is_some() {
                        return Err(ObsError::InvalidObsSpec {
                            reason: format!("entry {i}: pooling on Fixed regions not supported"),
                        });
                    }

                    let mut region_plan = space.compile_region(region_spec).map_err(|e| {
                        ObsError::InvalidObsSpec {
                            reason: format!("entry {i}: region compile failed: {e}"),
                        }
                    })?;

                    let ratio = region_plan.valid_ratio();
                    if ratio < COVERAGE_ERROR_THRESHOLD {
                        return Err(ObsError::InvalidComposition {
                            reason: format!(
                                "entry {i}: valid_ratio {ratio:.3} < {COVERAGE_ERROR_THRESHOLD}"
                            ),
                        });
                    }

                    let mut gather_ops = Vec::with_capacity(region_plan.coords().len());
                    for (coord_idx, coord) in region_plan.coords().iter().enumerate() {
                        let field_data_idx = *coord_to_field_idx.get(coord).ok_or_else(|| {
                            ObsError::InvalidObsSpec {
                                reason: format!(
                                    "entry {i}: coord {coord:?} not in canonical ordering"
                                ),
                            }
                        })?;
                        let tensor_idx = region_plan.tensor_indices()[coord_idx];
                        gather_ops.push(GatherOp {
                            field_data_idx,
                            tensor_idx,
                        });
                    }

                    let element_count = region_plan.bounding_shape().total_elements();
                    let shape = match region_plan.bounding_shape() {
                        murk_space::BoundingShape::Rect(dims) => dims.clone(),
                    };
                    entry_shapes.push(shape);

                    fixed_entries.push(CompiledEntry {
                        field_id: entry.field_id,
                        transform: entry.transform.clone(),
                        dtype: entry.dtype,
                        output_offset,
                        mask_offset,
                        element_count,
                        gather_ops,
                        valid_mask: region_plan.take_valid_mask(),
                        valid_ratio: ratio,
                    });

                    output_offset += element_count;
                    mask_offset += element_count;
                }

                ObsRegion::AgentDisk { radius } => {
                    let half_ext: smallvec::SmallVec<[u32; 4]> =
                        (0..ndim).map(|_| *radius).collect();
                    let (ae, shape) = Self::compile_agent_entry(
                        i,
                        entry,
                        &half_ext,
                        *radius,
                        &geometry,
                        Some(*radius),
                        output_offset,
                        mask_offset,
                    )?;
                    entry_shapes.push(shape);
                    output_offset += ae.element_count;
                    mask_offset += ae.element_count;
                    agent_entries.push(ae);
                }

                ObsRegion::AgentRect { half_extent } => {
                    let radius = *half_extent.iter().max().unwrap_or(&0);
                    let (ae, shape) = Self::compile_agent_entry(
                        i,
                        entry,
                        half_extent,
                        radius,
                        &geometry,
                        None,
                        output_offset,
                        mask_offset,
                    )?;
                    entry_shapes.push(shape);
                    output_offset += ae.element_count;
                    mask_offset += ae.element_count;
                    agent_entries.push(ae);
                }
            }
        }

        let plan = ObsPlan {
            strategy: PlanStrategy::Standard(StandardPlanData {
                fixed_entries,
                agent_entries,
                geometry,
            }),
            output_len: output_offset,
            mask_len: mask_offset,
            compiled_generation: None,
        };

        Ok(ObsPlanResult {
            output_len: plan.output_len,
            mask_len: plan.mask_len,
            entry_shapes,
            plan,
        })
    }

    /// Compile a single agent-relative entry into a template.
    ///
    /// `disk_radius`: if `Some(r)`, template ops outside graph-distance `r`
    /// are marked `in_disk = false` (for `AgentDisk`). `None` for `AgentRect`.
    #[allow(clippy::too_many_arguments)]
    fn compile_agent_entry(
        entry_idx: usize,
        entry: &crate::spec::ObsEntry,
        half_extent: &[u32],
        radius: u32,
        geometry: &Option<GridGeometry>,
        disk_radius: Option<u32>,
        output_offset: usize,
        mask_offset: usize,
    ) -> Result<(AgentCompiledEntry, Vec<usize>), ObsError> {
        let pre_pool_shape: Vec<usize> =
            half_extent.iter().map(|&he| 2 * he as usize + 1).collect();
        let pre_pool_element_count: usize = pre_pool_shape.iter().product();

        let template_ops = generate_template_ops(half_extent, geometry, disk_radius);

        let (element_count, output_shape) = if let Some(pool) = &entry.pool {
            if pre_pool_shape.len() != 2 {
                return Err(ObsError::InvalidObsSpec {
                    reason: format!(
                        "entry {entry_idx}: pooling requires 2D region, got {}D",
                        pre_pool_shape.len()
                    ),
                });
            }
            let h = pre_pool_shape[0];
            let w = pre_pool_shape[1];
            let ks = pool.kernel_size;
            let stride = pool.stride;
            if ks == 0 || stride == 0 {
                return Err(ObsError::InvalidObsSpec {
                    reason: format!("entry {entry_idx}: pool kernel_size and stride must be > 0"),
                });
            }
            let out_h = if h >= ks { (h - ks) / stride + 1 } else { 0 };
            let out_w = if w >= ks { (w - ks) / stride + 1 } else { 0 };
            if out_h == 0 || out_w == 0 {
                return Err(ObsError::InvalidObsSpec {
                    reason: format!(
                        "entry {entry_idx}: pool produces empty output \
                         (region [{h},{w}], kernel_size {ks}, stride {stride})"
                    ),
                });
            }
            (out_h * out_w, vec![out_h, out_w])
        } else {
            (pre_pool_element_count, pre_pool_shape.clone())
        };

        Ok((
            AgentCompiledEntry {
                field_id: entry.field_id,
                pool: entry.pool.clone(),
                transform: entry.transform.clone(),
                dtype: entry.dtype,
                output_offset,
                mask_offset,
                element_count,
                pre_pool_element_count,
                pre_pool_shape,
                template_ops,
                radius,
            },
            output_shape,
        ))
    }

    /// Compile with generation binding for PLAN_INVALIDATED detection.
    ///
    /// Same as [`compile`](Self::compile) but records the snapshot's
    /// `world_generation_id` for later validation in [`ObsPlan::execute`].
    pub fn compile_bound(
        spec: &ObsSpec,
        space: &dyn Space,
        generation: WorldGenerationId,
    ) -> Result<ObsPlanResult, ObsError> {
        let mut result = Self::compile(spec, space)?;
        result.plan.compiled_generation = Some(generation);
        Ok(result)
    }

    /// Total number of f32 elements in the output tensor.
    pub fn output_len(&self) -> usize {
        self.output_len
    }

    /// Total number of bytes in the validity mask.
    pub fn mask_len(&self) -> usize {
        self.mask_len
    }

    /// The generation this plan was compiled against, if bound.
    pub fn compiled_generation(&self) -> Option<WorldGenerationId> {
        self.compiled_generation
    }

    /// Execute the observation plan against a snapshot.
    ///
    /// Fills `output` with gathered and transformed field values, and
    /// `mask` with validity flags (1 = valid, 0 = padding). Both
    /// buffers must be pre-allocated to [`output_len`](Self::output_len)
    /// and [`mask_len`](Self::mask_len) respectively.
    ///
    /// `engine_tick` is the current engine tick for computing
    /// [`ObsMetadata::age_ticks`]. Pass `None` in Lockstep mode
    /// (age is always 0). In RealtimeAsync mode, pass the current
    /// engine tick so age reflects snapshot staleness.
    ///
    /// Returns [`ObsMetadata`] on success.
    ///
    /// # Errors
    ///
    /// - [`ObsError::PlanInvalidated`] if bound and generation mismatches.
    /// - [`ObsError::ExecutionFailed`] if a field is missing from the snapshot.
    pub fn execute(
        &self,
        snapshot: &dyn SnapshotAccess,
        engine_tick: Option<TickId>,
        output: &mut [f32],
        mask: &mut [u8],
    ) -> Result<ObsMetadata, ObsError> {
        let entries = match &self.strategy {
            PlanStrategy::Simple(entries) => entries,
            PlanStrategy::Standard(_) => {
                return Err(ObsError::ExecutionFailed {
                    reason: "Standard plan requires execute_agents(), not execute()".into(),
                });
            }
        };

        if output.len() < self.output_len {
            return Err(ObsError::ExecutionFailed {
                reason: format!(
                    "output buffer too small: {} < {}",
                    output.len(),
                    self.output_len
                ),
            });
        }
        if mask.len() < self.mask_len {
            return Err(ObsError::ExecutionFailed {
                reason: format!("mask buffer too small: {} < {}", mask.len(), self.mask_len),
            });
        }

        // Generation check (PLAN_INVALIDATED).
        if let Some(compiled_gen) = self.compiled_generation {
            let snapshot_gen = snapshot.world_generation_id();
            if compiled_gen != snapshot_gen {
                return Err(ObsError::PlanInvalidated {
                    reason: format!(
                        "plan compiled for generation {}, snapshot is generation {}",
                        compiled_gen.0, snapshot_gen.0
                    ),
                });
            }
        }

        let mut total_valid = 0usize;
        let mut total_elements = 0usize;

        for entry in entries {
            let field_data =
                snapshot
                    .read_field(entry.field_id)
                    .ok_or_else(|| ObsError::ExecutionFailed {
                        reason: format!("field {:?} not in snapshot", entry.field_id),
                    })?;

            let out_slice =
                &mut output[entry.output_offset..entry.output_offset + entry.element_count];
            let mask_slice = &mut mask[entry.mask_offset..entry.mask_offset + entry.element_count];

            // Initialize to zero/padding.
            out_slice.fill(0.0);
            mask_slice.copy_from_slice(&entry.valid_mask);

            // Branch-free gather: pre-computed (field_data_idx, tensor_idx) pairs.
            for op in &entry.gather_ops {
                let raw = *field_data.get(op.field_data_idx).ok_or_else(|| {
                    ObsError::ExecutionFailed {
                        reason: format!(
                            "field {:?} has {} elements but gather requires index {}",
                            entry.field_id,
                            field_data.len(),
                            op.field_data_idx,
                        ),
                    }
                })?;
                out_slice[op.tensor_idx] = apply_transform(raw, &entry.transform);
            }

            total_valid += entry.valid_mask.iter().filter(|&&v| v == 1).count();
            total_elements += entry.element_count;
        }

        let coverage = if total_elements == 0 {
            0.0
        } else {
            total_valid as f64 / total_elements as f64
        };

        let age_ticks = match engine_tick {
            Some(tick) => tick.0.saturating_sub(snapshot.tick_id().0),
            None => 0,
        };

        Ok(ObsMetadata {
            tick_id: snapshot.tick_id(),
            age_ticks,
            coverage,
            world_generation_id: snapshot.world_generation_id(),
            parameter_version: snapshot.parameter_version(),
        })
    }

    /// Execute the plan for a batch of `N` identical environments.
    ///
    /// Each snapshot in the batch fills `output_len()` elements in the
    /// output buffer, starting at `batch_idx * output_len()`. Same for
    /// masks. This is the primary interface for vectorized RL training.
    ///
    /// Returns one [`ObsMetadata`] per snapshot.
    pub fn execute_batch(
        &self,
        snapshots: &[&dyn SnapshotAccess],
        engine_tick: Option<TickId>,
        output: &mut [f32],
        mask: &mut [u8],
    ) -> Result<Vec<ObsMetadata>, ObsError> {
        // execute_batch only works with Simple plans.
        if matches!(self.strategy, PlanStrategy::Standard(_)) {
            return Err(ObsError::ExecutionFailed {
                reason: "Standard plan requires execute_agents(), not execute_batch()".into(),
            });
        }

        let batch_size = snapshots.len();
        let expected_out = batch_size * self.output_len;
        let expected_mask = batch_size * self.mask_len;

        if output.len() < expected_out {
            return Err(ObsError::ExecutionFailed {
                reason: format!(
                    "batch output buffer too small: {} < {}",
                    output.len(),
                    expected_out
                ),
            });
        }
        if mask.len() < expected_mask {
            return Err(ObsError::ExecutionFailed {
                reason: format!(
                    "batch mask buffer too small: {} < {}",
                    mask.len(),
                    expected_mask
                ),
            });
        }

        let mut metadata = Vec::with_capacity(batch_size);
        for (i, snap) in snapshots.iter().enumerate() {
            let out_start = i * self.output_len;
            let mask_start = i * self.mask_len;
            let out_slice = &mut output[out_start..out_start + self.output_len];
            let mask_slice = &mut mask[mask_start..mask_start + self.mask_len];
            let meta = self.execute(*snap, engine_tick, out_slice, mask_slice)?;
            metadata.push(meta);
        }
        Ok(metadata)
    }

    /// Execute the Standard plan for `N` agents in one environment.
    ///
    /// Each agent gets `output_len()` elements starting at
    /// `agent_idx * output_len()`. Fixed entries produce the same
    /// output for all agents; agent-relative entries are resolved
    /// per-agent using interior/boundary dispatch.
    ///
    /// Interior agents (~49% for 20×20 grid, radius 3) use a branchless
    /// fast path with stride arithmetic. Boundary agents fall back to
    /// per-cell bounds checking.
    pub fn execute_agents(
        &self,
        snapshot: &dyn SnapshotAccess,
        space: &dyn Space,
        agent_centers: &[Coord],
        engine_tick: Option<TickId>,
        output: &mut [f32],
        mask: &mut [u8],
    ) -> Result<Vec<ObsMetadata>, ObsError> {
        let standard = match &self.strategy {
            PlanStrategy::Standard(data) => data,
            PlanStrategy::Simple(_) => {
                return Err(ObsError::ExecutionFailed {
                    reason: "execute_agents requires a Standard plan \
                             (spec must contain agent-relative entries)"
                        .into(),
                });
            }
        };

        let n_agents = agent_centers.len();
        let expected_out = n_agents * self.output_len;
        let expected_mask = n_agents * self.mask_len;

        if output.len() < expected_out {
            return Err(ObsError::ExecutionFailed {
                reason: format!(
                    "output buffer too small: {} < {}",
                    output.len(),
                    expected_out
                ),
            });
        }
        if mask.len() < expected_mask {
            return Err(ObsError::ExecutionFailed {
                reason: format!("mask buffer too small: {} < {}", mask.len(), expected_mask),
            });
        }

        // Validate agent center dimensionality.
        let expected_ndim = space.ndim();
        for (i, center) in agent_centers.iter().enumerate() {
            if center.len() != expected_ndim {
                return Err(ObsError::ExecutionFailed {
                    reason: format!(
                        "agent_centers[{i}] has {} dimensions, but space requires {expected_ndim}",
                        center.len()
                    ),
                });
            }
        }

        // Generation check.
        if let Some(compiled_gen) = self.compiled_generation {
            let snapshot_gen = snapshot.world_generation_id();
            if compiled_gen != snapshot_gen {
                return Err(ObsError::PlanInvalidated {
                    reason: format!(
                        "plan compiled for generation {}, snapshot is generation {}",
                        compiled_gen.0, snapshot_gen.0
                    ),
                });
            }
        }

        // Pre-read all field data (shared borrows, valid for duration).
        let mut field_data_map: IndexMap<FieldId, &[f32]> = IndexMap::new();
        for entry in &standard.fixed_entries {
            if !field_data_map.contains_key(&entry.field_id) {
                let data = snapshot.read_field(entry.field_id).ok_or_else(|| {
                    ObsError::ExecutionFailed {
                        reason: format!("field {:?} not in snapshot", entry.field_id),
                    }
                })?;
                field_data_map.insert(entry.field_id, data);
            }
        }
        for entry in &standard.agent_entries {
            if !field_data_map.contains_key(&entry.field_id) {
                let data = snapshot.read_field(entry.field_id).ok_or_else(|| {
                    ObsError::ExecutionFailed {
                        reason: format!("field {:?} not in snapshot", entry.field_id),
                    }
                })?;
                field_data_map.insert(entry.field_id, data);
            }
        }

        let mut metadata = Vec::with_capacity(n_agents);

        for (agent_i, center) in agent_centers.iter().enumerate() {
            let out_start = agent_i * self.output_len;
            let mask_start = agent_i * self.mask_len;
            let agent_output = &mut output[out_start..out_start + self.output_len];
            let agent_mask = &mut mask[mask_start..mask_start + self.mask_len];

            agent_output.fill(0.0);
            agent_mask.fill(0);

            let mut total_valid = 0usize;
            let mut total_elements = 0usize;

            // ── Fixed entries (same for all agents) ──────────────
            for entry in &standard.fixed_entries {
                let field_data = field_data_map[&entry.field_id];
                let out_slice = &mut agent_output
                    [entry.output_offset..entry.output_offset + entry.element_count];
                let mask_slice =
                    &mut agent_mask[entry.mask_offset..entry.mask_offset + entry.element_count];

                mask_slice.copy_from_slice(&entry.valid_mask);
                for op in &entry.gather_ops {
                    let raw = *field_data.get(op.field_data_idx).ok_or_else(|| {
                        ObsError::ExecutionFailed {
                            reason: format!(
                                "field {:?} has {} elements but gather requires index {}",
                                entry.field_id,
                                field_data.len(),
                                op.field_data_idx,
                            ),
                        }
                    })?;
                    out_slice[op.tensor_idx] = apply_transform(raw, &entry.transform);
                }

                total_valid += entry.valid_mask.iter().filter(|&&v| v == 1).count();
                total_elements += entry.element_count;
            }

            // ── Agent-relative entries ───────────────────────────
            for entry in &standard.agent_entries {
                let field_data = field_data_map[&entry.field_id];

                // Fast path: stride arithmetic works only for non-wrapping
                // grids where all cells in the bounding box are in-bounds.
                // Torus (all_wrap) requires modular arithmetic → slow path.
                let use_fast_path = standard
                    .geometry
                    .as_ref()
                    .map(|geo| !geo.all_wrap && geo.is_interior(center, entry.radius))
                    .unwrap_or(false);

                let valid = execute_agent_entry(
                    entry,
                    center,
                    field_data,
                    &standard.geometry,
                    space,
                    use_fast_path,
                    agent_output,
                    agent_mask,
                );

                total_valid += valid;
                total_elements += entry.element_count;
            }

            let coverage = if total_elements == 0 {
                0.0
            } else {
                total_valid as f64 / total_elements as f64
            };

            let age_ticks = match engine_tick {
                Some(tick) => tick.0.saturating_sub(snapshot.tick_id().0),
                None => 0,
            };

            metadata.push(ObsMetadata {
                tick_id: snapshot.tick_id(),
                age_ticks,
                coverage,
                world_generation_id: snapshot.world_generation_id(),
                parameter_version: snapshot.parameter_version(),
            });
        }

        Ok(metadata)
    }

    /// Whether this plan requires `execute_agents` (Standard) or `execute` (Simple).
    pub fn is_standard(&self) -> bool {
        matches!(self.strategy, PlanStrategy::Standard(_))
    }
}

/// Execute a single agent-relative entry for one agent.
///
/// Returns the number of valid cells written.
#[allow(clippy::too_many_arguments)]
fn execute_agent_entry(
    entry: &AgentCompiledEntry,
    center: &Coord,
    field_data: &[f32],
    geometry: &Option<GridGeometry>,
    space: &dyn Space,
    use_fast_path: bool,
    agent_output: &mut [f32],
    agent_mask: &mut [u8],
) -> usize {
    if entry.pool.is_some() {
        execute_agent_entry_pooled(
            entry,
            center,
            field_data,
            geometry,
            space,
            use_fast_path,
            agent_output,
            agent_mask,
        )
    } else {
        execute_agent_entry_direct(
            entry,
            center,
            field_data,
            geometry,
            space,
            use_fast_path,
            agent_output,
            agent_mask,
        )
    }
}

/// Direct gather (no pooling): gather + transform → output.
#[allow(clippy::too_many_arguments)]
fn execute_agent_entry_direct(
    entry: &AgentCompiledEntry,
    center: &Coord,
    field_data: &[f32],
    geometry: &Option<GridGeometry>,
    space: &dyn Space,
    use_fast_path: bool,
    agent_output: &mut [f32],
    agent_mask: &mut [u8],
) -> usize {
    let out_slice =
        &mut agent_output[entry.output_offset..entry.output_offset + entry.element_count];
    let mask_slice = &mut agent_mask[entry.mask_offset..entry.mask_offset + entry.element_count];

    if use_fast_path {
        // FAST PATH: all cells in-bounds, branchless stride arithmetic.
        let geo = geometry.as_ref().unwrap();
        let base_rank = geo.canonical_rank(center) as isize;
        let mut valid = 0;
        for op in &entry.template_ops {
            if !op.in_disk {
                continue;
            }
            let field_idx = (base_rank + op.stride_offset) as usize;
            if let Some(&val) = field_data.get(field_idx) {
                out_slice[op.tensor_idx] = apply_transform(val, &entry.transform);
                mask_slice[op.tensor_idx] = 1;
                valid += 1;
            }
        }
        valid
    } else {
        // SLOW PATH: bounds-check each offset (or modular wrap for torus).
        let mut valid = 0;
        for op in &entry.template_ops {
            if !op.in_disk {
                continue;
            }
            let field_idx = resolve_field_index(center, &op.relative, geometry, space);
            if let Some(idx) = field_idx {
                if idx < field_data.len() {
                    out_slice[op.tensor_idx] = apply_transform(field_data[idx], &entry.transform);
                    mask_slice[op.tensor_idx] = 1;
                    valid += 1;
                }
            }
        }
        valid
    }
}

/// Pooled gather: gather → scratch → pool → transform → output.
#[allow(clippy::too_many_arguments)]
fn execute_agent_entry_pooled(
    entry: &AgentCompiledEntry,
    center: &Coord,
    field_data: &[f32],
    geometry: &Option<GridGeometry>,
    space: &dyn Space,
    use_fast_path: bool,
    agent_output: &mut [f32],
    agent_mask: &mut [u8],
) -> usize {
    let mut scratch = vec![0.0f32; entry.pre_pool_element_count];
    let mut scratch_mask = vec![0u8; entry.pre_pool_element_count];

    if use_fast_path {
        let geo = geometry.as_ref().unwrap();
        let base_rank = geo.canonical_rank(center) as isize;
        for op in &entry.template_ops {
            if !op.in_disk {
                continue;
            }
            let field_idx = (base_rank + op.stride_offset) as usize;
            if let Some(&val) = field_data.get(field_idx) {
                scratch[op.tensor_idx] = val;
                scratch_mask[op.tensor_idx] = 1;
            }
        }
    } else {
        for op in &entry.template_ops {
            if !op.in_disk {
                continue;
            }
            let field_idx = resolve_field_index(center, &op.relative, geometry, space);
            if let Some(idx) = field_idx {
                if idx < field_data.len() {
                    scratch[op.tensor_idx] = field_data[idx];
                    scratch_mask[op.tensor_idx] = 1;
                }
            }
        }
    }

    let pool_config = entry.pool.as_ref().unwrap();
    let (pooled, pooled_mask, _) =
        pool_2d(&scratch, &scratch_mask, &entry.pre_pool_shape, pool_config);

    let out_slice =
        &mut agent_output[entry.output_offset..entry.output_offset + entry.element_count];
    let mask_slice = &mut agent_mask[entry.mask_offset..entry.mask_offset + entry.element_count];

    let n = pooled.len().min(entry.element_count);
    for i in 0..n {
        out_slice[i] = apply_transform(pooled[i], &entry.transform);
    }
    mask_slice[..n].copy_from_slice(&pooled_mask[..n]);

    pooled_mask[..n].iter().filter(|&&v| v == 1).count()
}

/// Generate template operations for a rectangular bounding box.
///
/// `half_extent[d]` is the half-size per dimension. The bounding box is
/// `(2*he[0]+1) × (2*he[1]+1) × ...` in row-major order.
///
/// If `strides` is provided (from `GridGeometry`), each op gets a precomputed
/// `stride_offset` for the interior fast path.
///
/// If `disk_radius` is `Some(r)`, cells with graph distance > `r` are marked
/// `in_disk = false`. The `geometry` is required to compute graph distance.
/// When `geometry` is `None`, all cells are treated as in-disk (conservative).
fn generate_template_ops(
    half_extent: &[u32],
    geometry: &Option<GridGeometry>,
    disk_radius: Option<u32>,
) -> Vec<TemplateOp> {
    let ndim = half_extent.len();
    let shape: Vec<usize> = half_extent.iter().map(|&he| 2 * he as usize + 1).collect();
    let total: usize = shape.iter().product();

    let strides = geometry.as_ref().map(|g| g.coord_strides.as_slice());

    let mut ops = Vec::with_capacity(total);

    for tensor_idx in 0..total {
        // Decompose tensor_idx into n-d relative coords (row-major).
        let mut relative = Coord::new();
        let mut remaining = tensor_idx;
        // Build in reverse order, then reverse.
        for d in (0..ndim).rev() {
            let coord = (remaining % shape[d]) as i32 - half_extent[d] as i32;
            relative.push(coord);
            remaining /= shape[d];
        }
        relative.reverse();

        let stride_offset = strides
            .map(|s| {
                relative
                    .iter()
                    .zip(s.iter())
                    .map(|(&r, &s)| r as isize * s as isize)
                    .sum::<isize>()
            })
            .unwrap_or(0);

        let in_disk = match disk_radius {
            Some(r) => match geometry {
                Some(geo) => geo.graph_distance(&relative) <= r,
                None => true, // no geometry → conservative (include all)
            },
            None => true, // AgentRect → all cells valid
        };

        ops.push(TemplateOp {
            relative,
            tensor_idx,
            stride_offset,
            in_disk,
        });
    }

    ops
}

/// Resolve the field data index for an absolute coordinate.
///
/// Handles three cases:
/// 1. Torus (all_wrap): modular wrap, always in-bounds.
/// 2. Grid with geometry: bounds-check then stride arithmetic.
/// 3. No geometry: fall back to `space.canonical_rank()`.
fn resolve_field_index(
    center: &Coord,
    relative: &Coord,
    geometry: &Option<GridGeometry>,
    space: &dyn Space,
) -> Option<usize> {
    if let Some(geo) = geometry {
        if geo.all_wrap {
            // Torus: wrap coordinates with modular arithmetic.
            let wrapped: Coord = center
                .iter()
                .zip(relative.iter())
                .zip(geo.coord_dims.iter())
                .map(|((&c, &r), &d)| {
                    let d = d as i32;
                    ((c + r) % d + d) % d
                })
                .collect();
            Some(geo.canonical_rank(&wrapped))
        } else {
            let abs_coord: Coord = center
                .iter()
                .zip(relative.iter())
                .map(|(&c, &r)| c + r)
                .collect();
            let abs_slice: &[i32] = &abs_coord;
            if geo.in_bounds(abs_slice) {
                Some(geo.canonical_rank(abs_slice))
            } else {
                None
            }
        }
    } else {
        let abs_coord: Coord = center
            .iter()
            .zip(relative.iter())
            .map(|(&c, &r)| c + r)
            .collect();
        space.canonical_rank(&abs_coord)
    }
}

/// Apply a transform to a raw field value.
fn apply_transform(raw: f32, transform: &ObsTransform) -> f32 {
    match transform {
        ObsTransform::Identity => raw,
        ObsTransform::Normalize { min, max } => {
            let range = max - min;
            if range == 0.0 {
                0.0
            } else {
                let normalized = (raw as f64 - min) / range;
                normalized.clamp(0.0, 1.0) as f32
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{
        ObsDtype, ObsEntry, ObsRegion, ObsSpec, ObsTransform, PoolConfig, PoolKernel,
    };
    use murk_core::{FieldId, ParameterVersion, TickId, WorldGenerationId};
    use murk_space::{EdgeBehavior, Hex2D, RegionSpec, Square4, Square8};
    use murk_test_utils::MockSnapshot;

    fn square4_space() -> Square4 {
        Square4::new(3, 3, EdgeBehavior::Absorb).unwrap()
    }

    fn snapshot_with_field(field: FieldId, data: Vec<f32>) -> MockSnapshot {
        let mut snap = MockSnapshot::new(TickId(5), WorldGenerationId(1), ParameterVersion(0));
        snap.set_field(field, data);
        snap
    }

    // ── Compilation tests ────────────────────────────────────

    #[test]
    fn compile_empty_spec_errors() {
        let space = square4_space();
        let spec = ObsSpec { entries: vec![] };
        let err = ObsPlan::compile(&spec, &space).unwrap_err();
        assert!(matches!(err, ObsError::InvalidObsSpec { .. }));
    }

    #[test]
    fn compile_all_region_square4() {
        let space = square4_space();
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();
        assert_eq!(result.output_len, 9); // 3x3
        assert_eq!(result.mask_len, 9);
        assert_eq!(result.entry_shapes, vec![vec![3, 3]]);
    }

    #[test]
    fn compile_rect_region() {
        let space = Square4::new(5, 5, EdgeBehavior::Absorb).unwrap();
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::Rect {
                    min: smallvec::smallvec![1, 1],
                    max: smallvec::smallvec![2, 3],
                }),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();
        // 2 rows x 3 cols = 6 cells
        assert_eq!(result.output_len, 6);
        assert_eq!(result.entry_shapes, vec![vec![2, 3]]);
    }

    #[test]
    fn compile_two_entries_offsets() {
        let space = square4_space();
        let spec = ObsSpec {
            entries: vec![
                ObsEntry {
                    field_id: FieldId(0),
                    region: ObsRegion::Fixed(RegionSpec::All),
                    pool: None,
                    transform: ObsTransform::Identity,
                    dtype: ObsDtype::F32,
                },
                ObsEntry {
                    field_id: FieldId(1),
                    region: ObsRegion::Fixed(RegionSpec::All),
                    pool: None,
                    transform: ObsTransform::Identity,
                    dtype: ObsDtype::F32,
                },
            ],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();
        assert_eq!(result.output_len, 18); // 9 + 9
        assert_eq!(result.mask_len, 18);
    }

    #[test]
    fn compile_invalid_region_errors() {
        let space = square4_space();
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::Coords(vec![smallvec::smallvec![99, 99]])),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let err = ObsPlan::compile(&spec, &space).unwrap_err();
        assert!(matches!(err, ObsError::InvalidObsSpec { .. }));
    }

    // ── Execution tests ──────────────────────────────────────

    #[test]
    fn execute_identity_all_region() {
        let space = square4_space();
        // Field data in canonical (row-major) order for 3x3:
        // (0,0)=1, (0,1)=2, (0,2)=3, (1,0)=4, ..., (2,2)=9
        let data: Vec<f32> = (1..=9).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let meta = result
            .plan
            .execute(&snap, None, &mut output, &mut mask)
            .unwrap();

        // Output should match field data in canonical order.
        let expected: Vec<f32> = (1..=9).map(|x| x as f32).collect();
        assert_eq!(output, expected);
        assert_eq!(mask, vec![1u8; 9]);
        assert_eq!(meta.tick_id, TickId(5));
        assert_eq!(meta.coverage, 1.0);
        assert_eq!(meta.world_generation_id, WorldGenerationId(1));
        assert_eq!(meta.parameter_version, ParameterVersion(0));
        assert_eq!(meta.age_ticks, 0);
    }

    #[test]
    fn execute_normalize_transform() {
        let space = square4_space();
        // Values 0..8 mapped to [0,1] with min=0, max=8.
        let data: Vec<f32> = (0..9).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Normalize { min: 0.0, max: 8.0 },
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result
            .plan
            .execute(&snap, None, &mut output, &mut mask)
            .unwrap();

        // Each value x should be x/8.
        for (i, &v) in output.iter().enumerate() {
            let expected = i as f32 / 8.0;
            assert!(
                (v - expected).abs() < 1e-6,
                "output[{i}] = {v}, expected {expected}"
            );
        }
    }

    #[test]
    fn execute_normalize_clamps_out_of_range() {
        let space = square4_space();
        // Values -5, 0, 5, 10, 15 etc.
        let data: Vec<f32> = (-4..5).map(|x| x as f32 * 5.0).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Normalize {
                    min: 0.0,
                    max: 10.0,
                },
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result
            .plan
            .execute(&snap, None, &mut output, &mut mask)
            .unwrap();

        for &v in &output {
            assert!((0.0..=1.0).contains(&v), "value {v} out of [0,1] range");
        }
    }

    #[test]
    fn execute_normalize_zero_range() {
        let space = square4_space();
        let data = vec![5.0f32; 9];
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Normalize { min: 5.0, max: 5.0 },
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![-1.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result
            .plan
            .execute(&snap, None, &mut output, &mut mask)
            .unwrap();

        // Zero range → all outputs 0.0.
        assert!(output.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn execute_rect_subregion_correct_values() {
        let space = Square4::new(4, 4, EdgeBehavior::Absorb).unwrap();
        // 4x4 field: value = row * 4 + col + 1.
        let data: Vec<f32> = (1..=16).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::Rect {
                    min: smallvec::smallvec![1, 1],
                    max: smallvec::smallvec![2, 2],
                }),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();
        assert_eq!(result.output_len, 4); // 2x2

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result
            .plan
            .execute(&snap, None, &mut output, &mut mask)
            .unwrap();

        // Rect covers (1,1)=6, (1,2)=7, (2,1)=10, (2,2)=11
        assert_eq!(output, vec![6.0, 7.0, 10.0, 11.0]);
        assert_eq!(mask, vec![1, 1, 1, 1]);
    }

    #[test]
    fn execute_two_fields() {
        let space = square4_space();
        let data_a: Vec<f32> = (1..=9).map(|x| x as f32).collect();
        let data_b: Vec<f32> = (10..=18).map(|x| x as f32).collect();
        let mut snap = MockSnapshot::new(TickId(1), WorldGenerationId(1), ParameterVersion(0));
        snap.set_field(FieldId(0), data_a);
        snap.set_field(FieldId(1), data_b);

        let spec = ObsSpec {
            entries: vec![
                ObsEntry {
                    field_id: FieldId(0),
                    region: ObsRegion::Fixed(RegionSpec::All),
                    pool: None,
                    transform: ObsTransform::Identity,
                    dtype: ObsDtype::F32,
                },
                ObsEntry {
                    field_id: FieldId(1),
                    region: ObsRegion::Fixed(RegionSpec::All),
                    pool: None,
                    transform: ObsTransform::Identity,
                    dtype: ObsDtype::F32,
                },
            ],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();
        assert_eq!(result.output_len, 18);

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result
            .plan
            .execute(&snap, None, &mut output, &mut mask)
            .unwrap();

        // First 9: field 0, next 9: field 1.
        let expected_a: Vec<f32> = (1..=9).map(|x| x as f32).collect();
        let expected_b: Vec<f32> = (10..=18).map(|x| x as f32).collect();
        assert_eq!(&output[..9], &expected_a);
        assert_eq!(&output[9..], &expected_b);
    }

    #[test]
    fn execute_missing_field_errors() {
        let space = square4_space();
        let snap = MockSnapshot::new(TickId(1), WorldGenerationId(1), ParameterVersion(0));

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let err = result
            .plan
            .execute(&snap, None, &mut output, &mut mask)
            .unwrap_err();
        assert!(matches!(err, ObsError::ExecutionFailed { .. }));
    }

    #[test]
    fn execute_buffer_too_small_errors() {
        let space = square4_space();
        let data: Vec<f32> = vec![0.0; 9];
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; 4]; // too small
        let mut mask = vec![0u8; result.mask_len];
        let err = result
            .plan
            .execute(&snap, None, &mut output, &mut mask)
            .unwrap_err();
        assert!(matches!(err, ObsError::ExecutionFailed { .. }));
    }

    // ── Validity / coverage tests ────────────────────────────

    #[test]
    fn valid_ratio_one_for_square_all() {
        let space = square4_space();
        let data: Vec<f32> = vec![1.0; 9];
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let meta = result
            .plan
            .execute(&snap, None, &mut output, &mut mask)
            .unwrap();

        assert_eq!(meta.coverage, 1.0);
    }

    // ── Generation binding tests ─────────────────────────────

    #[test]
    fn plan_invalidated_on_generation_mismatch() {
        let space = square4_space();
        let data: Vec<f32> = vec![1.0; 9];
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        // Compile bound to generation 99, but snapshot is generation 1.
        let result = ObsPlan::compile_bound(&spec, &space, WorldGenerationId(99)).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let err = result
            .plan
            .execute(&snap, None, &mut output, &mut mask)
            .unwrap_err();
        assert!(matches!(err, ObsError::PlanInvalidated { .. }));
    }

    #[test]
    fn generation_match_succeeds() {
        let space = square4_space();
        let data: Vec<f32> = vec![1.0; 9];
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile_bound(&spec, &space, WorldGenerationId(1)).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result
            .plan
            .execute(&snap, None, &mut output, &mut mask)
            .unwrap();
    }

    #[test]
    fn unbound_plan_ignores_generation() {
        let space = square4_space();
        let data: Vec<f32> = vec![1.0; 9];
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        // Unbound plan — no generation check.
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result
            .plan
            .execute(&snap, None, &mut output, &mut mask)
            .unwrap();
    }

    // ── Metadata tests ───────────────────────────────────────

    #[test]
    fn metadata_fields_populated() {
        let space = square4_space();
        let data: Vec<f32> = vec![1.0; 9];
        let mut snap = MockSnapshot::new(TickId(42), WorldGenerationId(7), ParameterVersion(3));
        snap.set_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let meta = result
            .plan
            .execute(&snap, None, &mut output, &mut mask)
            .unwrap();

        assert_eq!(meta.tick_id, TickId(42));
        assert_eq!(meta.age_ticks, 0);
        assert_eq!(meta.coverage, 1.0);
        assert_eq!(meta.world_generation_id, WorldGenerationId(7));
        assert_eq!(meta.parameter_version, ParameterVersion(3));
    }

    // ── Batch execution tests ────────────────────────────────

    #[test]
    fn execute_batch_n1_matches_execute() {
        let space = square4_space();
        let data: Vec<f32> = (1..=9).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data.clone());

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        // Single execute.
        let mut out_single = vec![0.0f32; result.output_len];
        let mut mask_single = vec![0u8; result.mask_len];
        let meta_single = result
            .plan
            .execute(&snap, None, &mut out_single, &mut mask_single)
            .unwrap();

        // Batch N=1.
        let mut out_batch = vec![0.0f32; result.output_len];
        let mut mask_batch = vec![0u8; result.mask_len];
        let snap_ref: &dyn SnapshotAccess = &snap;
        let meta_batch = result
            .plan
            .execute_batch(&[snap_ref], None, &mut out_batch, &mut mask_batch)
            .unwrap();

        assert_eq!(out_single, out_batch);
        assert_eq!(mask_single, mask_batch);
        assert_eq!(meta_single, meta_batch[0]);
    }

    #[test]
    fn execute_batch_multiple_snapshots() {
        let space = square4_space();
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let snap_a = snapshot_with_field(FieldId(0), vec![1.0; 9]);
        let snap_b = snapshot_with_field(FieldId(0), vec![2.0; 9]);

        let snaps: Vec<&dyn SnapshotAccess> = vec![&snap_a, &snap_b];
        let mut output = vec![0.0f32; result.output_len * 2];
        let mut mask = vec![0u8; result.mask_len * 2];
        let metas = result
            .plan
            .execute_batch(&snaps, None, &mut output, &mut mask)
            .unwrap();

        assert_eq!(metas.len(), 2);
        assert!(output[..9].iter().all(|&v| v == 1.0));
        assert!(output[9..].iter().all(|&v| v == 2.0));
    }

    #[test]
    fn execute_batch_buffer_too_small() {
        let space = square4_space();
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let snap = snapshot_with_field(FieldId(0), vec![1.0; 9]);
        let snaps: Vec<&dyn SnapshotAccess> = vec![&snap, &snap];
        let mut output = vec![0.0f32; 9]; // need 18
        let mut mask = vec![0u8; 18];
        let err = result
            .plan
            .execute_batch(&snaps, None, &mut output, &mut mask)
            .unwrap_err();
        assert!(matches!(err, ObsError::ExecutionFailed { .. }));
    }

    // ── Field length mismatch tests ──────────────────────────

    #[test]
    fn short_field_buffer_returns_error_not_panic() {
        let space = square4_space(); // 3x3 = 9 cells
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        // Snapshot field has only 4 elements, but plan expects 9.
        let snap = snapshot_with_field(FieldId(0), vec![1.0; 4]);
        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let err = result
            .plan
            .execute(&snap, None, &mut output, &mut mask)
            .unwrap_err();
        assert!(matches!(err, ObsError::ExecutionFailed { .. }));
    }

    // ── Standard plan (agent-centered) tests ─────────────────

    #[test]
    fn standard_plan_detected_from_agent_region() {
        let space = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentRect {
                    half_extent: smallvec::smallvec![2, 2],
                },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();
        assert!(result.plan.is_standard());
        // 5x5 = 25 elements
        assert_eq!(result.output_len, 25);
        assert_eq!(result.entry_shapes, vec![vec![5, 5]]);
    }

    #[test]
    fn execute_on_standard_plan_errors() {
        let space = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let data: Vec<f32> = (0..100).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentDisk { radius: 2 },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let err = result
            .plan
            .execute(&snap, None, &mut output, &mut mask)
            .unwrap_err();
        assert!(matches!(err, ObsError::ExecutionFailed { .. }));
    }

    #[test]
    fn interior_boundary_equivalence() {
        // An INTERIOR agent using Standard plan should produce identical
        // output to a Simple plan with an explicit Rect at the same position.
        let space = Square4::new(20, 20, EdgeBehavior::Absorb).unwrap();
        let data: Vec<f32> = (0..400).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let radius = 3u32;
        let center: Coord = smallvec::smallvec![10, 10]; // interior

        // Standard plan: AgentRect centered on agent.
        let standard_spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentRect {
                    half_extent: smallvec::smallvec![radius, radius],
                },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let std_result = ObsPlan::compile(&standard_spec, &space).unwrap();
        let mut std_output = vec![0.0f32; std_result.output_len];
        let mut std_mask = vec![0u8; std_result.mask_len];
        std_result
            .plan
            .execute_agents(
                &snap,
                &space,
                std::slice::from_ref(&center),
                None,
                &mut std_output,
                &mut std_mask,
            )
            .unwrap();

        // Simple plan: explicit Rect covering the same area.
        let r = radius as i32;
        let simple_spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::Rect {
                    min: smallvec::smallvec![10 - r, 10 - r],
                    max: smallvec::smallvec![10 + r, 10 + r],
                }),
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let simple_result = ObsPlan::compile(&simple_spec, &space).unwrap();
        let mut simple_output = vec![0.0f32; simple_result.output_len];
        let mut simple_mask = vec![0u8; simple_result.mask_len];
        simple_result
            .plan
            .execute(&snap, None, &mut simple_output, &mut simple_mask)
            .unwrap();

        // Same shape, same values.
        assert_eq!(std_result.output_len, simple_result.output_len);
        assert_eq!(std_output, simple_output);
        assert_eq!(std_mask, simple_mask);
    }

    #[test]
    fn boundary_agent_gets_padding() {
        // Agent at corner (0,0) with radius 2: many cells out-of-bounds.
        let space = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let data: Vec<f32> = (0..100).map(|x| x as f32 + 1.0).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentRect {
                    half_extent: smallvec::smallvec![2, 2],
                },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();
        let center: Coord = smallvec::smallvec![0, 0];
        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let metas = result
            .plan
            .execute_agents(&snap, &space, &[center], None, &mut output, &mut mask)
            .unwrap();

        // 5x5 = 25 cells total. Agent at (0,0) with radius 2:
        // Only cells with row in [0,2] and col in [0,2] are valid (3x3 = 9).
        let valid_count: usize = mask.iter().filter(|&&v| v == 1).count();
        assert_eq!(valid_count, 9);

        // Coverage should be 9/25
        assert!((metas[0].coverage - 9.0 / 25.0).abs() < 1e-6);

        // Check that the top-left corner of the bounding box (relative [-2,-2])
        // is padding (mask=0, value=0).
        assert_eq!(mask[0], 0); // relative (-2,-2) → absolute (-2,-2) → out of bounds
        assert_eq!(output[0], 0.0);

        // The cell at relative (0,0) is at tensor_idx = 2*5+2 = 12
        // Absolute (0,0) → field value = 1.0
        assert_eq!(mask[12], 1);
        assert_eq!(output[12], 1.0);
    }

    #[test]
    fn hex_foveation_interior() {
        // Test agent-centered observation on Hex2D grid.
        let space = Hex2D::new(20, 20).unwrap(); // 20 rows, 20 cols
        let data: Vec<f32> = (0..400).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentDisk { radius: 2 },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();
        assert_eq!(result.output_len, 25); // 5x5 bounding box (tensor shape)

        // Interior agent: q=10, r=10
        let center: Coord = smallvec::smallvec![10, 10];
        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result
            .plan
            .execute_agents(&snap, &space, &[center], None, &mut output, &mut mask)
            .unwrap();

        // Hex disk of radius 2: 19 of 25 cells are within hex distance.
        // The 6 corners of the 5x5 bounding box exceed hex distance 2.
        // Hex distance = max(|dq|, |dr|, |dq+dr|) for axial coordinates.
        let valid_count = mask.iter().filter(|&&v| v == 1).count();
        assert_eq!(valid_count, 19);

        // Corners that should be masked out (distance > 2):
        // tensor_idx 0: dq=-2,dr=-2 → max(2,2,4)=4
        // tensor_idx 1: dq=-2,dr=-1 → max(2,1,3)=3
        // tensor_idx 5: dq=-1,dr=-2 → max(1,2,3)=3
        // tensor_idx 19: dq=+1,dr=+2 → max(1,2,3)=3
        // tensor_idx 23: dq=+2,dr=+1 → max(2,1,3)=3
        // tensor_idx 24: dq=+2,dr=+2 → max(2,2,4)=4
        for &idx in &[0, 1, 5, 19, 23, 24] {
            assert_eq!(mask[idx], 0, "tensor_idx {idx} should be outside hex disk");
            assert_eq!(output[idx], 0.0, "tensor_idx {idx} should be zero-padded");
        }

        // Center cell is at tensor_idx = 2*5+2 = 12 (relative [0,0]).
        // Hex2D canonical_rank([q,r]) = r*cols + q = 10*20 + 10 = 210
        assert_eq!(output[12], 210.0);

        // Cell at relative [1, 0] (dq=+1, dr=0) → absolute [11, 10]
        // rank = 10*20 + 11 = 211
        // In row-major bounding box: dim0_idx=3, dim1_idx=2 → tensor_idx = 3*5+2 = 17
        assert_eq!(output[17], 211.0);
    }

    #[test]
    fn wrap_space_all_interior() {
        // Wrapped (torus) space: all agents are interior.
        let space = Square4::new(10, 10, EdgeBehavior::Wrap).unwrap();
        let data: Vec<f32> = (0..100).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentRect {
                    half_extent: smallvec::smallvec![2, 2],
                },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        // Agent at corner (0,0) — still interior on torus.
        let center: Coord = smallvec::smallvec![0, 0];
        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result
            .plan
            .execute_agents(&snap, &space, &[center], None, &mut output, &mut mask)
            .unwrap();

        // All 25 cells valid (torus wraps).
        assert!(mask.iter().all(|&v| v == 1));
        assert_eq!(output[12], 0.0); // center (0,0) → rank 0
    }

    #[test]
    fn execute_agents_multiple() {
        let space = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let data: Vec<f32> = (0..100).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentRect {
                    half_extent: smallvec::smallvec![1, 1],
                },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();
        assert_eq!(result.output_len, 9); // 3x3

        // Two agents: one interior, one at edge.
        let centers = vec![
            smallvec::smallvec![5, 5], // interior
            smallvec::smallvec![0, 5], // top edge
        ];
        let n = centers.len();
        let mut output = vec![0.0f32; result.output_len * n];
        let mut mask = vec![0u8; result.mask_len * n];
        let metas = result
            .plan
            .execute_agents(&snap, &space, &centers, None, &mut output, &mut mask)
            .unwrap();

        assert_eq!(metas.len(), 2);

        // Agent 0 (interior): all 9 cells valid, center = (5,5) → rank 55
        assert!(mask[..9].iter().all(|&v| v == 1));
        assert_eq!(output[4], 55.0); // center at tensor_idx = 1*3+1 = 4

        // Agent 1 (top edge): row -1 is out of bounds → 3 cells masked
        let agent1_mask = &mask[9..18];
        let valid_count: usize = agent1_mask.iter().filter(|&&v| v == 1).count();
        assert_eq!(valid_count, 6); // 2 rows in-bounds × 3 cols
    }

    #[test]
    fn execute_agents_with_normalize() {
        let space = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let data: Vec<f32> = (0..100).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentRect {
                    half_extent: smallvec::smallvec![1, 1],
                },
                pool: None,
                transform: ObsTransform::Normalize {
                    min: 0.0,
                    max: 99.0,
                },
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let center: Coord = smallvec::smallvec![5, 5];
        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result
            .plan
            .execute_agents(&snap, &space, &[center], None, &mut output, &mut mask)
            .unwrap();

        // Center (5,5) rank=55, normalized = 55/99 ≈ 0.5556
        let expected = 55.0 / 99.0;
        assert!((output[4] - expected as f32).abs() < 1e-5);
    }

    #[test]
    fn execute_agents_with_pooling() {
        let space = Square4::new(20, 20, EdgeBehavior::Absorb).unwrap();
        let data: Vec<f32> = (0..400).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        // AgentRect with half_extent=3 → 7x7 bounding box.
        // Mean pool 2x2 stride 2 → floor((7-2)/2)+1 = 3 per dim → 3x3 = 9 output.
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentRect {
                    half_extent: smallvec::smallvec![3, 3],
                },
                pool: Some(PoolConfig {
                    kernel: PoolKernel::Mean,
                    kernel_size: 2,
                    stride: 2,
                }),
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();
        assert_eq!(result.output_len, 9); // 3x3
        assert_eq!(result.entry_shapes, vec![vec![3, 3]]);

        // Interior agent at (10, 10): all cells valid.
        let center: Coord = smallvec::smallvec![10, 10];
        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result
            .plan
            .execute_agents(&snap, &space, &[center], None, &mut output, &mut mask)
            .unwrap();

        // All pooled cells should be valid.
        assert!(mask.iter().all(|&v| v == 1));

        // Verify first pooled cell: mean of top-left 2x2 of the 7x7 gather.
        // Gather bounding box starts at (10-3, 10-3) = (7, 7).
        // Top-left 2x2: (7,7)=147, (7,8)=148, (8,7)=167, (8,8)=168
        // Mean = (147+148+167+168)/4 = 157.5
        assert!((output[0] - 157.5).abs() < 1e-4);
    }

    #[test]
    fn mixed_fixed_and_agent_entries() {
        let space = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let data: Vec<f32> = (0..100).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![
                // Fixed entry: full grid (100 elements).
                ObsEntry {
                    field_id: FieldId(0),
                    region: ObsRegion::Fixed(RegionSpec::All),
                    pool: None,
                    transform: ObsTransform::Identity,
                    dtype: ObsDtype::F32,
                },
                // Agent entry: 3x3 rect around agent.
                ObsEntry {
                    field_id: FieldId(0),
                    region: ObsRegion::AgentRect {
                        half_extent: smallvec::smallvec![1, 1],
                    },
                    pool: None,
                    transform: ObsTransform::Identity,
                    dtype: ObsDtype::F32,
                },
            ],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();
        assert!(result.plan.is_standard());
        assert_eq!(result.output_len, 109); // 100 + 9

        let center: Coord = smallvec::smallvec![5, 5];
        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result
            .plan
            .execute_agents(&snap, &space, &[center], None, &mut output, &mut mask)
            .unwrap();

        // Fixed entry: first 100 elements match field data.
        let expected: Vec<f32> = (0..100).map(|x| x as f32).collect();
        assert_eq!(&output[..100], &expected[..]);
        assert!(mask[..100].iter().all(|&v| v == 1));

        // Agent entry: 3x3 centered on (5,5). Center at tensor_idx = 1*3+1 = 4.
        // rank(5,5) = 55
        assert_eq!(output[100 + 4], 55.0);
    }

    #[test]
    fn wrong_dimensionality_returns_error() {
        // 2D space but 1D agent center → should error, not panic.
        let space = Square4::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let data: Vec<f32> = (0..100).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentDisk { radius: 1 },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let bad_center: Coord = smallvec::smallvec![5]; // 1D, not 2D
        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let err =
            result
                .plan
                .execute_agents(&snap, &space, &[bad_center], None, &mut output, &mut mask);
        assert!(err.is_err());
        let msg = format!("{}", err.unwrap_err());
        assert!(
            msg.contains("dimensions"),
            "error should mention dimensions: {msg}"
        );
    }

    #[test]
    fn agent_disk_square4_filters_corners() {
        // On a 4-connected grid, AgentDisk radius=2 should use Manhattan distance.
        // Bounding box is 5x5 = 25, but Manhattan disk has 13 cells (diamond shape).
        let space = Square4::new(20, 20, EdgeBehavior::Absorb).unwrap();
        let data: Vec<f32> = (0..400).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentDisk { radius: 2 },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();
        assert_eq!(result.output_len, 25); // tensor shape is still 5x5

        // Interior agent at (10, 10).
        let center: Coord = smallvec::smallvec![10, 10];
        let mut output = vec![0.0f32; 25];
        let mut mask = vec![0u8; 25];
        result
            .plan
            .execute_agents(&snap, &space, &[center], None, &mut output, &mut mask)
            .unwrap();

        // Manhattan distance disk of radius 2 on a 5x5 bounding box:
        //   . . X . .    (row -2: only center col)
        //   . X X X .    (row -1: 3 cells)
        //   X X X X X    (row  0: 5 cells)
        //   . X X X .    (row +1: 3 cells)
        //   . . X . .    (row +2: only center col)
        // Total: 1 + 3 + 5 + 3 + 1 = 13 cells
        let valid_count = mask.iter().filter(|&&v| v == 1).count();
        assert_eq!(
            valid_count, 13,
            "Manhattan disk radius=2 should have 13 cells"
        );

        // Corners should be masked out: (dr,dc) where |dr|+|dc| > 2
        // tensor_idx 0: dr=-2,dc=-2 → dist=4 → OUT
        // tensor_idx 4: dr=-2,dc=+2 → dist=4 → OUT
        // tensor_idx 20: dr=+2,dc=-2 → dist=4 → OUT
        // tensor_idx 24: dr=+2,dc=+2 → dist=4 → OUT
        for &idx in &[0, 4, 20, 24] {
            assert_eq!(
                mask[idx], 0,
                "corner tensor_idx {idx} should be outside disk"
            );
        }

        // Center cell: tensor_idx = 2*5+2 = 12, absolute = row 10 * 20 + col 10 = 210
        assert_eq!(output[12], 210.0);
        assert_eq!(mask[12], 1);
    }

    #[test]
    fn agent_rect_no_disk_filtering() {
        // AgentRect should NOT filter any cells — full rectangle is valid.
        let space = Square4::new(20, 20, EdgeBehavior::Absorb).unwrap();
        let data: Vec<f32> = (0..400).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentRect {
                    half_extent: smallvec::smallvec![2, 2],
                },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let center: Coord = smallvec::smallvec![10, 10];
        let mut output = vec![0.0f32; 25];
        let mut mask = vec![0u8; 25];
        result
            .plan
            .execute_agents(&snap, &space, &[center], None, &mut output, &mut mask)
            .unwrap();

        // All 25 cells should be valid for AgentRect (no disk filtering).
        assert!(mask.iter().all(|&v| v == 1));
    }

    #[test]
    fn agent_disk_square8_chebyshev() {
        // On an 8-connected grid, AgentDisk radius=1 uses Chebyshev distance.
        // Bounding box is 3x3 = 9, Chebyshev disk radius=1 = full 3x3 → 9 cells.
        let space = Square8::new(10, 10, EdgeBehavior::Absorb).unwrap();
        let data: Vec<f32> = (0..100).map(|x| x as f32).collect();
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::AgentDisk { radius: 1 },
                pool: None,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();
        assert_eq!(result.output_len, 9);

        let center: Coord = smallvec::smallvec![5, 5];
        let mut output = vec![0.0f32; 9];
        let mut mask = vec![0u8; 9];
        result
            .plan
            .execute_agents(&snap, &space, &[center], None, &mut output, &mut mask)
            .unwrap();

        // Chebyshev distance <= 1 covers full 3x3 = 9 cells (all corners included).
        let valid_count = mask.iter().filter(|&&v| v == 1).count();
        assert_eq!(valid_count, 9, "Chebyshev disk radius=1 = full 3x3");
    }

    #[test]
    fn compile_rejects_inverted_normalize_range() {
        let space = square4_space();
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Normalize {
                    min: 10.0,
                    max: 5.0,
                },
                dtype: ObsDtype::F32,
            }],
        };
        let err = ObsPlan::compile(&spec, &space).unwrap_err();
        assert!(matches!(err, ObsError::InvalidObsSpec { .. }));
    }

    #[test]
    fn compile_rejects_nan_normalize() {
        let space = square4_space();
        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: ObsRegion::Fixed(RegionSpec::All),
                pool: None,
                transform: ObsTransform::Normalize {
                    min: f64::NAN,
                    max: 1.0,
                },
                dtype: ObsDtype::F32,
            }],
        };
        assert!(ObsPlan::compile(&spec, &space).is_err());
    }
}
