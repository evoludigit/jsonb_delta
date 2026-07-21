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

## 🍓 Part of the FraiseQL Ecosystem

**jsonb_delta** powers CQRS projection updates across the FraiseQL stack:

### **Server Stack (PostgreSQL + Python/Rust)**

| Tool | Purpose | Status | Performance Gain |
|------|---------|--------|------------------|
| **[pg_tviews](https://github.com/fraiseql/pg_tviews)** | Incremental materialized views | Beta | **100-500× faster** |
| **[jsonb_delta](https://github.com/evoludigit/jsonb_delta)** | JSONB surgical updates | **Stable** ⭐ | **1.6–3.9×; up to ~21× coalesced** |
| **[pgGit](https://github.com/evoludigit/pgGit)** | Database version control | Stable | Git for databases |
| **[confiture](https://github.com/fraiseql/confiture)** | PostgreSQL migrations | Stable | **300-600× faster** |
| **[fraiseql](https://fraiseql.dev)** | GraphQL framework | Stable | **7-10× faster** |
| **[fraiseql-data](https://github.com/fraiseql/fraiseql-seed)** | Seed data generation | Phase 6 | Auto-dependency resolution |

### **Client Libraries (TypeScript/JavaScript)**

| Library | Purpose | Framework Support |
|---------|---------|-------------------|
| **[graphql-cascade](https://github.com/graphql-cascade/graphql-cascade)** | Automatic cache invalidation | Apollo, React Query, Relay, URQL |

**How jsonb_delta fits:**
- **pg_tviews** uses jsonb_delta for 1.6–3.9× faster surgical JSONB updates
- **fraiseql** GraphQL mutations update JSONB projections surgically
- **graphql-cascade** (client-side) handles browser cache invalidation

**CQRS workflow:**
```sql
-- Update source table
UPDATE tb_order SET status = 'shipped' WHERE id = 'ORD-123';

-- jsonb_delta surgically updates denormalized view (2.9× faster)
UPDATE customer_projections
SET data = jsonb_delta_array_update_where_path(
    data,
    'orders', 'id', 'ORD-123'::jsonb,
    'status', '"shipped"'::jsonb
)
WHERE customer_id = 'CUST-456';
```

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
| **Updating array element by ID** | **jsonb_delta** (1.6–3.9× faster) |
| **Deleting array element by ID** | **jsonb_delta** (1.6–3.8× faster) |
| **Coalescing many edits in one call** | **jsonb_delta** `jsonb_apply_changeset` (up to ~21× vs chaining) |
| **Batch array updates** | **jsonb_delta** (parity at small N, wins as N grows) |
| **Multi-row array updates** | **jsonb_delta** (one call over many documents) |
| **Deep nested path updates** | **jsonb_delta** (cleaner API) |

### Performance at Scale

The win holds up as arrays grow (single-element update, in-memory, integer key;
measured on Hetzner CCX13 / PostgreSQL 17.10 — see
[benchmarks](benchmarks/2026-07-21-issue15-ccx13.md)):

| Array Size | Native re-aggregation | jsonb_delta | Speedup |
|------------|-----------------------|-------------|---------|
| 10 elements | 0.09 ms | 0.04 ms | 2.1× |
| 100 elements | 0.18 ms | 0.07 ms | 2.5× |
| 1000 elements | 2.42 ms | 1.03 ms | 2.4× |

**Why?** Native SQL must:

1. Parse entire array into memory
2. Iterate and rebuild with `jsonb_agg()`
3. Serialize back to storage

jsonb_delta uses single-pass iteration with minimal allocations.

---

## Features

- ✅ **Complete CRUD** for JSONB arrays (create, read, update, delete)
- ⚡ **1.6–3.9× faster** than native SQL re-aggregation for surgical update/delete ([benchmarks](benchmarks/2026-07-21-issue15-ccx13.md))
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

Measured on a rentable, reproducible profile — Hetzner CCX13 (dedicated 2 vCPU,
AMD EPYC-Milan), PostgreSQL 17.10, release build, `pg_tviews`-free. Full artifact
with raw per-trial timings:
[`benchmarks/2026-07-21-issue15-ccx13.md`](benchmarks/2026-07-21-issue15-ccx13.md).
Ratios are jsonb_delta vs the native SQL baseline (`jsonb_set` + `jsonb_agg`
re-aggregation, or `||` for merge); > 1 means jsonb_delta is faster.

| Operation | Native | jsonb_delta | Speedup |
|-----------|--------|-------------|---------|
| Update 1 element (1000-elem array, in-memory) | 2.42 ms | 1.03 ms | **2.4×** |
| Delete 1 element (1000-elem array, in-memory) | 2.21 ms | 0.58 ms | **3.8×** |
| Shallow merge vs `\|\|` | 0.27 ms | 0.26 ms | **~1.0× (parity)** |
| Coalesce 50 edits vs chaining (500-elem doc) | 23.5 ms | 1.1 ms | **21×** |

Where jsonb_delta is at parity or slower — shallow merge vs `||`, one- or two-op
changesets, small batches — the artifact says so explicitly rather than omitting it.

### Performance Scaling by Array Size

Single-element update, in-memory, integer key (CCX13). The win holds up as arrays
grow, rather than collapsing — which the serde v0.2.0 build did (see [#15](https://github.com/evoludigit/jsonb_delta/issues/15)):

```
Array Size: 10 elements
├── Native re-aggregation: 0.09ms
└── jsonb_delta:           0.04ms (2.1× faster)

Array Size: 100 elements
├── Native re-aggregation: 0.18ms
└── jsonb_delta:           0.07ms (2.5× faster)

Array Size: 1000 elements
├── Native re-aggregation: 2.42ms
└── jsonb_delta:           1.03ms (2.4× faster)
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

### Coalesced Changeset

- `jsonb_apply_changeset(doc, ops)` - Apply an **ordered list of edits in one pass**. `ops` is a
  JSONB array of typed operations (`set`, `remove`, `merge`, `deep_merge`, `increment`,
  `array_update`, `array_update_all`, `array_replace`, `array_upsert`, `array_delete`,
  `array_insert`). Paths may be dot-notation strings or segment arrays; array matching supports
  int, text, and UUID keys.

  This is the function to reach for when a single denormalized row needs **many** changes at once
  (the incremental-view-maintenance case). Because the whole-document (de)serialization is paid
  **once for the entire changeset** instead of once per edit, coalescing N edits into one call
  avoids the per-edit reserialization that chaining several `jsonb_smart_patch_*` calls incurs —
  and the advantage grows with the number of coalesced edits. For a *single* edit there is no
  coalescing benefit; prefer the dedicated functions above. (Quantified speedups pending
  measurement under the project's benchmark methodology; see `test/benchmark_changeset.sql`.)

  ```sql
  SELECT jsonb_apply_changeset(data, '[
    {"op": "array_upsert", "path": "posts", "match_key": "id", "match_value": "3f2a-uuid",
     "value": {"id": "3f2a-uuid", "title": "Edited"}, "sort_key": "created_at", "sort_order": "DESC"},
    {"op": "array_delete", "path": "posts", "match_key": "id", "match_value": 7},
    {"op": "deep_merge", "path": ["author", "stats"], "value": {"posts": 12}},
    {"op": "increment", "path": "stats.post_count", "by": 1},
    {"op": "remove", "path": "stats.stale_cache"}
  ]'::jsonb)
  FROM tv_feed WHERE ...;
  ```

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
- pgrx 0.17.0

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
