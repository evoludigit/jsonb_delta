# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2024-04-17

### Security
- **Array Bounds Protection**: Added array index cap (`MAX_JSONB_ARRAY_SIZE = 100,000`) to prevent OOM attacks via large index padding in `jsonb_delta_set_path` and `jsonb_delta_array_update_where_path`.
- **Input Validation**: Added `match_key` non-empty validation to all 7 array-matching functions (`jsonb_array_update_where`, `jsonb_array_delete_where`, `jsonb_array_insert_where`, `jsonb_array_update_where_batch`, `jsonb_array_update_multi_row`, `jsonb_smart_patch_array`, `jsonb_delta_array_update_where_path`).
- **Path Security**: Added path key-segment length cap (`MAX_KEY_LENGTH = 256` bytes) in `parse_path()` to prevent unbounded memory allocation.
- **Depth Protection**: Added JSONB nesting depth validation (max 1,000 levels) to prevent stack overflow attacks.

### Performance
- **Binary Search Optimization**: `find_insertion_point()` now uses binary search (`partition_point`) for O(log n) complexity down from O(n), significantly improving sorted array insertions.
- **SIMD Integer Matching**: Leverages auto-vectorization for integer ID lookups, optimized for the trinity pattern (`id` UUID / `pk_{entity}` BIGINT / `fk_{entity}` BIGINT / `identifier` text).
- **Helper Consolidation**: Removed duplicate code paths, reducing compilation overhead and improving maintainability.

### Developer Experience
- **Comprehensive Testing**: Added 34 unit tests, property-based fuzzing, and SQL integration tests covering all functions and edge cases.
- **Error Messages**: Improved error messages with specific values (actual depth found, key lengths, etc.) for better debugging.
- **Documentation**: Added detailed API documentation with security limits and usage examples.

### Fixed
- Depth validation error now reports the actual depth found instead of generic `>max`.
- Consolidated duplicate helper functions (`value_type_name`, `find_element_by_match`) across modules.

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

[Unreleased]: https://github.com/evoludigit/jsonb_delta/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/evoludigit/jsonb_delta/releases/tag/v0.2.0
[0.1.0]: https://github.com/evoludigit/jsonb_delta/releases/tag/v0.1.0
