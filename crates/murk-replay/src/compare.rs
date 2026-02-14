//! Snapshot comparison and replay verification framework.
//!
//! Provides hash-first comparison (fast path) with per-field byte-exact
//! fallback on mismatch, plus a streaming replay-and-compare driver.

use murk_core::command::Command;
use murk_core::id::FieldId;
use murk_core::traits::SnapshotAccess;

use crate::codec::deserialize_command;
use crate::error::ReplayError;
use crate::hash::snapshot_hash;
use crate::reader::ReplayReader;

/// A single field-level divergence between recorded and replayed state.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldDivergence {
    /// Which field diverged.
    pub field_id: u32,
    /// Byte offset within the field's f32 array where divergence starts.
    pub byte_offset: usize,
    /// Cell index (byte_offset / 4) within the field.
    pub cell_index: usize,
    /// The recorded value (from the original run).
    pub recorded_value: f32,
    /// The replayed value (from the current run).
    pub replayed_value: f32,
}

/// Report of all divergences found at a single tick.
#[derive(Clone, Debug)]
pub struct DivergenceReport {
    /// The tick at which divergence was detected.
    pub tick_id: u64,
    /// All field-level divergences found at this tick.
    pub divergences: Vec<FieldDivergence>,
}

/// Compare a replayed snapshot against a recorded hash.
///
/// Fast path: compute the hash and compare. If hashes match, returns `Ok(None)`.
/// On mismatch, falls back to per-field byte-exact comparison to identify
/// exactly which fields and cells diverged.
///
/// The `recorded_snapshot` parameter provides field data from the original run
/// for byte-exact comparison. If `None` is passed, only hash comparison is done.
pub fn compare_snapshot(
    replayed: &dyn SnapshotAccess,
    recorded_hash: u64,
    field_count: u32,
    tick_id: u64,
    recorded_snapshot: Option<&dyn SnapshotAccess>,
) -> Result<Option<DivergenceReport>, ReplayError> {
    let replayed_hash = snapshot_hash(replayed, field_count);

    if replayed_hash == recorded_hash {
        return Ok(None);
    }

    // Hash mismatch — do per-field comparison if we have the recorded data
    let mut divergences = Vec::new();

    if let Some(recorded) = recorded_snapshot {
        for field_idx in 0..field_count {
            let fid = FieldId(field_idx);
            let recorded_data = recorded.read_field(fid);
            let replayed_data = replayed.read_field(fid);

            match (recorded_data, replayed_data) {
                (Some(rec), Some(rep)) => {
                    for (i, (&rv, &pv)) in rec.iter().zip(rep.iter()).enumerate() {
                        if rv.to_bits() != pv.to_bits() {
                            divergences.push(FieldDivergence {
                                field_id: field_idx,
                                byte_offset: i * 4,
                                cell_index: i,
                                recorded_value: rv,
                                replayed_value: pv,
                            });
                        }
                    }
                    // Length mismatch
                    if rec.len() != rep.len() {
                        divergences.push(FieldDivergence {
                            field_id: field_idx,
                            byte_offset: rec.len().min(rep.len()) * 4,
                            cell_index: rec.len().min(rep.len()),
                            recorded_value: 0.0,
                            replayed_value: 0.0,
                        });
                    }
                }
                (Some(_), None) | (None, Some(_)) => {
                    divergences.push(FieldDivergence {
                        field_id: field_idx,
                        byte_offset: 0,
                        cell_index: 0,
                        recorded_value: 0.0,
                        replayed_value: 0.0,
                    });
                }
                (None, None) => {}
            }
        }
    }

    Ok(Some(DivergenceReport {
        tick_id,
        divergences,
    }))
}

/// Replay a recorded session through a caller-provided step function
/// and compare snapshot hashes at every tick.
///
/// The `step_fn` closure receives the deserialized commands for each tick,
/// steps the simulation, and returns the snapshot hash of the resulting state.
/// This closure-based API avoids `Snapshot<'_>` lifetime conflicts with
/// world ownership.
///
/// Returns `Ok(None)` if all ticks match, or `Ok(Some(report))` at the
/// first divergence. The report contains only the tick ID and hash mismatch
/// (no per-field detail, since the closure only returns a hash).
pub fn replay_and_compare<R: std::io::Read>(
    mut reader: ReplayReader<R>,
    step_fn: &mut dyn FnMut(Vec<Command>) -> Result<u64, ReplayError>,
) -> Result<Option<DivergenceReport>, ReplayError> {
    while let Some(frame) = reader.next_frame()? {
        // Deserialize commands
        let commands: Vec<Command> = frame
            .commands
            .iter()
            .map(deserialize_command)
            .collect::<Result<Vec<_>, _>>()?;

        // Step the simulation and get the snapshot hash
        let replayed_hash = step_fn(commands)?;

        // Compare hashes
        if replayed_hash != frame.snapshot_hash {
            return Ok(Some(DivergenceReport {
                tick_id: frame.tick_id,
                divergences: vec![],
            }));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::snapshot_hash;
    use murk_core::id::{ParameterVersion, TickId, WorldGenerationId};
    use murk_test_utils::MockSnapshot;

    fn make_snapshot(field_data: Vec<(u32, Vec<f32>)>) -> MockSnapshot {
        let mut snap = MockSnapshot::new(TickId(1), WorldGenerationId(1), ParameterVersion(0));
        for (fid, data) in field_data {
            snap.set_field(FieldId(fid), data);
        }
        snap
    }

    #[test]
    fn matching_snapshots_return_none() {
        let snap = make_snapshot(vec![(0, vec![1.0, 2.0, 3.0])]);
        let hash = snapshot_hash(&snap, 1);
        let result = compare_snapshot(&snap, hash, 1, 1, Some(&snap)).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn divergent_snapshots_return_report() {
        let recorded = make_snapshot(vec![(0, vec![1.0, 2.0, 3.0])]);
        let replayed = make_snapshot(vec![(0, vec![1.0, 9.0, 3.0])]);
        let recorded_hash = snapshot_hash(&recorded, 1);

        let result = compare_snapshot(&replayed, recorded_hash, 1, 42, Some(&recorded)).unwrap();
        let report = result.unwrap();
        assert_eq!(report.tick_id, 42);
        assert_eq!(report.divergences.len(), 1);
        assert_eq!(report.divergences[0].cell_index, 1);
        assert_eq!(report.divergences[0].recorded_value, 2.0);
        assert_eq!(report.divergences[0].replayed_value, 9.0);
    }

    #[test]
    fn hash_only_comparison_no_recorded_data() {
        let snap = make_snapshot(vec![(0, vec![1.0, 2.0])]);
        let wrong_hash = 0xDEAD;

        let result = compare_snapshot(&snap, wrong_hash, 1, 1, None).unwrap();
        let report = result.unwrap();
        assert_eq!(report.tick_id, 1);
        assert!(report.divergences.is_empty()); // no detail without recorded data
    }

    #[test]
    fn replay_and_compare_all_match() {
        use crate::types::*;
        use crate::writer::ReplayWriter;

        let meta = BuildMetadata {
            toolchain: "t".into(),
            target_triple: "t".into(),
            murk_version: "0.1.0".into(),
            compile_flags: "t".into(),
        };
        let init = InitDescriptor {
            seed: 42,
            config_hash: 0,
            field_count: 1,
            cell_count: 5,
            space_descriptor: vec![],
        };

        let mut buf = Vec::new();
        {
            let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
            for tick in 1..=3u64 {
                let snap = make_snapshot(vec![(0, vec![tick as f32; 5])]);
                writer.write_frame(tick, &[], &snap).unwrap();
            }
        }

        let reader = ReplayReader::open(buf.as_slice()).unwrap();
        let field_count = reader.init_descriptor().field_count;
        let mut tick = 0u64;
        let result = replay_and_compare(reader, &mut |_commands| {
            tick += 1;
            let snap = make_snapshot(vec![(0, vec![tick as f32; 5])]);
            Ok(snapshot_hash(&snap, field_count))
        })
        .unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn replay_and_compare_detects_divergence() {
        use crate::types::*;
        use crate::writer::ReplayWriter;

        let meta = BuildMetadata {
            toolchain: "t".into(),
            target_triple: "t".into(),
            murk_version: "0.1.0".into(),
            compile_flags: "t".into(),
        };
        let init = InitDescriptor {
            seed: 42,
            config_hash: 0,
            field_count: 1,
            cell_count: 5,
            space_descriptor: vec![],
        };

        let mut buf = Vec::new();
        {
            let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
            for tick in 1..=3u64 {
                let snap = make_snapshot(vec![(0, vec![tick as f32; 5])]);
                writer.write_frame(tick, &[], &snap).unwrap();
            }
        }

        let reader = ReplayReader::open(buf.as_slice()).unwrap();
        let field_count = reader.init_descriptor().field_count;
        let mut tick = 0u64;
        let result = replay_and_compare(reader, &mut |_commands| {
            tick += 1;
            // Diverge at tick 2
            let val = if tick == 2 { 999.0 } else { tick as f32 };
            let snap = make_snapshot(vec![(0, vec![val; 5])]);
            Ok(snapshot_hash(&snap, field_count))
        })
        .unwrap();

        let report = result.unwrap();
        assert_eq!(report.tick_id, 2);
    }
}
