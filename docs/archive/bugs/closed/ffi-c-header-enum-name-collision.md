# Bug Report

**Date:** 2026-02-24
**Reporter:** static-analysis-agent
**Severity:** [ ] Critical | [x] High | [ ] Medium | [ ] Low

## Affected Crate(s)

- [ ] murk-core
- [ ] murk-engine
- [ ] murk-arena
- [ ] murk-space
- [ ] murk-propagator
- [ ] murk-propagators
- [ ] murk-obs
- [ ] murk-replay
- [x] murk-ffi
- [ ] murk-python
- [ ] murk-bench
- [ ] murk-test-utils
- [ ] murk (umbrella)

## Engine Mode

- [ ] Lockstep
- [ ] RealtimeAsync
- [x] Both / Unknown

## Summary

The generated C header `murk.h` defines duplicate unscoped enum constants (`Absorb`, `Clamp`, `Wrap`) across `enum MurkBoundaryBehavior` and `enum MurkEdgeBehavior`, making the header uncompilable in any C translation unit.

## Steps to Reproduce

1. Create a minimal C file that includes `murk.h`:
   ```c
   #include "murk.h"
   int main(void) { return 0; }
   ```
2. Compile with any C compiler:
   ```
   cc -xc -fsyntax-only -c test.c
   ```
3. Observe redeclaration errors for `Absorb`, `Clamp`, and `Wrap`.

## Expected Behavior

Including `murk.h` in C should compile cleanly. Each enum constant should have a unique name in the global C namespace.

## Actual Behavior

Compilation fails with redeclaration errors because C enumerators are unscoped (flat namespace), and three names collide:

- `Absorb`: defined in `MurkBoundaryBehavior` (line 35, value 2) and `MurkEdgeBehavior` (line 65, value 0)
- `Clamp`: defined in `MurkBoundaryBehavior` (line 27, value 0) and `MurkEdgeBehavior` (line 69, value 1)
- `Wrap`: defined in `MurkBoundaryBehavior` (line 39, value 3) and `MurkEdgeBehavior` (line 73, value 2)

Note: the colliding constants also have different numeric values between the two enums, so even if a C compiler tolerated the redefinition, usage would be silently wrong.

## Reproduction Rate

Always (deterministic).

## Environment

- **OS:** Any
- **Rust toolchain:** stable
- **Murk version/commit:** HEAD (feat/release-0.1.9)

## Determinism Impact

- [x] Bug is deterministic (same inputs always reproduce it)
- [ ] Bug is non-deterministic (flaky / timing-dependent)
- [ ] Replay divergence observed

## Logs / Backtrace

```
N/A - found via static analysis
```

## Minimal Reproducer

```c
#include "murk.h"
int main(void) { return 0; }
// cc -xc -fsyntax-only test.c
// error: redeclaration of enumerator 'Absorb'
// error: redeclaration of enumerator 'Clamp'
// error: redeclaration of enumerator 'Wrap'
```

## Additional Context

**Source report:** `docs/bugs/generated/crates/murk-ffi/include/murk.h.md`

**Affected files:**
- Header: `crates/murk-ffi/include/murk.h:23-41` (`enum MurkBoundaryBehavior`) and `crates/murk-ffi/include/murk.h:61-75` (`enum MurkEdgeBehavior`)
- Rust enums: `crates/murk-ffi/src/types.rs:57-69` (`MurkBoundaryBehavior`) and `crates/murk-ffi/src/types.rs:71-81` (`MurkEdgeBehavior`)
- cbindgen config: `crates/murk-ffi/cbindgen.toml:45-46` (`rename_variants = "None"`)

**Root cause:** cbindgen emits Rust enum variant names verbatim into C, where enum constants share a single flat namespace. The `rename_variants = "None"` setting in `cbindgen.toml` does not prefix variants with the enum type name.

**Suggested fix:** Configure cbindgen to prefix enum variants with a scoped name. Options:
1. Set `[enum] prefix_with_name = true` in `cbindgen.toml` to emit e.g. `MurkBoundaryBehavior_Clamp`, `MurkEdgeBehavior_Clamp`.
2. Alternatively, use `[export.rename]` to manually map conflicting variant names to prefixed constants (e.g. `MURK_BOUNDARY_CLAMP`, `MURK_EDGE_CLAMP`).
3. Alternatively, rename the Rust variants to be globally unique (e.g. `BoundaryClamp`, `EdgeClamp`), though this is more invasive to the Rust API.

Option 1 is the least disruptive and handles future enum additions automatically.
