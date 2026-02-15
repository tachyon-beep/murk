# murk-ffi

C ABI bindings for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

Provides a stable C ABI with handle tables (slot+generation), safe
double-destroy semantics, and ABI versioning. Used by the Python bindings
(`murk-python`) and available for any C-compatible consumer.

Most users should use the Rust API via [`murk`](https://crates.io/crates/murk)
or the Python bindings via `pip install murk`.
