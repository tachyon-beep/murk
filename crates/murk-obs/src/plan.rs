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
use murk_core::{Coord, FieldId, SnapshotAccess, WorldGenerationId};
use murk_space::Space;

use crate::metadata::ObsMetadata;
use crate::spec::{ObsDtype, ObsSpec, ObsTransform};

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

/// Compiled observation plan — the "Simple plan class".
///
/// Holds precomputed gather indices and transform parameters so that
/// [`execute`](Self::execute) is a branch-free gather loop with zero
/// spatial computation at runtime.
#[derive(Debug)]
pub struct ObsPlan {
    entries: Vec<CompiledEntry>,
    /// Total output elements across all entries.
    output_len: usize,
    /// Total mask bytes across all entries.
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

impl ObsPlan {
    /// Compile an [`ObsSpec`] against a [`Space`].
    ///
    /// Validates entries, compiles region plans, pre-computes gather
    /// indices via the space's canonical ordering, and computes output
    /// layout. Returns an error if the spec is empty, a region fails
    /// to compile, or coverage is below the 0.35 threshold.
    pub fn compile(spec: &ObsSpec, space: &dyn Space) -> Result<ObsPlanResult, ObsError> {
        if spec.entries.is_empty() {
            return Err(ObsError::InvalidObsSpec {
                reason: "ObsSpec has no entries".into(),
            });
        }

        // Build coord → flat field index lookup from canonical ordering.
        // This is O(cell_count) and done once per compile.
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
            let region_plan =
                space
                    .compile_region(&entry.region)
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

            // Pre-compute gather operations: for each coord in the region,
            // resolve its flat field data index via the canonical ordering map.
            let mut gather_ops = Vec::with_capacity(region_plan.coords.len());
            for (coord_idx, coord) in region_plan.coords.iter().enumerate() {
                let field_data_idx =
                    *coord_to_field_idx
                        .get(coord)
                        .ok_or_else(|| ObsError::InvalidObsSpec {
                            reason: format!(
                                "entry {i}: coord {coord:?} not in canonical ordering"
                            ),
                        })?;
                let tensor_idx = region_plan.tensor_indices[coord_idx];
                gather_ops.push(GatherOp {
                    field_data_idx,
                    tensor_idx,
                });
            }

            let element_count = region_plan.bounding_shape.total_elements();
            let shape = match &region_plan.bounding_shape {
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
                valid_mask: region_plan.valid_mask,
                valid_ratio: ratio,
            });

            output_offset += element_count;
            mask_offset += element_count;
        }

        let plan = ObsPlan {
            entries,
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
    /// Returns [`ObsMetadata`] on success.
    ///
    /// # Errors
    ///
    /// - [`ObsError::PlanInvalidated`] if bound and generation mismatches.
    /// - [`ObsError::ExecutionFailed`] if a field is missing from the snapshot.
    pub fn execute(
        &self,
        snapshot: &dyn SnapshotAccess,
        output: &mut [f32],
        mask: &mut [u8],
    ) -> Result<ObsMetadata, ObsError> {
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
                reason: format!(
                    "mask buffer too small: {} < {}",
                    mask.len(),
                    self.mask_len
                ),
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

        for entry in &self.entries {
            let field_data =
                snapshot
                    .read_field(entry.field_id)
                    .ok_or_else(|| ObsError::ExecutionFailed {
                        reason: format!("field {:?} not in snapshot", entry.field_id),
                    })?;

            let out_slice =
                &mut output[entry.output_offset..entry.output_offset + entry.element_count];
            let mask_slice =
                &mut mask[entry.mask_offset..entry.mask_offset + entry.element_count];

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

        Ok(ObsMetadata {
            tick_id: snapshot.tick_id(),
            age_ticks: 0,
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
        output: &mut [f32],
        mask: &mut [u8],
    ) -> Result<Vec<ObsMetadata>, ObsError> {
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
            let meta = self.execute(*snap, out_slice, mask_slice)?;
            metadata.push(meta);
        }
        Ok(metadata)
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
    use crate::spec::{ObsDtype, ObsEntry, ObsSpec, ObsTransform};
    use murk_core::{FieldId, ParameterVersion, TickId, WorldGenerationId};
    use murk_space::{EdgeBehavior, RegionSpec, Square4};
    use murk_test_utils::MockSnapshot;

    fn square4_space() -> Square4 {
        Square4::new(3, 3, EdgeBehavior::Absorb).unwrap()
    }

    fn snapshot_with_field(field: FieldId, data: Vec<f32>) -> MockSnapshot {
        let mut snap = MockSnapshot::new(
            TickId(5),
            WorldGenerationId(1),
            ParameterVersion(0),
        );
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
                region: RegionSpec::All,
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
                region: RegionSpec::Rect {
                    min: smallvec::smallvec![1, 1],
                    max: smallvec::smallvec![2, 3],
                },
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
                    region: RegionSpec::All,
                    transform: ObsTransform::Identity,
                    dtype: ObsDtype::F32,
                },
                ObsEntry {
                    field_id: FieldId(1),
                    region: RegionSpec::All,
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
                region: RegionSpec::Coords(vec![smallvec::smallvec![99, 99]]),
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
                region: RegionSpec::All,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let meta = result.plan.execute(&snap, &mut output, &mut mask).unwrap();

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
                region: RegionSpec::All,
                transform: ObsTransform::Normalize {
                    min: 0.0,
                    max: 8.0,
                },
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result.plan.execute(&snap, &mut output, &mut mask).unwrap();

        // Each value x should be x/8.
        for (i, &v) in output.iter().enumerate() {
            let expected = i as f32 / 8.0;
            assert!((v - expected).abs() < 1e-6, "output[{i}] = {v}, expected {expected}");
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
                region: RegionSpec::All,
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
        result.plan.execute(&snap, &mut output, &mut mask).unwrap();

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
                region: RegionSpec::All,
                transform: ObsTransform::Normalize {
                    min: 5.0,
                    max: 5.0,
                },
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![-1.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result.plan.execute(&snap, &mut output, &mut mask).unwrap();

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
                region: RegionSpec::Rect {
                    min: smallvec::smallvec![1, 1],
                    max: smallvec::smallvec![2, 2],
                },
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();
        assert_eq!(result.output_len, 4); // 2x2

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result.plan.execute(&snap, &mut output, &mut mask).unwrap();

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
                    region: RegionSpec::All,
                    transform: ObsTransform::Identity,
                    dtype: ObsDtype::F32,
                },
                ObsEntry {
                    field_id: FieldId(1),
                    region: RegionSpec::All,
                    transform: ObsTransform::Identity,
                    dtype: ObsDtype::F32,
                },
            ],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();
        assert_eq!(result.output_len, 18);

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result.plan.execute(&snap, &mut output, &mut mask).unwrap();

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
                region: RegionSpec::All,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let err = result.plan.execute(&snap, &mut output, &mut mask).unwrap_err();
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
                region: RegionSpec::All,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; 4]; // too small
        let mut mask = vec![0u8; result.mask_len];
        let err = result.plan.execute(&snap, &mut output, &mut mask).unwrap_err();
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
                region: RegionSpec::All,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let meta = result.plan.execute(&snap, &mut output, &mut mask).unwrap();

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
                region: RegionSpec::All,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        // Compile bound to generation 99, but snapshot is generation 1.
        let result =
            ObsPlan::compile_bound(&spec, &space, WorldGenerationId(99)).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let err = result.plan.execute(&snap, &mut output, &mut mask).unwrap_err();
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
                region: RegionSpec::All,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result =
            ObsPlan::compile_bound(&spec, &space, WorldGenerationId(1)).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result.plan.execute(&snap, &mut output, &mut mask).unwrap();
    }

    #[test]
    fn unbound_plan_ignores_generation() {
        let space = square4_space();
        let data: Vec<f32> = vec![1.0; 9];
        let snap = snapshot_with_field(FieldId(0), data);

        let spec = ObsSpec {
            entries: vec![ObsEntry {
                field_id: FieldId(0),
                region: RegionSpec::All,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        // Unbound plan — no generation check.
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        result.plan.execute(&snap, &mut output, &mut mask).unwrap();
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
                region: RegionSpec::All,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let meta = result.plan.execute(&snap, &mut output, &mut mask).unwrap();

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
                region: RegionSpec::All,
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
            .execute(&snap, &mut out_single, &mut mask_single)
            .unwrap();

        // Batch N=1.
        let mut out_batch = vec![0.0f32; result.output_len];
        let mut mask_batch = vec![0u8; result.mask_len];
        let snap_ref: &dyn SnapshotAccess = &snap;
        let meta_batch = result
            .plan
            .execute_batch(&[snap_ref], &mut out_batch, &mut mask_batch)
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
                region: RegionSpec::All,
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
            .execute_batch(&snaps, &mut output, &mut mask)
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
                region: RegionSpec::All,
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
            .execute_batch(&snaps, &mut output, &mut mask)
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
                region: RegionSpec::All,
                transform: ObsTransform::Identity,
                dtype: ObsDtype::F32,
            }],
        };
        let result = ObsPlan::compile(&spec, &space).unwrap();

        // Snapshot field has only 4 elements, but plan expects 9.
        let snap = snapshot_with_field(FieldId(0), vec![1.0; 4]);
        let mut output = vec![0.0f32; result.output_len];
        let mut mask = vec![0u8; result.mask_len];
        let err = result.plan.execute(&snap, &mut output, &mut mask).unwrap_err();
        assert!(matches!(err, ObsError::ExecutionFailed { .. }));
    }
}
