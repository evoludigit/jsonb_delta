# Changelog

All notable changes to the `jsonb_delta` extension will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [0.1.0] - 2025-12-12

### 🎯 Initial Release

Complete JSONB CRUD operations for PostgreSQL CQRS architectures. **2-7× faster** than native SQL re-aggregation.

### Added

#### Core Functions

- **`jsonb_merge_shallow(target, source)`**
  - Shallow merge of two JSONB objects
  - Source keys overwrite target keys on conflict

- **`jsonb_merge_at_path(target, source, path)`**
  - Merge JSONB object at nested path
  - Array-based path specification

#### Smart Patch Functions

- **`jsonb_smart_patch_scalar(target, source)`**
  - Intelligent shallow merge for top-level updates
  - Simplifies incremental view maintenance

- **`jsonb_smart_patch_nested(target, source, path)`**
  - Merge at nested path with cleaner API
  - Wrapper around `jsonb_merge_at_path`

- **`jsonb_smart_patch_array(target, source, array_path, match_key, match_value)`**
  - Update specific array element
  - Combines search and update in single operation

#### Array Operations

- **`jsonb_array_update_where(target, array_path, match_key, match_value, updates)`**
  - Update single array element by matching predicate
  - **2-3× faster** than SQL re-aggregation

- **`jsonb_array_update_where_batch(target, array_path, match_key, updates_array)`**
  - Batch update multiple array elements
  - **3-5× faster** than multiple separate calls

- **`jsonb_array_update_multi_row(targets[], array_path, match_key, match_value, updates)`**
  - Update arrays across multiple documents
  - **~4× faster** for 100-row batches

- **`jsonb_array_insert_where(target, array_path, element, sort_key, order)`**
  - Insert with optional sort order maintenance
  - **4-6× faster** than re-aggregation with sorting

- **`jsonb_array_delete_where(target, array_path, match_key, match_value)`**
  - Surgical array element deletion
  - **5-7× faster** than re-aggregation

#### Deep Merge

- **`jsonb_deep_merge(target, source)`**
  - Recursive deep merge for nested structures
  - Preserves existing fields while updating changed ones
  - **2× faster** than multiple path-based operations

#### Helper Functions

- **`jsonb_extract_id(data, key DEFAULT 'id')`**
  - Safe ID extraction (supports UUID and integer)
  - Returns text representation

- **`jsonb_array_contains_id(data, array_path, id_key, id_value)`**
  - Fast existence check with optimized integer search
  - Loop unrolling for better performance

### Performance

| Operation | Native SQL | jsonb_delta | Speedup |
|-----------|-----------|-----------|---------|
| Array element update | 1.91 ms | 0.72 ms | **2.66×** |
| Array DELETE | 20-30 ms | 4-6 ms | **5-7×** |
| Array INSERT (sorted) | 22-35 ms | 5-8 ms | **4-6×** |
| Deep merge | 8-12 ms | 4-6 ms | **2×** |

### Compatibility

- PostgreSQL 13-18 support
- Zero external dependencies
- Pure Rust implementation with pgrx 0.16.1
- All functions: IMMUTABLE, PARALLEL SAFE, STRICT

### Documentation

- Complete API reference with examples
- Integration guide for CQRS/pg_tview workflows
- Comprehensive test suite
- Benchmark results

---

## Contributing

See [contributing.md](contributing.md) for contribution guidelines.

## Links

- **GitHub**: [Repository](https://github.com/evoludigit/jsonb_delta)
- **Issues**: [GitHub Issues](https://github.com/evoludigit/jsonb_delta/issues)
