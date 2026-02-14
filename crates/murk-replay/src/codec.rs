//! Binary encode/decode for the replay format.
//!
//! All integers are little-endian. Strings and byte arrays are length-prefixed
//! with a `u32` length. The format is intentionally simple — no compression,
//! no alignment padding, no self-describing schema.

use std::io::{Read, Write};

use murk_core::command::{Command, CommandPayload};
use murk_core::id::{Coord, FieldId, ParameterKey, TickId};

use crate::error::ReplayError;
use crate::types::*;
use crate::{FORMAT_VERSION, MAGIC};

// ── Primitive writers ───────────────────────────────────────────

/// Write a single byte.
pub fn write_u8(w: &mut dyn Write, v: u8) -> Result<(), ReplayError> {
    w.write_all(&[v])?;
    Ok(())
}

/// Write a little-endian u32.
pub fn write_u32_le(w: &mut dyn Write, v: u32) -> Result<(), ReplayError> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

/// Write a little-endian u64.
pub fn write_u64_le(w: &mut dyn Write, v: u64) -> Result<(), ReplayError> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

/// Write a little-endian f32.
pub fn write_f32_le(w: &mut dyn Write, v: f32) -> Result<(), ReplayError> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

/// Write a little-endian f64.
pub fn write_f64_le(w: &mut dyn Write, v: f64) -> Result<(), ReplayError> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

/// Write a little-endian i32.
pub fn write_i32_le(w: &mut dyn Write, v: i32) -> Result<(), ReplayError> {
    w.write_all(&v.to_le_bytes())?;
    Ok(())
}

/// Write a length-prefixed UTF-8 string (u32 length + bytes).
pub fn write_length_prefixed_str(w: &mut dyn Write, s: &str) -> Result<(), ReplayError> {
    write_u32_le(w, s.len() as u32)?;
    w.write_all(s.as_bytes())?;
    Ok(())
}

/// Write a length-prefixed byte array (u32 length + bytes).
pub fn write_length_prefixed_bytes(w: &mut dyn Write, b: &[u8]) -> Result<(), ReplayError> {
    write_u32_le(w, b.len() as u32)?;
    w.write_all(b)?;
    Ok(())
}

// ── Primitive readers ───────────────────────────────────────────

/// Read a single byte.
pub fn read_u8(r: &mut dyn Read) -> Result<u8, ReplayError> {
    let mut buf = [0u8; 1];
    r.read_exact(&mut buf)?;
    Ok(buf[0])
}

/// Read a little-endian u32.
pub fn read_u32_le(r: &mut dyn Read) -> Result<u32, ReplayError> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(u32::from_le_bytes(buf))
}

/// Read a little-endian u64.
pub fn read_u64_le(r: &mut dyn Read) -> Result<u64, ReplayError> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;
    Ok(u64::from_le_bytes(buf))
}

/// Read a little-endian f32.
pub fn read_f32_le(r: &mut dyn Read) -> Result<f32, ReplayError> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(f32::from_le_bytes(buf))
}

/// Read a little-endian f64.
pub fn read_f64_le(r: &mut dyn Read) -> Result<f64, ReplayError> {
    let mut buf = [0u8; 8];
    r.read_exact(&mut buf)?;
    Ok(f64::from_le_bytes(buf))
}

/// Read a little-endian i32.
pub fn read_i32_le(r: &mut dyn Read) -> Result<i32, ReplayError> {
    let mut buf = [0u8; 4];
    r.read_exact(&mut buf)?;
    Ok(i32::from_le_bytes(buf))
}

/// Read a length-prefixed UTF-8 string.
pub fn read_length_prefixed_str(r: &mut dyn Read) -> Result<String, ReplayError> {
    let len = read_u32_le(r)? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    String::from_utf8(buf).map_err(|e| ReplayError::MalformedFrame {
        detail: format!("invalid UTF-8 string: {e}"),
    })
}

/// Read a length-prefixed byte array.
pub fn read_length_prefixed_bytes(r: &mut dyn Read) -> Result<Vec<u8>, ReplayError> {
    let len = read_u32_le(r)? as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

// ── Header encode/decode ────────────────────────────────────────

/// Encode the replay file header (magic, version, build metadata, init descriptor).
pub fn encode_header(
    w: &mut dyn Write,
    meta: &BuildMetadata,
    init: &InitDescriptor,
) -> Result<(), ReplayError> {
    // Magic bytes
    w.write_all(&MAGIC)?;
    // Format version
    write_u8(w, FORMAT_VERSION)?;

    // Build metadata
    write_length_prefixed_str(w, &meta.toolchain)?;
    write_length_prefixed_str(w, &meta.target_triple)?;
    write_length_prefixed_str(w, &meta.murk_version)?;
    write_length_prefixed_str(w, &meta.compile_flags)?;

    // Init descriptor
    write_u64_le(w, init.seed)?;
    write_u64_le(w, init.config_hash)?;
    write_u32_le(w, init.field_count)?;
    write_u64_le(w, init.cell_count)?;
    write_length_prefixed_bytes(w, &init.space_descriptor)?;

    Ok(())
}

/// Decode and validate the replay file header.
///
/// Returns the build metadata and init descriptor on success.
pub fn decode_header(r: &mut dyn Read) -> Result<(BuildMetadata, InitDescriptor), ReplayError> {
    // Magic bytes
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic)?;
    if magic != MAGIC {
        return Err(ReplayError::InvalidMagic);
    }

    // Format version
    let version = read_u8(r)?;
    if version != FORMAT_VERSION {
        return Err(ReplayError::UnsupportedVersion { found: version });
    }

    // Build metadata
    let meta = BuildMetadata {
        toolchain: read_length_prefixed_str(r)?,
        target_triple: read_length_prefixed_str(r)?,
        murk_version: read_length_prefixed_str(r)?,
        compile_flags: read_length_prefixed_str(r)?,
    };

    // Init descriptor
    let init = InitDescriptor {
        seed: read_u64_le(r)?,
        config_hash: read_u64_le(r)?,
        field_count: read_u32_le(r)?,
        cell_count: read_u64_le(r)?,
        space_descriptor: read_length_prefixed_bytes(r)?,
    };

    Ok((meta, init))
}

// ── Frame encode/decode ─────────────────────────────────────────

/// Encode a single replay frame.
pub fn encode_frame(w: &mut dyn Write, frame: &Frame) -> Result<(), ReplayError> {
    write_u64_le(w, frame.tick_id)?;
    write_u32_le(w, frame.commands.len() as u32)?;

    for cmd in &frame.commands {
        write_u8(w, cmd.payload_type)?;
        write_length_prefixed_bytes(w, &cmd.payload)?;
        write_u8(w, cmd.priority_class)?;
        // Presence flag + value for optional source_id
        match cmd.source_id {
            Some(id) => {
                write_u8(w, 1)?;
                write_u64_le(w, id)?;
            }
            None => write_u8(w, 0)?,
        }
        // Presence flag + value for optional source_seq
        match cmd.source_seq {
            Some(seq) => {
                write_u8(w, 1)?;
                write_u64_le(w, seq)?;
            }
            None => write_u8(w, 0)?,
        }
    }

    write_u64_le(w, frame.snapshot_hash)?;
    Ok(())
}

/// Decode a single replay frame.
///
/// Returns `Ok(None)` on clean EOF (no bytes available), `Ok(Some(frame))`
/// on success, or an error on truncated/corrupt data.
pub fn decode_frame(r: &mut dyn Read) -> Result<Option<Frame>, ReplayError> {
    // Read the tick_id header byte-by-byte to distinguish clean EOF
    // (zero bytes available) from truncation (1-7 bytes before EOF).
    let mut tick_buf = [0u8; 8];
    let mut filled = 0;
    while filled < 8 {
        match r.read(&mut tick_buf[filled..]) {
            Ok(0) => {
                if filled == 0 {
                    // Clean EOF — no more frames.
                    return Ok(None);
                }
                // Partial tick header — truncated/corrupt file.
                return Err(ReplayError::MalformedFrame {
                    detail: format!(
                        "truncated frame header: got {filled} of 8 bytes for tick_id"
                    ),
                });
            }
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(ReplayError::Io(e)),
        }
    }
    let tick_id = u64::from_le_bytes(tick_buf);

    let command_count = read_u32_le(r)? as usize;
    let mut commands = Vec::with_capacity(command_count);

    for _ in 0..command_count {
        let payload_type = read_u8(r)?;
        let payload = read_length_prefixed_bytes(r)?;
        let priority_class = read_u8(r)?;
        // Read optional source_id (presence flag + value)
        let source_id = match read_u8(r)? {
            0 => None,
            1 => Some(read_u64_le(r)?),
            flag => {
                return Err(ReplayError::MalformedFrame {
                    detail: format!("invalid source_id presence flag: {flag}"),
                })
            }
        };
        // Read optional source_seq (presence flag + value)
        let source_seq = match read_u8(r)? {
            0 => None,
            1 => Some(read_u64_le(r)?),
            flag => {
                return Err(ReplayError::MalformedFrame {
                    detail: format!("invalid source_seq presence flag: {flag}"),
                })
            }
        };

        commands.push(SerializedCommand {
            payload_type,
            payload,
            priority_class,
            source_id,
            source_seq,
        });
    }

    let snapshot_hash = read_u64_le(r)?;

    Ok(Some(Frame {
        tick_id,
        commands,
        snapshot_hash,
    }))
}

// ── Command serialization ───────────────────────────────────────

/// Serialize a `Coord` (SmallVec<[i32; 4]>) as a length-prefixed i32 array.
fn serialize_coord(buf: &mut Vec<u8>, coord: &Coord) {
    let len = coord.len() as u32;
    buf.extend_from_slice(&len.to_le_bytes());
    for &v in coord.iter() {
        buf.extend_from_slice(&v.to_le_bytes());
    }
}

/// Deserialize a `Coord` from a byte slice, advancing the offset.
fn deserialize_coord(data: &[u8], offset: &mut usize) -> Result<Coord, ReplayError> {
    if *offset + 4 > data.len() {
        return Err(ReplayError::MalformedFrame {
            detail: "truncated coord length".into(),
        });
    }
    let len = u32::from_le_bytes(data[*offset..*offset + 4].try_into().unwrap()) as usize;
    *offset += 4;

    let byte_len = len * 4;
    if *offset + byte_len > data.len() {
        return Err(ReplayError::MalformedFrame {
            detail: "truncated coord data".into(),
        });
    }

    let mut coord = Coord::new();
    for _ in 0..len {
        let v = i32::from_le_bytes(data[*offset..*offset + 4].try_into().unwrap());
        coord.push(v);
        *offset += 4;
    }
    Ok(coord)
}

/// Serialize a [`Command`] into a [`SerializedCommand`].
///
/// Only `payload`, `priority_class`, `source_id`, and `source_seq` are recorded.
/// `expires_after_tick` and `arrival_seq` are not serialized per spec.
pub fn serialize_command(cmd: &Command) -> SerializedCommand {
    let (payload_type, payload) = match &cmd.payload {
        CommandPayload::Move {
            entity_id,
            target_coord,
        } => {
            let mut buf = Vec::new();
            buf.extend_from_slice(&entity_id.to_le_bytes());
            serialize_coord(&mut buf, target_coord);
            (PAYLOAD_MOVE, buf)
        }
        CommandPayload::Spawn {
            coord,
            field_values,
        } => {
            let mut buf = Vec::new();
            serialize_coord(&mut buf, coord);
            buf.extend_from_slice(&(field_values.len() as u32).to_le_bytes());
            for (fid, val) in field_values {
                buf.extend_from_slice(&fid.0.to_le_bytes());
                buf.extend_from_slice(&val.to_le_bytes());
            }
            (PAYLOAD_SPAWN, buf)
        }
        CommandPayload::Despawn { entity_id } => {
            let buf = entity_id.to_le_bytes().to_vec();
            (PAYLOAD_DESPAWN, buf)
        }
        CommandPayload::SetField {
            coord,
            field_id,
            value,
        } => {
            let mut buf = Vec::new();
            serialize_coord(&mut buf, coord);
            buf.extend_from_slice(&field_id.0.to_le_bytes());
            buf.extend_from_slice(&value.to_le_bytes());
            (PAYLOAD_SET_FIELD, buf)
        }
        CommandPayload::Custom { type_id, data } => {
            let mut buf = Vec::new();
            buf.extend_from_slice(&type_id.to_le_bytes());
            buf.extend_from_slice(&(data.len() as u32).to_le_bytes());
            buf.extend_from_slice(data);
            (PAYLOAD_CUSTOM, buf)
        }
        CommandPayload::SetParameter { key, value } => {
            let mut buf = Vec::new();
            buf.extend_from_slice(&key.0.to_le_bytes());
            buf.extend_from_slice(&value.to_le_bytes());
            (PAYLOAD_SET_PARAMETER, buf)
        }
        CommandPayload::SetParameterBatch { params } => {
            let mut buf = Vec::new();
            buf.extend_from_slice(&(params.len() as u32).to_le_bytes());
            for (key, value) in params {
                buf.extend_from_slice(&key.0.to_le_bytes());
                buf.extend_from_slice(&value.to_le_bytes());
            }
            (PAYLOAD_SET_PARAMETER_BATCH, buf)
        }
    };

    SerializedCommand {
        payload_type,
        payload,
        priority_class: cmd.priority_class,
        source_id: cmd.source_id,
        source_seq: cmd.source_seq,
    }
}

/// Deserialize a [`SerializedCommand`] back into a [`Command`].
///
/// Sets `expires_after_tick` to `TickId(u64::MAX)` and `arrival_seq` to `0`
/// since those fields are not recorded in the replay.
pub fn deserialize_command(sc: &SerializedCommand) -> Result<Command, ReplayError> {
    let data = &sc.payload;
    let payload = match sc.payload_type {
        PAYLOAD_MOVE => {
            if data.len() < 8 {
                return Err(ReplayError::MalformedFrame {
                    detail: "truncated Move payload".into(),
                });
            }
            let entity_id = u64::from_le_bytes(data[0..8].try_into().unwrap());
            let mut offset = 8;
            let target_coord = deserialize_coord(data, &mut offset)?;
            CommandPayload::Move {
                entity_id,
                target_coord,
            }
        }
        PAYLOAD_SPAWN => {
            let mut offset = 0;
            let coord = deserialize_coord(data, &mut offset)?;
            if offset + 4 > data.len() {
                return Err(ReplayError::MalformedFrame {
                    detail: "truncated Spawn field_values count".into(),
                });
            }
            let count =
                u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()) as usize;
            offset += 4;
            let mut field_values = Vec::with_capacity(count);
            for _ in 0..count {
                if offset + 8 > data.len() {
                    return Err(ReplayError::MalformedFrame {
                        detail: "truncated Spawn field_values entry".into(),
                    });
                }
                let fid = u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
                offset += 4;
                let val = f32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
                offset += 4;
                field_values.push((FieldId(fid), val));
            }
            CommandPayload::Spawn {
                coord,
                field_values,
            }
        }
        PAYLOAD_DESPAWN => {
            if data.len() < 8 {
                return Err(ReplayError::MalformedFrame {
                    detail: "truncated Despawn payload".into(),
                });
            }
            let entity_id = u64::from_le_bytes(data[0..8].try_into().unwrap());
            CommandPayload::Despawn { entity_id }
        }
        PAYLOAD_SET_FIELD => {
            let mut offset = 0;
            let coord = deserialize_coord(data, &mut offset)?;
            if offset + 8 > data.len() {
                return Err(ReplayError::MalformedFrame {
                    detail: "truncated SetField payload".into(),
                });
            }
            let field_id =
                FieldId(u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()));
            offset += 4;
            let value = f32::from_le_bytes(data[offset..offset + 4].try_into().unwrap());
            CommandPayload::SetField {
                coord,
                field_id,
                value,
            }
        }
        PAYLOAD_CUSTOM => {
            if data.len() < 8 {
                return Err(ReplayError::MalformedFrame {
                    detail: "truncated Custom payload".into(),
                });
            }
            let type_id = u32::from_le_bytes(data[0..4].try_into().unwrap());
            let data_len = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
            if data.len() < 8 + data_len {
                return Err(ReplayError::MalformedFrame {
                    detail: "truncated Custom data".into(),
                });
            }
            CommandPayload::Custom {
                type_id,
                data: data[8..8 + data_len].to_vec(),
            }
        }
        PAYLOAD_SET_PARAMETER => {
            if data.len() < 12 {
                return Err(ReplayError::MalformedFrame {
                    detail: "truncated SetParameter payload".into(),
                });
            }
            let key = ParameterKey(u32::from_le_bytes(data[0..4].try_into().unwrap()));
            let value = f64::from_le_bytes(data[4..12].try_into().unwrap());
            CommandPayload::SetParameter { key, value }
        }
        PAYLOAD_SET_PARAMETER_BATCH => {
            if data.len() < 4 {
                return Err(ReplayError::MalformedFrame {
                    detail: "truncated SetParameterBatch count".into(),
                });
            }
            let count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
            let mut offset = 4;
            let mut params = Vec::with_capacity(count);
            for _ in 0..count {
                if offset + 12 > data.len() {
                    return Err(ReplayError::MalformedFrame {
                        detail: "truncated SetParameterBatch entry".into(),
                    });
                }
                let key =
                    ParameterKey(u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap()));
                offset += 4;
                let value = f64::from_le_bytes(data[offset..offset + 8].try_into().unwrap());
                offset += 8;
                params.push((key, value));
            }
            CommandPayload::SetParameterBatch { params }
        }
        tag => return Err(ReplayError::UnknownPayloadType { tag }),
    };

    Ok(Command {
        payload,
        expires_after_tick: TickId(u64::MAX),
        source_id: sc.source_id,
        source_seq: sc.source_seq,
        priority_class: sc.priority_class,
        arrival_seq: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use murk_core::Coord;

    // ── Proptest strategies ─────────────────────────────────────

    fn arb_coord() -> impl Strategy<Value = Coord> {
        prop::collection::vec(-1000i32..1000, 1..=4).prop_map(|v| Coord::from_vec(v))
    }

    /// Strategy for optional u64 that includes None, Some(0), and arbitrary values.
    fn arb_opt_u64() -> impl Strategy<Value = Option<u64>> {
        prop_oneof![
            Just(None),
            Just(Some(0u64)),
            any::<u64>().prop_map(Some),
        ]
    }

    fn arb_command() -> impl Strategy<Value = Command> {
        prop_oneof![
            // Move
            (any::<u64>(), arb_coord(), arb_opt_u64(), arb_opt_u64()).prop_map(
                |(eid, coord, sid, sseq)| Command {
                    payload: CommandPayload::Move {
                        entity_id: eid,
                        target_coord: coord,
                    },
                    expires_after_tick: TickId(u64::MAX),
                    source_id: sid,
                    source_seq: sseq,
                    priority_class: 1,
                    arrival_seq: 0,
                }
            ),
            // Spawn
            (
                arb_coord(),
                prop::collection::vec((0u32..10, any::<f32>()), 0..4),
                arb_opt_u64(),
                arb_opt_u64(),
            )
                .prop_map(|(coord, fvs, sid, sseq)| Command {
                    payload: CommandPayload::Spawn {
                        coord,
                        field_values: fvs.into_iter().map(|(f, v)| (FieldId(f), v)).collect(),
                    },
                    expires_after_tick: TickId(u64::MAX),
                    source_id: sid,
                    source_seq: sseq,
                    priority_class: 0,
                    arrival_seq: 0,
                }),
            // Despawn
            (any::<u64>(), arb_opt_u64(), arb_opt_u64()).prop_map(|(eid, sid, sseq)| Command {
                payload: CommandPayload::Despawn { entity_id: eid },
                expires_after_tick: TickId(u64::MAX),
                source_id: sid,
                source_seq: sseq,
                priority_class: 1,
                arrival_seq: 0,
            }),
            // SetField
            (arb_coord(), 0u32..10, any::<f32>(), arb_opt_u64(), arb_opt_u64()).prop_map(
                |(coord, fid, val, sid, sseq)| Command {
                    payload: CommandPayload::SetField {
                        coord,
                        field_id: FieldId(fid),
                        value: val,
                    },
                    expires_after_tick: TickId(u64::MAX),
                    source_id: sid,
                    source_seq: sseq,
                    priority_class: 1,
                    arrival_seq: 0,
                }
            ),
            // Custom
            (
                0u32..100,
                prop::collection::vec(any::<u8>(), 0..32),
                arb_opt_u64(),
                arb_opt_u64(),
            )
                .prop_map(|(tid, data, sid, sseq)| {
                    Command {
                        payload: CommandPayload::Custom { type_id: tid, data },
                        expires_after_tick: TickId(u64::MAX),
                        source_id: sid,
                        source_seq: sseq,
                        priority_class: 1,
                        arrival_seq: 0,
                    }
                }),
            // SetParameter
            (0u32..10, any::<f64>(), arb_opt_u64(), arb_opt_u64()).prop_map(
                |(k, v, sid, sseq)| Command {
                    payload: CommandPayload::SetParameter {
                        key: ParameterKey(k),
                        value: v,
                    },
                    expires_after_tick: TickId(u64::MAX),
                    source_id: sid,
                    source_seq: sseq,
                    priority_class: 1,
                    arrival_seq: 0,
                }
            ),
            // SetParameterBatch
            (
                prop::collection::vec((0u32..10, any::<f64>()), 1..4),
                arb_opt_u64(),
                arb_opt_u64(),
            )
                .prop_map(|(params, sid, sseq)| Command {
                    payload: CommandPayload::SetParameterBatch {
                        params: params
                            .into_iter()
                            .map(|(k, v)| (ParameterKey(k), v))
                            .collect(),
                    },
                    expires_after_tick: TickId(u64::MAX),
                    source_id: sid,
                    source_seq: sseq,
                    priority_class: 1,
                    arrival_seq: 0,
                }),
        ]
    }

    // ── Primitive round-trip tests ──────────────────────────────

    proptest! {
        #[test]
        fn roundtrip_u8(v in any::<u8>()) {
            let mut buf = Vec::new();
            write_u8(&mut buf, v).unwrap();
            let got = read_u8(&mut buf.as_slice()).unwrap();
            prop_assert_eq!(v, got);
        }

        #[test]
        fn roundtrip_u32(v in any::<u32>()) {
            let mut buf = Vec::new();
            write_u32_le(&mut buf, v).unwrap();
            let got = read_u32_le(&mut buf.as_slice()).unwrap();
            prop_assert_eq!(v, got);
        }

        #[test]
        fn roundtrip_u64(v in any::<u64>()) {
            let mut buf = Vec::new();
            write_u64_le(&mut buf, v).unwrap();
            let got = read_u64_le(&mut buf.as_slice()).unwrap();
            prop_assert_eq!(v, got);
        }

        #[test]
        fn roundtrip_i32(v in any::<i32>()) {
            let mut buf = Vec::new();
            write_i32_le(&mut buf, v).unwrap();
            let got = read_i32_le(&mut buf.as_slice()).unwrap();
            prop_assert_eq!(v, got);
        }

        #[test]
        fn roundtrip_f32(v in any::<u32>()) {
            let f = f32::from_bits(v);
            let mut buf = Vec::new();
            write_f32_le(&mut buf, f).unwrap();
            let got = read_f32_le(&mut buf.as_slice()).unwrap();
            prop_assert_eq!(v, got.to_bits());
        }

        #[test]
        fn roundtrip_f64(v in any::<u64>()) {
            let f = f64::from_bits(v);
            let mut buf = Vec::new();
            write_f64_le(&mut buf, f).unwrap();
            let got = read_f64_le(&mut buf.as_slice()).unwrap();
            prop_assert_eq!(v, got.to_bits());
        }

        #[test]
        fn roundtrip_string(s in "[a-zA-Z0-9_]{0,64}") {
            let mut buf = Vec::new();
            write_length_prefixed_str(&mut buf, &s).unwrap();
            let got = read_length_prefixed_str(&mut buf.as_slice()).unwrap();
            prop_assert_eq!(s, got);
        }

        #[test]
        fn roundtrip_bytes(b in prop::collection::vec(any::<u8>(), 0..128)) {
            let mut buf = Vec::new();
            write_length_prefixed_bytes(&mut buf, &b).unwrap();
            let got = read_length_prefixed_bytes(&mut buf.as_slice()).unwrap();
            prop_assert_eq!(b, got);
        }
    }

    // ── Header round-trip ───────────────────────────────────────

    #[test]
    fn roundtrip_header() {
        let meta = BuildMetadata {
            toolchain: "1.78.0".into(),
            target_triple: "x86_64-unknown-linux-gnu".into(),
            murk_version: "0.1.0".into(),
            compile_flags: "release".into(),
        };
        let init = InitDescriptor {
            seed: 42,
            config_hash: 0xDEADBEEF,
            field_count: 5,
            cell_count: 10000,
            space_descriptor: vec![1, 2, 3, 4],
        };

        let mut buf = Vec::new();
        encode_header(&mut buf, &meta, &init).unwrap();

        let (got_meta, got_init) = decode_header(&mut buf.as_slice()).unwrap();
        assert_eq!(meta, got_meta);
        assert_eq!(init, got_init);
    }

    #[test]
    fn bad_magic_rejected() {
        let data = b"XURK\x01";
        let result = decode_header(&mut data.as_slice());
        assert!(matches!(result, Err(ReplayError::InvalidMagic)));
    }

    #[test]
    fn bad_version_rejected() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&MAGIC);
        buf.push(99); // bad version
        let result = decode_header(&mut buf.as_slice());
        assert!(matches!(
            result,
            Err(ReplayError::UnsupportedVersion { found: 99 })
        ));
    }

    // ── Command round-trip ──────────────────────────────────────

    proptest! {
        #[test]
        fn roundtrip_command(cmd in arb_command()) {
            let sc = serialize_command(&cmd);
            let got = deserialize_command(&sc).unwrap();
            // Compare payloads (expires_after_tick and arrival_seq differ by design)
            prop_assert_eq!(&cmd.payload, &got.payload);
            prop_assert_eq!(cmd.priority_class, got.priority_class);
            prop_assert_eq!(cmd.source_id, got.source_id);
            prop_assert_eq!(cmd.source_seq, got.source_seq);
        }
    }

    // ── Frame round-trip ────────────────────────────────────────

    #[test]
    fn roundtrip_frame_empty() {
        let frame = Frame {
            tick_id: 42,
            commands: vec![],
            snapshot_hash: 0xCAFEBABE,
        };

        let mut buf = Vec::new();
        encode_frame(&mut buf, &frame).unwrap();
        let got = decode_frame(&mut buf.as_slice()).unwrap().unwrap();
        assert_eq!(frame, got);
    }

    #[test]
    fn roundtrip_frame_with_commands() {
        let frame = Frame {
            tick_id: 100,
            commands: vec![
                serialize_command(&Command {
                    payload: CommandPayload::SetParameter {
                        key: ParameterKey(0),
                        value: 3.14,
                    },
                    expires_after_tick: TickId(u64::MAX),
                    source_id: Some(1),
                    source_seq: Some(1),
                    priority_class: 1,
                    arrival_seq: 0,
                }),
                serialize_command(&Command {
                    payload: CommandPayload::Move {
                        entity_id: 7,
                        target_coord: Coord::from_slice(&[1, 2]),
                    },
                    expires_after_tick: TickId(u64::MAX),
                    source_id: Some(2),
                    source_seq: Some(3),
                    priority_class: 0,
                    arrival_seq: 0,
                }),
            ],
            snapshot_hash: 0xDEAD,
        };

        let mut buf = Vec::new();
        encode_frame(&mut buf, &frame).unwrap();
        let got = decode_frame(&mut buf.as_slice()).unwrap().unwrap();
        assert_eq!(frame, got);
    }

    #[test]
    fn eof_returns_none() {
        let buf: Vec<u8> = Vec::new();
        let got = decode_frame(&mut buf.as_slice()).unwrap();
        assert!(got.is_none());
    }

    // ── P1: Partial tick header detection ────────────────────────

    #[test]
    fn partial_tick_header_is_error_not_eof() {
        // 1-7 bytes of a tick header should be treated as corruption,
        // not clean EOF.
        for partial_len in 1..=7 {
            let buf = vec![0xAA; partial_len];
            let result = decode_frame(&mut buf.as_slice());
            assert!(
                result.is_err(),
                "expected error for {partial_len}-byte partial tick header, got Ok"
            );
            let err = result.unwrap_err();
            match &err {
                ReplayError::MalformedFrame { detail } => {
                    assert!(
                        detail.contains("truncated frame header"),
                        "wrong error detail for {partial_len} bytes: {detail}"
                    );
                }
                other => panic!(
                    "expected MalformedFrame for {partial_len} bytes, got {other:?}"
                ),
            }
        }
    }

    #[test]
    fn complete_tick_header_but_truncated_body_is_error() {
        // A full 8-byte tick_id followed by truncated body must also error.
        let buf = 42u64.to_le_bytes().to_vec();
        let result = decode_frame(&mut buf.as_slice());
        assert!(result.is_err(), "truncated frame body should error");
    }

    // ── P2: source_id/source_seq Option round-trip ──────────────

    #[test]
    fn source_id_none_roundtrips() {
        let cmd = Command {
            payload: CommandPayload::Despawn { entity_id: 1 },
            expires_after_tick: TickId(u64::MAX),
            source_id: None,
            source_seq: None,
            priority_class: 1,
            arrival_seq: 0,
        };
        let sc = serialize_command(&cmd);
        assert_eq!(sc.source_id, None);
        assert_eq!(sc.source_seq, None);
        let got = deserialize_command(&sc).unwrap();
        assert_eq!(got.source_id, None);
        assert_eq!(got.source_seq, None);
    }

    #[test]
    fn source_id_some_zero_roundtrips_as_some_zero() {
        // This is the critical case: Some(0) must NOT become None.
        let cmd = Command {
            payload: CommandPayload::Despawn { entity_id: 1 },
            expires_after_tick: TickId(u64::MAX),
            source_id: Some(0),
            source_seq: Some(0),
            priority_class: 1,
            arrival_seq: 0,
        };
        let sc = serialize_command(&cmd);
        assert_eq!(sc.source_id, Some(0));
        assert_eq!(sc.source_seq, Some(0));
        let got = deserialize_command(&sc).unwrap();
        assert_eq!(got.source_id, Some(0));
        assert_eq!(got.source_seq, Some(0));
    }

    #[test]
    fn source_id_some_zero_frame_roundtrip() {
        // Verify the wire format also preserves Some(0) through
        // encode_frame → decode_frame.
        let frame = Frame {
            tick_id: 1,
            commands: vec![SerializedCommand {
                payload_type: PAYLOAD_DESPAWN,
                payload: 99u64.to_le_bytes().to_vec(),
                priority_class: 1,
                source_id: Some(0),
                source_seq: Some(0),
            }],
            snapshot_hash: 0,
        };

        let mut buf = Vec::new();
        encode_frame(&mut buf, &frame).unwrap();
        let got = decode_frame(&mut buf.as_slice()).unwrap().unwrap();
        assert_eq!(got.commands[0].source_id, Some(0));
        assert_eq!(got.commands[0].source_seq, Some(0));
    }

    #[test]
    fn source_none_vs_some_zero_distinguishable_on_wire() {
        // Verify that None and Some(0) produce different wire bytes.
        let frame_none = Frame {
            tick_id: 1,
            commands: vec![SerializedCommand {
                payload_type: PAYLOAD_DESPAWN,
                payload: 1u64.to_le_bytes().to_vec(),
                priority_class: 1,
                source_id: None,
                source_seq: None,
            }],
            snapshot_hash: 0,
        };
        let frame_zero = Frame {
            tick_id: 1,
            commands: vec![SerializedCommand {
                payload_type: PAYLOAD_DESPAWN,
                payload: 1u64.to_le_bytes().to_vec(),
                priority_class: 1,
                source_id: Some(0),
                source_seq: Some(0),
            }],
            snapshot_hash: 0,
        };

        let mut buf_none = Vec::new();
        encode_frame(&mut buf_none, &frame_none).unwrap();
        let mut buf_zero = Vec::new();
        encode_frame(&mut buf_zero, &frame_zero).unwrap();

        // Different wire representations
        assert_ne!(buf_none, buf_zero);

        // Both round-trip correctly
        let got_none = decode_frame(&mut buf_none.as_slice()).unwrap().unwrap();
        let got_zero = decode_frame(&mut buf_zero.as_slice()).unwrap().unwrap();
        assert_eq!(got_none.commands[0].source_id, None);
        assert_eq!(got_zero.commands[0].source_id, Some(0));
    }

    // ── P2: Invalid presence flag rejection ─────────────────────

    #[test]
    fn invalid_source_id_presence_flag_rejected() {
        // Build a valid frame, then corrupt the source_id presence flag byte.
        let frame = Frame {
            tick_id: 1,
            commands: vec![SerializedCommand {
                payload_type: PAYLOAD_DESPAWN,
                payload: 1u64.to_le_bytes().to_vec(),
                priority_class: 1,
                source_id: None,
                source_seq: None,
            }],
            snapshot_hash: 0,
        };

        let mut buf = Vec::new();
        encode_frame(&mut buf, &frame).unwrap();

        // The source_id presence flag is the byte right after priority_class.
        // Layout per command: payload_type(1) + len(4) + payload(8) + priority_class(1)
        //   = 14 bytes into the command area.
        // Frame starts: tick_id(8) + command_count(4) = 12 bytes header.
        // So the source_id flag is at offset 12 + 14 = 26.
        let flag_offset = 12 + 1 + 4 + 8 + 1; // tick(8)+count(4) + type(1)+len(4)+payload(8)+prio(1)
        assert_eq!(buf[flag_offset], 0, "sanity: should be the None flag");
        buf[flag_offset] = 2; // invalid flag value

        let result = decode_frame(&mut buf.as_slice());
        assert!(result.is_err());
        match result.unwrap_err() {
            ReplayError::MalformedFrame { detail } => {
                assert!(
                    detail.contains("invalid source_id presence flag"),
                    "wrong detail: {detail}"
                );
            }
            other => panic!("expected MalformedFrame, got {other:?}"),
        }
    }

    #[test]
    fn invalid_source_seq_presence_flag_rejected() {
        // Build a frame with source_id=None, then corrupt the source_seq flag.
        let frame = Frame {
            tick_id: 1,
            commands: vec![SerializedCommand {
                payload_type: PAYLOAD_DESPAWN,
                payload: 1u64.to_le_bytes().to_vec(),
                priority_class: 1,
                source_id: None,
                source_seq: None,
            }],
            snapshot_hash: 0,
        };

        let mut buf = Vec::new();
        encode_frame(&mut buf, &frame).unwrap();

        // source_seq flag is one byte after the source_id flag (since source_id=None).
        let seq_flag_offset = 12 + 1 + 4 + 8 + 1 + 1;
        assert_eq!(buf[seq_flag_offset], 0, "sanity: should be the None flag");
        buf[seq_flag_offset] = 0xFF; // invalid flag value

        let result = decode_frame(&mut buf.as_slice());
        assert!(result.is_err());
        match result.unwrap_err() {
            ReplayError::MalformedFrame { detail } => {
                assert!(
                    detail.contains("invalid source_seq presence flag"),
                    "wrong detail: {detail}"
                );
            }
            other => panic!("expected MalformedFrame, got {other:?}"),
        }
    }
}
