# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- `pool_2d_output_shape` returns `Result<(usize, usize), ObsError>` instead of panicking on invalid arguments

### Fixed

- `ObsPlan::compile` now rejects `AgentRect` when `half_extent.len() != space.ndim()` instead of silently truncating dimensions via zip (#110)
- FlatBuffer deserializer accepts empty `Coords(vec![])` round-trip (previously rejected `ndim==0`) (#111)
- `GridGeometry::canonical_rank` upgraded from `debug_assert!`/`debug_assert_eq!` to `assert!`/`assert_eq!` — negative coordinates caught in release builds (#118)

## [0.1.7](https://github.com/tachyon-beep/murk/compare/murk-obs-v0.1.2...murk-obs-v0.1.7) - 2026-02-21

### Fixed

- FlatBuffer silent u16 truncation
- FlatBuffer signed/unsigned cast corruption
- Per-agent scratch allocation overflow
- Normalize inverted range
- Canonical rank negative coordinate handling
- Pool NaN produces infinity
- Plan fast-path unchecked index panic
- Geometry `is_interior` missing dimension check

## [0.1.2](https://github.com/tachyon-beep/murk/compare/murk-obs-v0.1.1...murk-obs-v0.1.2) - 2026-02-16

### Other

- release v0.1.1

## [0.1.1](https://github.com/tachyon-beep/murk/compare/murk-obs-v0.1.0...murk-obs-v0.1.1) - 2026-02-16

### Other

- release v0.1.1
