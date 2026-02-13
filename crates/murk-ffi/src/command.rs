//! C-compatible command and receipt types.
//!
//! [`MurkCommand`] is a flat C struct representing v1 command variants
//! (SetParameter, SetField). Converted to the Rust [`Command`] at the
//! FFI boundary.

use murk_core::command::{Command, CommandPayload, Receipt};
use murk_core::id::{Coord, FieldId, ParameterKey, TickId};

use crate::status::MurkStatus;

/// Command type discriminator.
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MurkCommandType {
    /// Set a global scalar parameter.
    SetParameter = 0,
    /// Set a single field value at a coordinate.
    SetField = 1,
}

/// Flat C-compatible command struct.
///
/// Fields are interpreted based on `command_type`. Unused fields are ignored.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MurkCommand {
    /// Command variant (0 = SetParameter, 1 = SetField).
    /// Stored as raw i32 to prevent UB from invalid C discriminators.
    pub command_type: i32,
    /// Command expires if not applied by this tick.
    pub expires_after_tick: u64,
    /// Optional source identifier (0 = none).
    pub source_id: u64,
    /// Optional per-source sequence number (0 = none).
    pub source_seq: u64,
    /// Priority class (lower = higher priority).
    pub priority_class: u8,
    /// Field ID (SetField).
    pub field_id: u32,
    /// Parameter key (SetParameter).
    pub param_key: u32,
    /// Float value (SetField).
    pub float_value: f32,
    /// Double value (SetParameter).
    pub double_value: f64,
    /// Coordinate (SetField, up to 4D).
    pub coord: [i32; 4],
    /// Number of coordinate dimensions used (SetField).
    pub coord_ndim: u32,
}

/// Receipt returned to C callers for each command.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct MurkReceipt {
    /// Whether the command was accepted (1) or rejected (0).
    pub accepted: u8,
    /// Tick at which the command was applied (0 = not applied).
    pub applied_tick_id: u64,
    /// Reason code (MurkStatus value, or 0 for none).
    pub reason_code: i32,
    /// Index of this command within the submitted batch.
    pub command_index: u32,
}

/// Convert a C `MurkCommand` to a Rust `Command`.
pub(crate) fn convert_command(cmd: &MurkCommand, index: usize) -> Result<Command, MurkStatus> {
    let payload = match cmd.command_type {
        x if x == MurkCommandType::SetParameter as i32 => CommandPayload::SetParameter {
            key: ParameterKey(cmd.param_key),
            value: cmd.double_value,
        },
        x if x == MurkCommandType::SetField as i32 => {
            let ndim = cmd.coord_ndim as usize;
            if ndim == 0 || ndim > 4 {
                return Err(MurkStatus::InvalidArgument);
            }
            let mut coord = Coord::new();
            for i in 0..ndim {
                coord.push(cmd.coord[i]);
            }
            CommandPayload::SetField {
                coord,
                field_id: FieldId(cmd.field_id),
                value: cmd.float_value,
            }
        }
        _ => return Err(MurkStatus::InvalidArgument),
    };

    Ok(Command {
        payload,
        expires_after_tick: TickId(cmd.expires_after_tick),
        source_id: if cmd.source_id == 0 {
            None
        } else {
            Some(cmd.source_id)
        },
        source_seq: if cmd.source_seq == 0 {
            None
        } else {
            Some(cmd.source_seq)
        },
        priority_class: cmd.priority_class,
        arrival_seq: index as u64,
    })
}

/// Convert a Rust `Receipt` to a C `MurkReceipt`.
pub(crate) fn convert_receipt(r: &Receipt) -> MurkReceipt {
    MurkReceipt {
        accepted: u8::from(r.accepted),
        applied_tick_id: r.applied_tick_id.map_or(0, |t| t.0),
        reason_code: r
            .reason_code
            .as_ref()
            .map_or(0, |e| MurkStatus::from(e) as i32),
        command_index: r.command_index as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn convert_set_parameter_round_trip() {
        let cmd = MurkCommand {
            command_type: MurkCommandType::SetParameter as i32,
            expires_after_tick: 100,
            source_id: 5,
            source_seq: 10,
            priority_class: 1,
            field_id: 0,
            param_key: 7,
            float_value: 0.0,
            double_value: 3.14,
            coord: [0; 4],
            coord_ndim: 0,
        };
        let rust_cmd = convert_command(&cmd, 0).unwrap();
        assert_eq!(rust_cmd.expires_after_tick, TickId(100));
        assert_eq!(rust_cmd.source_id, Some(5));
        assert_eq!(rust_cmd.source_seq, Some(10));
        assert_eq!(rust_cmd.priority_class, 1);
        match rust_cmd.payload {
            CommandPayload::SetParameter { key, value } => {
                assert_eq!(key, ParameterKey(7));
                assert!((value - 3.14).abs() < 1e-10);
            }
            other => panic!("expected SetParameter, got {other:?}"),
        }
    }

    #[test]
    fn convert_set_field_round_trip() {
        let cmd = MurkCommand {
            command_type: MurkCommandType::SetField as i32,
            expires_after_tick: 50,
            source_id: 0,
            source_seq: 0,
            priority_class: 0,
            field_id: 3,
            param_key: 0,
            float_value: 99.5,
            double_value: 0.0,
            coord: [1, 2, 0, 0],
            coord_ndim: 2,
        };
        let rust_cmd = convert_command(&cmd, 1).unwrap();
        assert_eq!(rust_cmd.source_id, None);
        assert_eq!(rust_cmd.source_seq, None);
        assert_eq!(rust_cmd.arrival_seq, 1);
        match rust_cmd.payload {
            CommandPayload::SetField {
                coord,
                field_id,
                value,
            } => {
                assert_eq!(coord.as_slice(), &[1, 2]);
                assert_eq!(field_id, FieldId(3));
                assert!((value - 99.5).abs() < 1e-6);
            }
            other => panic!("expected SetField, got {other:?}"),
        }
    }

    #[test]
    fn convert_set_field_zero_ndim_errors() {
        let cmd = MurkCommand {
            command_type: MurkCommandType::SetField as i32,
            expires_after_tick: 10,
            source_id: 0,
            source_seq: 0,
            priority_class: 0,
            field_id: 0,
            param_key: 0,
            float_value: 0.0,
            double_value: 0.0,
            coord: [0; 4],
            coord_ndim: 0,
        };
        assert_eq!(
            convert_command(&cmd, 0).unwrap_err(),
            MurkStatus::InvalidArgument
        );
    }

    #[test]
    fn invalid_command_type_returns_invalid_argument() {
        let cmd = MurkCommand {
            command_type: 999,
            expires_after_tick: 0,
            source_id: 0,
            source_seq: 0,
            priority_class: 0,
            field_id: 0,
            param_key: 0,
            float_value: 0.0,
            double_value: 0.0,
            coord: [0; 4],
            coord_ndim: 0,
        };
        assert_eq!(
            convert_command(&cmd, 0).unwrap_err(),
            MurkStatus::InvalidArgument
        );
    }

    #[test]
    fn convert_receipt_preserves_fields() {
        use murk_core::error::IngressError;

        let r = Receipt {
            accepted: true,
            applied_tick_id: Some(TickId(42)),
            reason_code: None,
            command_index: 7,
        };
        let c = convert_receipt(&r);
        assert_eq!(c.accepted, 1);
        assert_eq!(c.applied_tick_id, 42);
        assert_eq!(c.reason_code, 0);
        assert_eq!(c.command_index, 7);

        let r2 = Receipt {
            accepted: false,
            applied_tick_id: None,
            reason_code: Some(IngressError::QueueFull),
            command_index: 3,
        };
        let c2 = convert_receipt(&r2);
        assert_eq!(c2.accepted, 0);
        assert_eq!(c2.applied_tick_id, 0);
        assert_eq!(c2.reason_code, MurkStatus::QueueFull as i32);
        assert_eq!(c2.command_index, 3);
    }
}
