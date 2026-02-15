# murk-core

Leaf crate for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

Defines the foundational types shared across all Murk crates: field definitions,
commands, receipts, IDs, error types, and the core traits (`FieldReader`,
`FieldWriter`, `SnapshotAccess`).

Most users should depend on the top-level [`murk`](https://crates.io/crates/murk)
crate instead, which re-exports these types via `murk::types`.
