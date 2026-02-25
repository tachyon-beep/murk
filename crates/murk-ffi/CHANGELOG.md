# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `MurkStatus::NotApplied` (-22) — distinct status code for commands accepted but not applied (OOB coordinate, unknown field)
- `murk_batched_num_worlds_get`, `murk_batched_obs_output_len_get`, `murk_batched_obs_mask_len_get` — `_get` variants with unambiguous error reporting
- ABI version bumped from v3.0 to v3.1

### Fixed

- `IngressError::NotApplied` mapped to `NotApplied` (-22) instead of `UnsupportedCommand` (-21)
- `get_world()`/`get_batched()`/`get_obs_plan()` store `LAST_PANIC` diagnostic on mutex poisoning instead of silently discarding
- Legacy query functions store `LAST_PANIC` on inner mutex poisoning
- Batched query functions store `LAST_PANIC` on inner mutex poisoning

## [0.1.7](https://github.com/tachyon-beep/murk/compare/murk-ffi-v0.1.3...murk-ffi-v0.1.7) - 2026-02-21

### Added

- Auto-generated `include/murk.h` C header via cbindgen (42 functions, 8 structs, 8 enums)
- `sparse_retired_ranges` and `sparse_pending_retired` fields on `MurkStepMetrics`
- Batched FFI functions (`murk_batched_*`)
- `UnsupportedCommand` error variant

### Changed

- ABI version bumped from v1.0 to v2.0 (`MurkStepMetrics` layout: 40 → 48 bytes)

### Fixed

- Mutex poisoning panics across FFI boundary (3 fixes)
- Obs conversion duplicated across modules
- ObsPlan lock ordering inconsistency
- Trampoline null pointer dereference
- Config not consumed on null output pointer
- Inconsistent mutex poisoning handling
- `usize` in `#[repr(C)]` struct
- Handle accessor ambiguity (returns 0 for both success and invalid handle)
- Generation wraparound safety

## [0.1.3](https://github.com/tachyon-beep/murk/compare/murk-ffi-v0.1.2...murk-ffi-v0.1.3) - 2026-02-16

### Other

- release v0.1.2

## [0.1.2](https://github.com/tachyon-beep/murk/compare/murk-ffi-v0.1.1...murk-ffi-v0.1.2) - 2026-02-16

### Other

- release v0.1.2

## [0.1.1](https://github.com/tachyon-beep/murk/compare/murk-ffi-v0.1.0...murk-ffi-v0.1.1) - 2026-02-16

### Other

- reformat for rustfmt 1.93.1
