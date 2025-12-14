# jsonb_ivm

Surgical JSONB updates for PostgreSQL CQRS architectures

<!-- CI/CD Status Badges -->
[![Tests](https://github.com/fraiseql/jsonb_ivm/actions/workflows/test.yml/badge.svg)](https://github.com/fraiseql/jsonb_ivm/actions/workflows/test.yml)
[![Lint](https://github.com/fraiseql/jsonb_ivm/actions/workflows/lint.yml/badge.svg)](https://github.com/fraiseql/jsonb_ivm/actions/workflows/lint.yml)
[![Security](https://github.com/fraiseql/jsonb_ivm/actions/workflows/security-compliance.yml/badge.svg)](https://github.com/fraiseql/jsonb_ivm/actions/workflows/security-compliance.yml)
[![Benchmark](https://github.com/fraiseql/jsonb_ivm/actions/workflows/benchmark.yml/badge.svg)](https://github.com/fraiseql/jsonb_ivm/actions/workflows/benchmark.yml)

<!-- Project Info Badges -->
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-13--18-blue.svg)](https://www.postgresql.org/)
[![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-PostgreSQL-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.1.0-green)](changelog.md)

High-performance PostgreSQL extension for incremental JSONB view maintenance in CQRS/event sourcing systems. **2-7× faster** than native SQL re-aggregation.

---

## Why jsonb_ivm?

PostgreSQL has excellent native JSONB functions (`jsonb_set`, `||`, `jsonb_agg`, etc.), but they fall short in CQRS/event sourcing architectures where you need to **surgically update denormalized projections**.

### The Problem with Native JSONB Functions

PostgreSQL's built-in JSONB functions are designed for general-purpose manipulation, not incremental view maintenance. When updating arrays or nested structures, you face:

| Native Function | Limitation |
|-----------------|------------|
| `jsonb_set()` | Requires knowing the exact array index; can't match by key |
| `\|\|` operator | Only merges top-level keys; can't update nested paths |
| `jsonb_agg()` | Re-aggregates entire array just to update one element |
| `jsonb_array_elements()` | Requires subquery + re-aggregation for every update |

**Real-world example** - Updating an order's status in a customer's orders array:

```sql
-- Native PostgreSQL: Complex, slow, memory-intensive
UPDATE customers
SET data = jsonb_set(
    data,
    '{orders}',
    (
        SELECT jsonb_agg(
            CASE
                WHEN elem->>'id' = '12345'
                THEN elem || '{"status": "shipped"}'::jsonb
                ELSE elem
            END
        )
        FROM jsonb_array_elements(data->'orders') AS elem
    )
)
WHERE id = 'cust_001';
-- Time: 3.2ms | Memory: Full array copy + aggregation overhead
```

```sql
-- With jsonb_ivm: Simple, fast, minimal allocation
UPDATE customers
SET data = jsonb_array_update_where(
    data,
    'orders',
    'id',
    '"12345"'::jsonb,
    '{"status": "shipped"}'::jsonb
)
WHERE id = 'cust_001';
-- Time: 1.1ms | Memory: In-place mutation (2.9× faster)
```

### When to Use jsonb_ivm vs Native Functions

| Use Case | Recommendation |
|----------|----------------|
| Simple key-value merge | Native `\|\|` operator is fine |
| Setting value at known path | Native `jsonb_set()` works well |
| **Updating array element by ID** | **jsonb_ivm** (2-3× faster) |
| **Batch array updates** | **jsonb_ivm** (3-5× faster) |
| **Deleting array element by ID** | **jsonb_ivm** (5-7× faster) |
| **Multi-row array updates** | **jsonb_ivm** (4× faster) |
| **Deep nested path updates** | **jsonb_ivm** (cleaner API) |

### Performance at Scale

The performance gap widens with array size:

| Array Size | Native SQL | jsonb_ivm | Speedup |
|------------|------------|-----------|---------|
| 10 elements | 0.8ms | 0.4ms | 2.0× |
| 100 elements | 6.8ms | 2.1ms | 3.2× |
| 1000 elements | 82ms | 23ms | 3.6× |

**Why?** Native SQL must:

1. Parse entire array into memory
2. Iterate and rebuild with `jsonb_agg()`
3. Serialize back to storage

jsonb_ivm uses single-pass iteration with minimal allocations.

---

## Features

- ✅ **Complete CRUD** for JSONB arrays (create, read, update, delete)
- ⚡ **2-7× faster** than native SQL re-aggregation
- 🎯 **Surgical updates** - modify only what changed
- 🛡️ **Null-safe** - graceful handling of missing paths/keys
- 🔧 **Production-ready** - extensively tested on PostgreSQL 13-18
- 📦 **Zero dependencies** - pure Rust with pgrx

---

## Quick Start

### Installation

```bash
# Build from source
git clone https://github.com/fraiseql/jsonb_ivm.git
cd jsonb_ivm
cargo install --locked cargo-pgrx
cargo pgrx init
cargo pgrx install --release
```

### Enable Extension

```sql
CREATE EXTENSION jsonb_ivm;
```

### Your First Query

```sql
-- Update array element by ID
UPDATE project_views
SET data = jsonb_smart_patch_array(
    data,
    '{"status": "completed"}'::jsonb,
    'tasks',    -- array path
    'id',       -- match key
    '5'::jsonb  -- match value
)
WHERE id = 1;
```

### Nested Path Support (v0.2.0+)

Update deeply nested fields using dot notation and array indexing:

```sql
-- Update nested field in array element
UPDATE user_views
SET data = jsonb_ivm_array_update_where_path(
    data,
    'users',                    -- array location
    'id', '123'::jsonb,        -- match user with ID 123
    'profile.settings.theme',  -- NESTED PATH to update
    '"dark"'::jsonb            -- new value
)
WHERE id = 1;

-- Set value at complex nested path
UPDATE order_views
SET data = jsonb_ivm_set_path(
    data,
    'orders[0].items[1].price',  -- Complex path
    '29.99'::jsonb
)
WHERE id = 1;
```

**Supported Syntax**:
- ✅ `field.subfield` - Object property access
- ✅ `array[0]` - Array element access
- ✅ `orders[0].items[1].price` - Combined navigation

---

## Performance

| Operation | Native SQL | jsonb_ivm | Speedup |
|-----------|-----------|-----------|---------|
| Array element update | 3.2 ms | 1.1 ms | **2.9×** |
| Array DELETE | 4.1 ms | 0.6 ms | **6.8×** |
| Batch update (10 items) | 32 ms | 6 ms | **5.2×** |
| Multi-row (100 rows) | 450 ms | 110 ms | **4.1×** |

**See**: [Performance Benchmarks](docs/PERFORMANCE.md) for detailed analysis and methodology

---

## API Overview

### Core Functions

- `jsonb_merge_shallow(target, source)` - Fast top-level merge
- `jsonb_merge_at_path(target, source, path)` - Merge at nested path
- `jsonb_deep_merge(target, source)` - Recursive deep merge

### Array Updates

- `jsonb_array_update_where(target, array_path, match_key, match_value, updates)` - Update single element
- `jsonb_ivm_array_update_where_path(target, array_key, match_key, match_value, update_path, update_value)` - Update nested field in array element
- `jsonb_array_update_where_batch(target, array_path, match_key, updates_array)` - Batch updates
- `jsonb_array_update_multi_row(targets[], array_path, match_key, match_value, updates)` - Multi-row updates

### Path Operations

- `jsonb_ivm_set_path(target, path, value)` - Set value at any nested path

### Array CRUD

- `jsonb_array_insert_where(target, array_path, element, sort_key, order)` - Sorted insertion
- `jsonb_array_delete_where(target, array_path, match_key, match_value)` - Delete element
- `jsonb_array_contains_id(data, array_path, key, value)` - Check existence

### Smart Patch Functions

- `jsonb_smart_patch_scalar(target, source)` - Intelligent shallow merge
- `jsonb_smart_patch_nested(target, source, path)` - Merge at nested path
- `jsonb_smart_patch_array(target, source, array_path, match_key, match_value)` - Update array element

**See**: [API Reference](docs/API.md) for complete function documentation with examples

---

## Documentation

- **[API Reference](docs/API.md)** - Complete function reference with examples
- **[Performance Benchmarks](docs/PERFORMANCE.md)** - Detailed benchmarks and methodology
- **[Architecture](docs/ARCHITECTURE.md)** - Design decisions and technical details
- **[Rust API Docs](https://docs.rs/jsonb_ivm)** - Generated from source code (coming soon)

---

## Use Cases

### CQRS Projection Maintenance

Update denormalized views when source entities change without re-aggregating.

### Event Sourcing

Incrementally update materialized views from event streams.

### pg_tview Integration

Optimize materialized view maintenance with surgical JSONB updates.

**See**: [Integration Guide](docs/integration-guide.md) for detailed examples

---

## Requirements

- PostgreSQL 13-18
- Rust 1.70+ (for building from source)
- pgrx 0.16.1

---

## Development

```bash
# Install task runner
cargo install just

# Run tests
just test

# Run benchmarks
just benchmark

# Format code
cargo fmt

# Lint
cargo clippy --all-targets --all-features -- -D warnings
```

**See**: [development.md](development.md) for detailed development guide

---

## Contributing

Contributions welcome! Please see:

- **[Contributing Guide](contributing.md)** - Development workflow, code standards
- **[Testing Guide](TESTING.md)** - How to run tests (why `cargo test` doesn't work)

---

## License

This project is licensed under the **PostgreSQL License** - see [LICENSE](LICENSE) for details.

---

## Changelog

See [CHANGELOG.md](changelog.md) for version history.

**Latest**: v0.1.0 - Initial release with complete JSONB CRUD operations

---

## Author

**Lionel Hamayon** - [fraiseql](https://github.com/fraiseql)

---

Built with PostgreSQL ❤️ and Rust 🦀
