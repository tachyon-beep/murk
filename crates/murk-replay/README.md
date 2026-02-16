# murk-replay

Deterministic replay recording and verification for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

Record simulation runs with `ReplayWriter`, replay them with `ReplayReader`,
and verify determinism with per-tick snapshot hashing and divergence reports.

Available via the top-level [`murk`](https://crates.io/crates/murk) crate
as `murk::replay`.

## Installation

```bash
cargo add murk-replay
```

Most users should depend on the top-level [`murk`](https://crates.io/crates/murk) crate,
which re-exports this as `murk::replay`.

## Usage

```rust
use murk_replay::{ReplayWriter, ReplayReader, BuildMetadata, InitDescriptor, Frame};

let meta = BuildMetadata {
    toolchain: "stable".into(),
    target_triple: "x86_64-unknown-linux-gnu".into(),
    murk_version: "0.1.0".into(),
    compile_flags: "".into(),
};
let init = InitDescriptor {
    seed: 42,
    config_hash: 0,
    field_count: 1,
    cell_count: 256,
    space_descriptor: vec![],
};

let mut buf = Vec::new();
let mut writer = ReplayWriter::new(&mut buf, &meta, &init).unwrap();
let frame = Frame { tick_id: 1, commands: vec![], snapshot_hash: 0xABCD };
writer.write_raw_frame(&frame).unwrap();
drop(writer);

let mut reader = ReplayReader::open(buf.as_slice()).unwrap();
let f = reader.next_frame().unwrap().unwrap();
assert_eq!(f.tick_id, 1);
```

## Documentation

- [Murk Book](https://tachyon-beep.github.io/murk/) — concepts and guides
- [API Reference](https://docs.rs/murk-replay) — rustdoc
- [Examples](https://github.com/tachyon-beep/murk/tree/main/examples) — complete working projects
