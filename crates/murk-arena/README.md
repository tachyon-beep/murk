# murk-arena

Arena-based generational allocation for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

- **Double-buffered ping-pong arenas** alternate between staging (writable) and published (readable) roles
- **Three field mutability classes:** Static (generation-0 forever), PerTick (fresh each tick), Sparse (copy-on-write)
- **Zero-copy snapshots** via generation-tracked buffer swaps
- **Deterministic memory lifetimes** with no GC pauses and no `Box<dyn>` per cell

This is an internal crate. Most users should depend on the top-level
[`murk`](https://crates.io/crates/murk) crate instead.

## Installation

```bash
cargo add murk-arena
```

Most users should depend on the top-level [`murk`](https://crates.io/crates/murk) crate,
which re-exports this as `murk::types`.

## When to use directly

Depend on this crate directly only if building custom crates outside the Murk
workspace or need minimal dependencies.

## Usage

`murk-arena` is used internally by the engine. Users rarely interact
with it directly. The primary types are `PingPongArena` (the
double-buffered allocator), `Snapshot` (read-only view of published
state), and `ArenaConfig` (capacity tuning):

```rust
use murk_arena::ArenaConfig;

let config = ArenaConfig::new(256);
assert_eq!(config.cell_count, 256);
assert_eq!(config.segment_bytes(), 64 * 1024 * 1024);
```

`PingPongArena` is constructed by the engine with field definitions
and a shared static arena. See `murk-engine` for the user-facing API.

## Documentation

- [Murk Book](https://tachyon-beep.github.io/murk/) — concepts and guides
- [API Reference](https://docs.rs/murk-arena) — rustdoc
- [Examples](https://github.com/tachyon-beep/murk/tree/main/examples) — complete working projects
