# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.7](https://github.com/tachyon-beep/murk/compare/murk-arena-v0.1.3...murk-arena-v0.1.7) - 2026-02-21

### Added

- `sparse_retired_range_count()` and `sparse_pending_retired_count()` accessors on `PingPongArena`
- `RetiredRange` named struct replacing retired range tuples

### Fixed

- Per-tick allocation undercount in memory reporting
- Scratch region reuse across ticks
- Segment slice beyond cursor panic
- Missing segment size validation
- Publish-without-begin-tick state guard
- Static arena duplicate field ID acceptance
- Descriptor clone-per-tick overhead
- Cell count components overflow
- Generation counter overflow handling
- Sparse CoW generation rollover
- Sparse segment memory leak from unbounded CoW allocations

## [0.1.3](https://github.com/tachyon-beep/murk/compare/murk-arena-v0.1.2...murk-arena-v0.1.3) - 2026-02-16

### Other

- release v0.1.2

## [0.1.2](https://github.com/tachyon-beep/murk/compare/murk-arena-v0.1.1...murk-arena-v0.1.2) - 2026-02-16

### Other

- release v0.1.2

## [0.1.1](https://github.com/tachyon-beep/murk/compare/murk-arena-v0.1.0...murk-arena-v0.1.1) - 2026-02-16

### Added

- CI improvements, PyPI publish fix, property tests, version badge

### Fixed

- exclude proptest modules from Miri with #[cfg(not(miri))]
