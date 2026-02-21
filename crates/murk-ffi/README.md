# murk-ffi

C ABI bindings for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

Provides a stable C ABI with handle tables (slot+generation), safe
double-destroy semantics, and ABI versioning. Used by the Python bindings
(`murk-python`) and available for any C-compatible consumer.

Most users should use the Rust API via [`murk`](https://crates.io/crates/murk)
or the Python bindings via `pip install murk`.

## Installation

```bash
cargo add murk-ffi
```

Most users should depend on the top-level [`murk`](https://crates.io/crates/murk) crate,
which re-exports this as `murk::types`.

## Usage

`murk-ffi` exposes a C-compatible function interface. From C or Python,
the typical workflow is:

```c
#include <stdint.h>

// Query ABI compatibility
uint32_t version = murk_abi_version();  // major << 16 | minor

// Build a world through the config API
MurkConfig* cfg = murk_config_create();
murk_config_set_space(cfg, MURK_SPACE_SQUARE4, 16, 16);
murk_config_set_dt(cfg, 0.1);
murk_config_set_seed(cfg, 42);
murk_config_add_field(cfg, "heat", MURK_FIELD_SCALAR, MURK_MUT_PER_TICK);

// Create and step the world
MurkWorld* world = murk_lockstep_create(cfg);
MurkStatus status = murk_lockstep_step(world, commands, num_commands);
murk_lockstep_destroy(world);
```

For Python usage, install the `murk` package which wraps these FFI calls.

## Documentation

- [Murk Book](https://tachyon-beep.github.io/murk/) — concepts and guides
- [API Reference](https://docs.rs/murk-ffi) — rustdoc
- [Examples](https://github.com/tachyon-beep/murk/tree/main/examples) — complete working projects
