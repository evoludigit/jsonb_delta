# jsonb_delta

Efficient JSONB delta and patch operations for PostgreSQL

<!-- CI/CD Status Badges -->
[![Tests](https://github.com/evoludigit/jsonb_delta/actions/workflows/test.yml/badge.svg)](https://github.com/evoludigit/jsonb_delta/actions/workflows/test.yml)
[![Lint](https://github.com/evoludigit/jsonb_delta/actions/workflows/lint.yml/badge.svg)](https://github.com/evoludigit/jsonb_delta/actions/workflows/lint.yml)
[![Security](https://github.com/evoludigit/jsonb_delta/actions/workflows/security-compliance.yml/badge.svg)](https://github.com/evoludigit/jsonb_delta/actions/workflows/security-compliance.yml)
[![Benchmark](https://github.com/evoludigit/jsonb_delta/actions/workflows/benchmark.yml/badge.svg)](https://github.com/evoludigit/jsonb_delta/actions/workflows/benchmark.yml)

<!-- Project Info Badges -->
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-13--18-blue.svg)](https://www.postgresql.org/)
[![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/License-PostgreSQL-blue.svg)](LICENSE)
[![Version](https://img.shields.io/badge/version-0.1.0-green)](changelog.md)
[![Release](https://img.shields.io/github/v/release/evoludigit/jsonb_delta)](https://github.com/evoludigit/jsonb_delta/releases)

A PostgreSQL extension providing fast, targeted update primitives for JSONB documents, enabling merge, nested patch, and array manipulation operations that go beyond the capabilities of built-in functions.

---

## Why jsonb_delta?

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
-- With jsonb_delta: Simple, fast, minimal allocation
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

### When to Use jsonb_delta vs Native Functions

| Use Case | Recommendation |
|----------|----------------|
| Simple key-value merge | Native `\|\|` operator is fine |
| Setting value at known path | Native `jsonb_set()` works well |
| **Updating array element by ID** | **jsonb_delta** (2-3× faster) |
| **Batch array updates** | **jsonb_delta** (3-5× faster) |
| **Deleting array element by ID** | **jsonb_delta** (5-7× faster) |
| **Multi-row array updates** | **jsonb_delta** (4× faster) |
| **Deep nested path updates** | **jsonb_delta** (cleaner API) |

### Performance at Scale

The performance gap widens with array size:

| Array Size | Native SQL | jsonb_delta | Speedup |
|------------|------------|-----------|---------|
| 10 elements | 0.8ms | 0.4ms | 2.0× |
| 100 elements | 6.8ms | 2.1ms | 3.2× |
| 1000 elements | 82ms | 23ms | 3.6× |

**Why?** Native SQL must:

1. Parse entire array into memory
2. Iterate and rebuild with `jsonb_agg()`
3. Serialize back to storage

jsonb_delta uses single-pass iteration with minimal allocations.

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
git clone https://github.com/evoludigit/jsonb_delta.git
cd jsonb_delta
cargo install --locked cargo-pgrx
cargo pgrx init
cargo pgrx install --release
```

### Enable Extension

```sql
CREATE EXTENSION jsonb_delta;
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

## 📖 Usage Examples

### E-commerce: Update Order Status

**Before** (Native PostgreSQL - slow and complex):
```sql
UPDATE customers
SET data = jsonb_set(
    data,
    '{orders}',
    (
        SELECT jsonb_agg(
            CASE
                WHEN elem->>'id' = 'ORD-123'
                THEN elem || '{"status": "shipped", "shipped_at": "2025-01-15T10:30:00Z"}'::jsonb
                ELSE elem
            END
        )
        FROM jsonb_array_elements(data->'orders') AS elem
    )
)
WHERE id = 'CUST-456';
-- Time: ~3.2ms | Memory: Full array reconstruction
```

**After** (with jsonb_delta - fast and simple):
```sql
UPDATE customers
SET data = jsonb_delta_array_update_where_path(
    data,
    'orders',                          -- array path
    'id', 'ORD-123'::jsonb,           -- match order by ID
    'status', '"shipped"'::jsonb,     -- update status
    'shipped_at', '"2025-01-15T10:30:00Z"'::jsonb  -- add timestamp
)
WHERE id = 'CUST-456';
-- Time: ~1.1ms | Memory: In-place mutation (2.9× faster)
```

### CQRS: Maintain Denormalized Views

**Scenario**: Update user profile in multiple views when user changes their email.

```sql
-- Update user profile view
UPDATE user_profiles
SET data = jsonb_delta_set_path(
    data,
    'email',
    '"new.email@example.com"'::jsonb
)
WHERE user_id = 'USER-789';

-- Update user search index (denormalized view)
UPDATE user_search
SET data = jsonb_merge_shallow(
    data,
    '{"email": "new.email@example.com", "last_updated": "2025-01-15T10:30:00Z"}'::jsonb
)
WHERE user_id = 'USER-789';

-- Update user permissions cache
UPDATE user_permissions
SET data = jsonb_delta_array_update_where_path(
    data,
    'users',
    'id', 'USER-789'::jsonb,
    'email', '"new.email@example.com"'::jsonb
)
WHERE organization_id = 'ORG-123';
```

### Event Sourcing: Apply Events to Projections

```sql
-- Function to apply "item_added_to_cart" event
CREATE OR REPLACE FUNCTION apply_cart_item_added(
    projection jsonb,
    event_data jsonb
) RETURNS jsonb AS $$
BEGIN
    RETURN jsonb_delta_array_update_where_path(
        projection,
        'items',
        'product_id', event_data->'product_id',
        'quantity', event_data->'quantity',
        'added_at', event_data->'timestamp'
    );
END;
$$ LANGUAGE plpgsql;

-- Apply event to cart projection
UPDATE cart_projections
SET data = apply_cart_item_added(
    data,
    '{"product_id": "PROD-456", "quantity": 2, "timestamp": "2025-01-15T10:30:00Z"}'::jsonb
)
WHERE cart_id = 'CART-789';
```

### Real-time Analytics: Update Counters

```sql
-- Update page view counters (high-frequency updates)
UPDATE page_analytics
SET data = jsonb_merge_shallow(
    data,
    jsonb_build_object(
        'total_views', (data->>'total_views')::int + 1,
        'unique_visitors', GREATEST(
            (data->>'unique_visitors')::int,
            CASE WHEN NOT(data ? 'visitor_ids') OR NOT(data->'visitor_ids' ? visitor_id)
                 THEN (data->>'unique_visitors')::int + 1
                 ELSE (data->>'unique_visitors')::int
            END
        ),
        'last_updated', extract(epoch from now())::text
    )
)
WHERE page_id = 'PAGE-123';
-- Note: Use jsonb_delta_set_path for better performance on large objects

### Nested Path Support (v0.2.0+)

Update deeply nested fields using dot notation and array indexing:

```sql
-- Update nested field in array element
UPDATE user_views
SET data = jsonb_delta_array_update_where_path(
    data,
    'users',                    -- array location
    'id', '123'::jsonb,        -- match user with ID 123
    'profile.settings.theme',  -- NESTED PATH to update
    '"dark"'::jsonb            -- new value
)
WHERE id = 1;

-- Set value at complex nested path
UPDATE order_views
SET data = jsonb_delta_set_path(
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

### Benchmark Results

| Operation | Native SQL | jsonb_delta | Speedup |
|-----------|-----------|-----------|---------|
| Array element update | 3.2 ms | 1.1 ms | **2.9×** |
| Array DELETE | 4.1 ms | 0.6 ms | **6.8×** |
| Batch update (10 items) | 32 ms | 6 ms | **5.2×** |
| Multi-row (100 rows) | 450 ms | 110 ms | **4.1×** |

### Performance Scaling by Array Size

```
Array Size: 10 elements
├── Native SQL: 0.8ms
└── jsonb_delta: 0.4ms (2.0× faster)

Array Size: 100 elements
├── Native SQL: 6.8ms
└── jsonb_delta: 2.1ms (3.2× faster)

Array Size: 1000 elements
├── Native SQL: 82ms
└── jsonb_delta: 23ms (3.6× faster)

Array Size: 10000 elements
├── Native SQL: 1.2s
└── jsonb_delta: 180ms (6.7× faster)
```

### Memory Efficiency

- **Native SQL**: Full array reconstruction + aggregation overhead
- **jsonb_delta**: Single-pass iteration with minimal allocations
- **Memory Savings**: Up to 90% reduction in temporary memory usage

### PostgreSQL Version Compatibility

| PostgreSQL Version | Status | Performance |
|-------------------|--------|-------------|
| 18.x (latest) | ✅ Full Support | Optimal |
| 17.x | ✅ Full Support | Optimal |
| 16.x | ✅ Full Support | Optimal |
| 15.x | ✅ Full Support | Optimal |
| 14.x | ✅ Full Support | Optimal |
| 13.x | ✅ Full Support | Optimal |

### System Requirements

- **Memory**: 2GB RAM minimum, 4GB recommended
- **Storage**: ~50MB for extension files
- **Dependencies**: None (pure Rust, zero external deps)

**📊 See**: [Performance Benchmarks](docs/PERFORMANCE.md) for detailed analysis, methodology, and raw data

---

## API Overview

### Core Functions

- `jsonb_merge_shallow(target, source)` - Fast top-level merge
- `jsonb_merge_at_path(target, source, path)` - Merge at nested path
- `jsonb_deep_merge(target, source)` - Recursive deep merge

### Array Updates

- `jsonb_array_update_where(target, array_path, match_key, match_value, updates)` - Update single element
- `jsonb_delta_array_update_where_path(target, array_key, match_key, match_value, update_path, update_value)` - Update nested field in array element
- `jsonb_array_update_where_batch(target, array_path, match_key, updates_array)` - Batch updates
- `jsonb_array_update_multi_row(targets[], array_path, match_key, match_value, updates)` - Multi-row updates

### Path Operations

- `jsonb_delta_set_path(target, path, value)` - Set value at any nested path

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
- **[Rust API Docs](https://docs.rs/jsonb_delta)** - Generated from source code (coming soon)

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

**Lionel Hamayon** - [evoludigit](https://github.com/evoludigit)

---

Built with PostgreSQL ❤️ and Rust 🦀
