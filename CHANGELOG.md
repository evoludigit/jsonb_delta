# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- **`jsonb_apply_changeset(doc, ops)`** — apply an ordered list of surgical edits to a JSONB document in a **single parse/serialize pass**. `ops` is a JSONB array of typed operations: `set`, `remove`, `merge`, `deep_merge`, `increment`, `array_update`, `array_update_all`, `array_replace`, `array_upsert`, `array_delete`, `array_insert`. Paths may be dot-notation strings (`"a.b[0].c"`) or segment arrays (`["a", "b", 0, "c"]`), and array matching works for any key type (int / text / **UUID**). Intended for incremental-view-maintenance callers (e.g. `pg_tviews`) that coalesce many changes to one row per transaction: replacing a chain of N `jsonb_smart_patch_*` calls with a single `jsonb_apply_changeset` amortizes the whole-document (de)serialization across the entire changeset. Measured **4.8×–40× faster** than the equivalent chained calls as the number of coalesced edits grows from 5 to 50 (PG 17, 500- and 5000-element arrays).

### Changed
- Toolchain: pgrx 0.16.1 → 0.17.0 (first pgrx with PostgreSQL 18 support);
  all cargo-pgrx pins in CI, Docker and the justfile moved with it.

### Fixed
- **Version coherence** (#14): `jsonb_delta.control` now installs
  `default_version = '0.2.0'`, matching the crate version. Previously a 0.2.0
  build installed an extension labelled 0.1.0.
- Shipped `sql/jsonb_delta--0.2.0.sql` (pgrx-generated) as the canonical install
  script for the crate version.
- Added `sql/jsonb_delta--0.1.0--0.2.0.sql` so existing 0.1.0 installs can run
  `ALTER EXTENSION jsonb_delta UPDATE`. Covered by `test/upgrade_path_test.sql`
  in CI and `just test-upgrade` locally.
- `just schema` now derives the script name from the crate version instead of
  hardcoding 0.1.0, and a version-guard test (`tests/version_coherence.rs`)
  fails the build if `Cargo.toml` and the control file ever disagree again.

### Security
- **Path segment-count cap**: `jsonb_apply_changeset` rejects op paths with more than `MAX_JSONB_DEPTH` (1000) segments, preventing construction of documents deeper than the depth cap (which would otherwise feed serde's unbounded output-serialization recursion). Changeset size is also capped at 10,000 ops per call.
- **Overflow-checked `increment`**: integer increments use checked arithmetic and raise an error on overflow instead of silently wrapping.

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
