# Murk Replay Wire Format Specification

Binary format for deterministic replay recording and playback. All integers are little-endian. Strings and byte arrays are length-prefixed with a `u32` length. No compression, no alignment padding, no self-describing schema.

**Current version:** 3
**Magic:** `b"MURK"` (4 bytes)
**Byte order:** Little-endian throughout

See [Primitive Encoding](#primitive-encoding) for type definitions used throughout this document.

---

## Table of Contents

- [File Structure](#file-structure)
- [Header Layout](#header-layout)
- [Frame Layout](#frame-layout)
- [Command Encoding](#command-encoding)
- [Primitive Encoding](#primitive-encoding)
- [Version History](#version-history)

---

## File Structure

```
[Header] [Frame 0] [Frame 1] ... [Frame N-1] [EOF]
```

A replay file consists of a single header followed by zero or more frames. EOF is detected by a clean zero-byte read at a frame boundary (no sentinel or frame count in the header).

---

## Header Layout

The header is written once at file creation by `ReplayWriter::new()` and validated on open by `ReplayReader::open()`.

```
Offset  Size     Type                Description
──────  ────     ────                ───────────
0       4        [u8; 4]             Magic bytes: b"MURK"
4       1        u8                  Format version (currently 3)
```

### Build Metadata

Immediately follows the format version. All strings are length-prefixed (u32 length + UTF-8 bytes).

```
Offset  Size     Type                Description
──────  ────     ────                ───────────
5       4+N      lpstring            toolchain (e.g. "1.78.0")
5+a     4+N      lpstring            target_triple (e.g. "x86_64-unknown-linux-gnu")
5+a+b   4+N      lpstring            murk_version (e.g. "0.1.0")
5+a+b+c 4+N      lpstring            compile_flags (e.g. "release")
```

Where `lpstring` means `u32 length (LE) + N bytes of UTF-8 data`, and `a`, `b`, `c` denote the variable sizes of preceding strings (4 + string length each).

### Init Descriptor

Immediately follows build metadata. Contains the simulation initialization parameters needed to reconstruct an identical world for replay.

```
Offset  Size     Type                Description
──────  ────     ────                ───────────
+0      8        u64 LE              seed: RNG seed for deterministic simulation
+8      8        u64 LE              config_hash: hash of the world configuration
+16     4        u32 LE              field_count: number of fields in the world
+20     8        u64 LE              cell_count: total spatial cells
+28     4+N      lpbytes             space_descriptor: opaque serialized space descriptor
```

Where `lpbytes` means `u32 length (LE) + N bytes of opaque data`.

**Total header size:** 5 + (4 variable-length strings) + 28 + (1 variable-length byte array) = variable.

---

## Frame Layout

Each frame records a single tick's command inputs and the resulting snapshot hash for determinism verification. Frames are written sequentially with no padding between them.

```
Offset  Size     Type                Description
──────  ────     ────                ───────────
+0      8        u64 LE              tick_id: the tick number
+8      4        u32 LE              command_count: number of commands in this frame
+12     ...      [Command]           command_count serialized commands (see below)
+N      8        u64 LE              snapshot_hash: FNV-1a hash of the post-tick snapshot
```

### EOF Detection

When reading frames, a clean EOF (zero bytes available at the start of a frame) returns `None` (no more frames). A partial read of the 8-byte `tick_id` header (1-7 bytes) is treated as a truncation error (`MalformedFrame`), not a clean EOF. This distinguishes complete files from files truncated by a crash during recording.

---

## Command Encoding

Each command within a frame is encoded as follows:

```
Offset  Size     Type                Description
──────  ────     ────                ───────────
+0      1        u8                  payload_type: discriminant tag (see table below)
+1      4        u32 LE              payload_length: byte length of the payload
+5      N        [u8]                payload: serialized command data (N = payload_length)
+5+N    1        u8                  priority_class: lower = higher priority
+6+N    1        u8                  source_id presence flag (0 = absent, 1 = present)
+7+N    0 or 8   u64 LE              source_id value (only if presence flag = 1)
+...    1        u8                  source_seq presence flag (0 = absent, 1 = present)
+...    0 or 8   u64 LE              source_seq value (only if presence flag = 1)
+...    8        u64 LE              expires_after_tick
+...    8        u64 LE              arrival_seq
```

**Command size:** varies from 8 bytes (minimum: 1 + 4 + 0 + 1 + 1 + 1 = 8 with empty payload, no source fields) to unbounded depending on payload size and source field presence.

`expires_after_tick` and `arrival_seq` are serialized in format version 3.

### Presence Flag Encoding

The `source_id` and `source_seq` fields use explicit presence flags to distinguish `None` from `Some(0)`:

| Flag value | Meaning | Following bytes |
|------------|---------|-----------------|
| `0x00` | Absent (`None`) | 0 bytes |
| `0x01` | Present (`Some(value)`) | 8 bytes (u64 LE) |
| Other | Invalid | Decode error (`MalformedFrame`) |

This encoding was introduced in format version 2 to fix a bug in v1 where `Some(0)` was indistinguishable from `None`.

---

## Payload Type Tags

| Tag | Constant | CommandPayload Variant |
|-----|----------|----------------------|
| `0` | `PAYLOAD_MOVE` | `Move` |
| `1` | `PAYLOAD_SPAWN` | `Spawn` |
| `2` | `PAYLOAD_DESPAWN` | `Despawn` |
| `3` | `PAYLOAD_SET_FIELD` | `SetField` |
| `4` | `PAYLOAD_CUSTOM` | `Custom` |
| `5` | `PAYLOAD_SET_PARAMETER` | `SetParameter` |
| `6` | `PAYLOAD_SET_PARAMETER_BATCH` | `SetParameterBatch` |

Unrecognized tags produce `ReplayError::UnknownPayloadType`.

---

## Payload Serialization

### Move (tag 0)

```
Offset  Size     Type                Description
──────  ────     ────                ───────────
0       8        u64 LE              entity_id
8       4+N*4    coord               target_coord (see Coord encoding below)
```

### Spawn (tag 1)

```
Offset  Size     Type                Description
──────  ────     ────                ───────────
0       4+N*4    coord               coord: spawn location
+a      4        u32 LE              field_values count
+a+4    M*(4+4)  [(u32, f32)]        field_values: array of (FieldId as u32 LE, value as f32 LE)
```

### Despawn (tag 2)

```
Offset  Size     Type                Description
──────  ────     ────                ───────────
0       8        u64 LE              entity_id
```

### SetField (tag 3)

```
Offset  Size     Type                Description
──────  ────     ────                ───────────
0       4+N*4    coord               coord: target cell
+a      4        u32 LE              field_id (FieldId inner value)
+a+4    4        f32 LE              value
```

### Custom (tag 4)

```
Offset  Size     Type                Description
──────  ────     ────                ───────────
0       4        u32 LE              type_id: user-registered type identifier
4       4        u32 LE              data_length: byte length of opaque data
8       N        [u8]                data: opaque payload (N = data_length)
```

### SetParameter (tag 5)

```
Offset  Size     Type                Description
──────  ────     ────                ───────────
0       4        u32 LE              key (ParameterKey inner value)
4       8        f64 LE              value
```

Total payload size: 12 bytes (fixed).

### SetParameterBatch (tag 6)

```
Offset  Size     Type                Description
──────  ────     ────                ───────────
0       4        u32 LE              param_count: number of parameters
4       N*12     [(u32, f64)]        params: array of (ParameterKey as u32 LE, value as f64 LE)
```

Each entry is 12 bytes (4 bytes key + 8 bytes value).

---

## Coord Encoding

Coordinates (`Coord`, which is `SmallVec<[i32; 4]>`) are serialized as a length-prefixed array of `i32` values:

```
Offset  Size     Type                Description
──────  ────     ────                ───────────
0       4        u32 LE              dimension_count: number of coordinate components
4       N*4      [i32 LE]            components: coordinate values (N = dimension_count)
```

Total size: `4 + (dimension_count * 4)` bytes. For a typical 2D coordinate, this is 12 bytes.

---

## Primitive Encoding

All primitive types use little-endian byte order:

| Type | Size | Encoding |
|------|------|----------|
| `u8` | 1 byte | Raw byte |
| `u32` | 4 bytes | Little-endian |
| `u64` | 8 bytes | Little-endian |
| `i32` | 4 bytes | Little-endian |
| `f32` | 4 bytes | IEEE 754, little-endian |
| `f64` | 8 bytes | IEEE 754, little-endian |
| `lpstring` | 4 + N bytes | u32 LE length prefix + UTF-8 bytes |
| `lpbytes` | 4 + N bytes | u32 LE length prefix + raw bytes |

---

## Snapshot Hash

The `snapshot_hash` field in each frame is an FNV-1a hash computed over the post-tick snapshot state. It is used during replay to verify determinism: after replaying all commands for a tick, the replayed simulation's snapshot hash is compared against the recorded hash. A mismatch produces `ReplayError::SnapshotMismatch`.

The hash is computed by `snapshot_hash()` in `crates/murk-replay/src/hash.rs` and covers all fields up to `field_count`.

---

## Version History

### Version 3 (current)

- **expires_after_tick and arrival_seq** are appended per command as `u64 LE` values.
- This preserves command expiry and deterministic ordering metadata through replay.

### Version 2

- **source_id and source_seq** use presence-flag encoding: a `u8` flag (`0` = absent, `1` = present) followed by an optional `u64` value.
- This correctly distinguishes `None` from `Some(0)`.
- **Superseded** by version 3. Files with version 2 are rejected with `ReplayError::UnsupportedVersion { found: 2 }`.

### Version 1

- **source_id and source_seq** were encoded as bare `u64` values where `0` meant "not set".
- Bug: `Some(0)` was indistinguishable from `None`, causing incorrect replay of commands with `source_id = Some(0)`.
- **Superseded** by later versions. Files with version 1 are rejected with `ReplayError::UnsupportedVersion { found: 1 }`.
