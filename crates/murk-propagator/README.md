# murk-propagator

Propagator trait and pipeline validation for the [Murk](https://github.com/tachyon-beep/murk) simulation engine.

The `Propagator` trait is the main extension point for user-defined simulation
logic. Propagators are stateless per-tick operators that declare their field
reads and writes, enabling automatic write-conflict detection, Euler/Jacobi
read modes, and CFL validation.

Available via the top-level [`murk`](https://crates.io/crates/murk) crate
as `murk::propagator`.
