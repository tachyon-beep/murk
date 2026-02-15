# murk-arena

Arena-based generational allocation for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

Provides double-buffered ping-pong arenas with three field mutability classes
(Static, PerTick, Sparse), zero-copy snapshots, and deterministic memory
lifetimes. No GC pauses, no `Box<dyn>` per cell.

This is an internal crate. Most users should depend on the top-level
[`murk`](https://crates.io/crates/murk) crate instead.
