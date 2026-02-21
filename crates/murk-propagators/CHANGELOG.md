# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.7](https://github.com/tachyon-beep/murk/compare/murk-propagators-v0.1.2...murk-propagators-v0.1.7) - 2026-02-21

### Added

- `ScalarDiffusion` with builder pattern and configurable parameters
- `GradientCompute` standalone propagator with buffer bounds guarding
- `IdentityCopy` propagator for field mirroring
- `FlowField` propagator for vector field advection
- `AgentEmission` propagator for agent-driven field writes
- `ResourceField` propagator for resource dynamics
- `MorphologicalOp` propagator for spatial erosion/dilation
- `WavePropagation` propagator for wave equation simulation
- `NoiseInjection` propagator with `rand` dependency
- Integration tests through `LockstepWorld` for all propagators

### Changed

- Examples migrated from hardcoded propagators to library propagators
- Benchmark profiles switched to library propagators
- Hardcoded field constants deprecated

### Fixed

- Diffusion CFL uses hardcoded degree instead of space connectivity
- Scratch bytes/slots mismatch in capacity calculation
- Agent presence issues with tick-0 actions
- NaN/infinity validation gaps
- Reward stale heat gradient dependency
- Performance hotspots in inner loops

## [0.1.2](https://github.com/tachyon-beep/murk/compare/murk-propagators-v0.1.1...murk-propagators-v0.1.2) - 2026-02-16

### Other

- release v0.1.1

## [0.1.1](https://github.com/tachyon-beep/murk/compare/murk-propagators-v0.1.0...murk-propagators-v0.1.1) - 2026-02-16

### Other

- release v0.1.1
