# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.7](https://github.com/tachyon-beep/murk/compare/murk-engine-v0.1.5...murk-engine-v0.1.7) - 2026-02-21

### Added

- `BatchedEngine` for high-throughput parallel world stepping with a single GIL release
- `sparse_retired_ranges` and `sparse_pending_retired` fields on `StepMetrics`
- Batched topology validation: reject incompatible space topologies

### Fixed

- SetField command visibility across tick boundary
- Ring buffer spurious `None` on latest snapshot
- Shutdown blocks on slow tick in RealtimeAsync mode
- Backoff config not validated at construction
- Adaptive backoff output unused
- Egress epoch/tick mismatch
- Observe buffer bounds check
- Reset returns wrong error variant
- Tick accepts non-SetField commands silently
- Cell count u32 truncation

## [0.1.5](https://github.com/tachyon-beep/murk/compare/murk-engine-v0.1.4...murk-engine-v0.1.5) - 2026-02-16

### Fixed

- increase reset_lifecycle test timeout for slow CI runners

## [0.1.3](https://github.com/tachyon-beep/murk/compare/murk-engine-v0.1.2...murk-engine-v0.1.3) - 2026-02-16

### Other

- release v0.1.2

## [0.1.2](https://github.com/tachyon-beep/murk/compare/murk-engine-v0.1.1...murk-engine-v0.1.2) - 2026-02-16

### Other

- release v0.1.2

## [0.1.1](https://github.com/tachyon-beep/murk/compare/murk-engine-v0.1.0...murk-engine-v0.1.1) - 2026-02-16

### Fixed

- increase shutdown_budget test tolerance for CI runners

### Other

- reformat for rustfmt 1.93.1
