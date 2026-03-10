# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- `Fcc12::max_neighbour_degree()` returns 0 for degenerate 1×1×1 Absorb grid (consistent with Square4, Hex2D, Line1D)
- `Fcc12::canonical_ordering()` count verification upgraded from `debug_assert_eq!` to `assert_eq!` (murk-1a1cfd)
- `Fcc12::axis_distance_u32()` zero-length and diff-bounds guards upgraded from `debug_assert!` to `assert!` — prevents u32 underflow in release builds (murk-dace86, murk-b20f07)

## [0.1.7](https://github.com/tachyon-beep/murk/compare/murk-space-v0.1.3...murk-space-v0.1.7) - 2026-02-21

### Fixed

- Hex2D disk overflow on large radii
- FCC12 parity overflow
- Product space weighted metric truncation
- Compliance ordering for membership checks
- `is_multiple_of` MSRV compatibility

## [0.1.3](https://github.com/tachyon-beep/murk/compare/murk-space-v0.1.2...murk-space-v0.1.3) - 2026-02-16

### Other

- release v0.1.2

## [0.1.2](https://github.com/tachyon-beep/murk/compare/murk-space-v0.1.1...murk-space-v0.1.2) - 2026-02-16

### Other

- release v0.1.2

## [0.1.1](https://github.com/tachyon-beep/murk/compare/murk-space-v0.1.0...murk-space-v0.1.1) - 2026-02-16

### Other

- reformat for rustfmt 1.93.1
