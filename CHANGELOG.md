# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed
- Simplified GitHub Actions CI workflow (removed macOS, platform detection logic)
- Expanded PostgreSQL test matrix from PG17 only to PG13-17

## [0.1.0] - 2024-12-17

### Added
- Initial release
- `jsonb_delta()` function to compute efficient deltas between JSONB values
- `jsonb_patch()` function to apply deltas to JSONB values
- Support for PostgreSQL versions 13-18
- Comprehensive test suite
- SQL integration tests
- Property-based fuzzing tests
- Load/performance tests
- Security scanning and compliance checks

### Features
- Efficient delta computation with minimal output size
- Support for nested objects and arrays
- Handles all JSONB value types (objects, arrays, strings, numbers, booleans, null)
- Idempotent patch application
- Round-trip guarantee: patch(original, delta(original, modified)) = modified

[Unreleased]: https://github.com/evoludigit/jsonb_delta/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/evoludigit/jsonb_delta/releases/tag/v0.1.0
