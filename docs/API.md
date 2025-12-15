# jsonb_delta API Reference

> **Incremental JSONB View Maintenance for CQRS Architectures**

This document provides comprehensive documentation for all functions in the `jsonb_delta` PostgreSQL extension.

## Table of Contents

- [Core Functions](#core-functions)
  - [jsonb_merge_shallow](#jsonb_merge_shallow)
  - [jsonb_merge_at_path](#jsonb_merge_at_path)
  - [jsonb_deep_merge](#jsonb_deep_merge)
- [Array Update Operations](#array-update-operations)
  - [jsonb_array_update_where](#jsonb_array_update_where)
  - [jsonb_array_update_where_batch](#jsonb_array_update_where_batch)
  - [jsonb_array_update_multi_row](#jsonb_array_update_multi_row)
  - [jsonb_delta_array_update_where_path](#jsonb_delta_array_update_where_path)
- [Array CRUD Operations](#array-crud-operations)
  - [jsonb_array_insert_where](#jsonb_array_insert_where)
  - [jsonb_array_delete_where](#jsonb_array_delete_where)
- [Smart Patch Functions](#smart-patch-functions)
  - [jsonb_smart_patch_scalar](#jsonb_smart_patch_scalar)
  - [jsonb_smart_patch_nested](#jsonb_smart_patch_nested)
  - [jsonb_smart_patch_array](#jsonb_smart_patch_array)
- [Helper Functions](#helper-functions)
  - [jsonb_extract_id](#jsonb_extract_id)
  - [jsonb_array_contains_id](#jsonb_array_contains_id)
  - [jsonb_delta_set_path](#jsonb_delta_set_path)

---

## Core Functions

### jsonb_merge_shallow

**Signature**: `jsonb_merge_shallow(target jsonb, source jsonb) → jsonb`

**Description**: Shallow merge of two JSONB objects. Source keys overwrite target keys on conflict.

**Properties**: `IMMUTABLE STRICT PARALLEL SAFE`

**Use Case**: Fast top-level updates for materialized views when only specific fields change.

**Example**:

```sql
SELECT jsonb_merge_shallow(
    '{"name": "Alice", "age": 30, "city": "NYC"}'::jsonb,
    '{"age": 31, "country": "USA"}'::jsonb
);
-- Result: {"name": "Alice", "age": 31, "city": "NYC", "country": "USA"}
```

**See also**: [Performance benchmarks](./PERFORMANCE.md#jsonb_merge_shallow)

---

### jsonb_merge_at_path

**Signature**: `jsonb_merge_at_path(target jsonb, source jsonb, path text[]) → jsonb`

**Description**: Merge a JSONB object at a specific nested path within the target document.

**Properties**: `IMMUTABLE STRICT PARALLEL SAFE`

**Use Case**: Update nested objects without reconstructing the entire document tree.

**Example**:

```sql
SELECT jsonb_merge_at_path(
    '{"user": {"profile": {"name": "Alice", "age": 30}}}'::jsonb,
    '{"age": 31, "city": "NYC"}'::jsonb,
    ARRAY['user', 'profile']
);
-- Result: {"user": {"profile": {"name": "Alice", "age": 31, "city": "NYC"}}}
```

---

### jsonb_deep_merge

**Signature**: `jsonb_deep_merge(target jsonb, source jsonb) → jsonb`

**Description**: Recursive deep merge for complex nested updates. Preserves existing structure while updating changed fields.

**Properties**: `IMMUTABLE STRICT PARALLEL SAFE`

**Use Case**: Merge complex nested structures while preserving unmodified branches.

**Example**:

```sql
SELECT jsonb_deep_merge(
    '{"a": {"b": 1, "c": 2}, "d": 3}'::jsonb,
    '{"a": {"c": 99}, "e": 4}'::jsonb
);
-- Result: {"a": {"b": 1, "c": 99}, "d": 3, "e": 4}
```

**Note**: Arrays are replaced entirely, not merged element-by-element.

---

## Array Update Operations

### jsonb_array_update_where

**Signature**: `jsonb_array_update_where(target jsonb, array_path text, match_key text, match_value jsonb, updates jsonb) → jsonb`

**Description**: Update a single element in a JSONB array by matching a key-value predicate.

**Properties**: `IMMUTABLE STRICT PARALLEL SAFE`

**Performance**: 2-3× faster than native SQL re-aggregation.

**Use Case**: Incrementally update a single item in an array (e.g., update one order in a customer's order list).

**Example**:

```sql
SELECT jsonb_array_update_where(
    '{"orders": [{"id": 1, "status": "pending"}, {"id": 2, "status": "shipped"}]}'::jsonb,
    'orders',           -- array path
    'id',               -- match key
    '1'::jsonb,         -- match value
    '{"status": "completed", "updated_at": "2025-01-15"}'::jsonb  -- updates
);
-- Result: {"orders": [{"id": 1, "status": "completed", "updated_at": "2025-01-15"}, {"id": 2, "status": "shipped"}]}
```

**See also**: [Performance benchmarks](./PERFORMANCE.md#jsonb_array_update_where)

---

### jsonb_array_update_where_batch

**Signature**: `jsonb_array_update_where_batch(target jsonb, array_path text, match_key text, updates_array jsonb) → jsonb`

**Description**: Batch update multiple elements in a JSONB array.

**Properties**: `IMMUTABLE STRICT PARALLEL SAFE`

**Performance**: 3-5× faster than multiple separate `jsonb_array_update_where` calls.

**Use Case**: Update multiple items in an array in a single operation.

**Example**:

```sql
SELECT jsonb_array_update_where_batch(
    '{"items": [{"id": 1, "qty": 5}, {"id": 2, "qty": 10}, {"id": 3, "qty": 3}]}'::jsonb,
    'items',
    'id',
    '[
        {"id": 1, "qty": 7},
        {"id": 3, "qty": 8}
    ]'::jsonb
);
-- Result: {"items": [{"id": 1, "qty": 7}, {"id": 2, "qty": 10}, {"id": 3, "qty": 8}]}
```

**See also**: [Performance benchmarks](./PERFORMANCE.md#batch-operations)

---

### jsonb_array_update_multi_row

**Signature**: `jsonb_array_update_multi_row(targets jsonb[], array_path text, match_key text, match_value jsonb, updates jsonb) → TABLE (result jsonb)`

**Description**: Update arrays across multiple JSONB documents in one call.

**Properties**: `IMMUTABLE STRICT PARALLEL SAFE`

**Performance**: ~4× faster for 100-row batches.

**Use Case**: Bulk update operations across multiple rows (e.g., mark all pending orders as shipped).

**Example**:

```sql
SELECT * FROM jsonb_array_update_multi_row(
    ARRAY[
        '{"orders": [{"id": 1, "status": "pending"}]}'::jsonb,
        '{"orders": [{"id": 1, "status": "pending"}, {"id": 2, "status": "shipped"}]}'::jsonb
    ],
    'orders',
    'id',
    '1'::jsonb,
    '{"status": "completed"}'::jsonb
);
-- Returns two rows with updated JSONB documents
```

**See also**: [Performance benchmarks](./PERFORMANCE.md#multi-row-operations)

---

### jsonb_delta_array_update_where_path

**Signature**: `jsonb_delta_array_update_where_path(target jsonb, array_key text, match_key text, match_value jsonb, update_path text, update_value jsonb) → jsonb`

**Description**: Update a nested field in a JSONB array element using dot notation and array indexing paths.

**Properties**: `IMMUTABLE STRICT PARALLEL SAFE`

**Performance**: Same as `jsonb_array_update_where` with path navigation overhead.

**Use Case**: Update deeply nested fields in array elements (e.g., update a user's profile settings in a user array).

**Syntax**:
- Dot notation: `field.subfield` → navigate object properties
- Array indexing: `field[0]` → access array elements
- Combined: `orders[0].items[1].price` → complex nested navigation

**Example**:

```sql
-- Update nested field in array element
SELECT jsonb_delta_array_update_where_path(
    '{"users": [{"id": 1, "profile": {"name": "Alice", "settings": {"theme": "light"}}}, {"id": 2, "profile": {"name": "Bob"}}]}'::jsonb,
    'users',                    -- array location
    'id', '1'::jsonb,          -- match condition
    'profile.settings.theme',  -- NESTED PATH to update
    '"dark"'::jsonb            -- new value
);
-- Result: {"users": [{"id": 1, "profile": {"name": "Alice", "settings": {"theme": "dark"}}}, {"id": 2, "profile": {"name": "Bob"}}]}

-- Update array element within array element
SELECT jsonb_delta_array_update_where_path(
    '{"companies": [{"id": 1, "departments": [{"name": "engineering", "employees": [{"name": "Alice", "salary": 50000}]}]}]}'::jsonb,
    'companies',
    'id', '1'::jsonb,
    'departments[0].employees[0].salary',  -- Complex nested path
    '60000'::jsonb
);
-- Result: {"companies": [{"id": 1, "departments": [{"name": "engineering", "employees": [{"name": "Alice", "salary": 60000}]}]}]}
```

**Limitations**:
- Max path depth: 100 segments
- No negative indices (use PostgreSQL's `jsonb_array_length`)
- No wildcards (use PostgreSQL's `jsonb_path_query`)

**See also**: [jsonb_delta_set_path](#jsonb_delta_set_path) for general path operations

---

## Array CRUD Operations

### jsonb_array_insert_where

**Signature**: `jsonb_array_insert_where(target jsonb, array_path text, new_element jsonb, sort_key text, sort_order text) → jsonb`

**Description**: Insert element into JSONB array with optional sorting. Maintains order during incremental updates.

**Properties**: `IMMUTABLE PARALLEL SAFE`

**Parameters**:
- `sort_key`: Optional field to sort by (pass `NULL` to append)
- `sort_order`: `'ASC'` or `'DESC'` (pass `NULL` for no sorting)

**Use Case**: Add new items to arrays while maintaining sort order.

**Example**:

```sql
-- Insert with automatic sorting
SELECT jsonb_array_insert_where(
    '{"items": [{"id": 1, "price": 10}, {"id": 3, "price": 30}]}'::jsonb,
    'items',
    '{"id": 2, "price": 20}'::jsonb,
    'price',  -- sort by price
    'ASC'     -- ascending order
);
-- Result: {"items": [{"id": 1, "price": 10}, {"id": 2, "price": 20}, {"id": 3, "price": 30}]}

-- Insert without sorting (append)
SELECT jsonb_array_insert_where(
    '{"items": [{"id": 1}, {"id": 2}]}'::jsonb,
    'items',
    '{"id": 3}'::jsonb,
    NULL,
    NULL
);
-- Result: {"items": [{"id": 1}, {"id": 2}, {"id": 3}]}
```

---

### jsonb_array_delete_where

**Signature**: `jsonb_array_delete_where(target jsonb, array_path text, match_key text, match_value jsonb) → jsonb`

**Description**: Surgical array element deletion.

**Properties**: `IMMUTABLE STRICT PARALLEL SAFE`

**Performance**: 5-7× faster than re-aggregation approaches.

**Use Case**: Remove specific items from arrays without rebuilding.

**Example**:

```sql
SELECT jsonb_array_delete_where(
    '{"items": [{"id": 1, "name": "A"}, {"id": 2, "name": "B"}, {"id": 3, "name": "C"}]}'::jsonb,
    'items',
    'id',
    '2'::jsonb
);
-- Result: {"items": [{"id": 1, "name": "A"}, {"id": 3, "name": "C"}]}
```

**See also**: [Performance benchmarks](./PERFORMANCE.md#jsonb_array_delete_where)

---

## Smart Patch Functions

The "smart patch" functions provide intelligent merge behavior suitable for incremental view maintenance.

### jsonb_smart_patch_scalar

**Signature**: `jsonb_smart_patch_scalar(target jsonb, source jsonb) → jsonb`

**Description**: Intelligent shallow merge for top-level object updates. Simplifies incremental view maintenance logic.

**Properties**: `IMMUTABLE STRICT PARALLEL SAFE`

**Use Case**: Apply partial updates to materialized view rows.

**Example**:

```sql
SELECT jsonb_smart_patch_scalar(
    '{"name": "Alice", "age": 30, "status": "active"}'::jsonb,
    '{"age": 31}'::jsonb
);
-- Result: {"name": "Alice", "age": 31, "status": "active"}
```

---

### jsonb_smart_patch_nested

**Signature**: `jsonb_smart_patch_nested(target jsonb, source jsonb, path text[]) → jsonb`

**Description**: Merge JSONB at nested path within document. Array-based path specification for flexible updates.

**Properties**: `IMMUTABLE STRICT PARALLEL SAFE`

**Use Case**: Update nested structures in materialized views.

**Example**:

```sql
SELECT jsonb_smart_patch_nested(
    '{"user": {"profile": {"name": "Alice", "verified": false}}}'::jsonb,
    '{"verified": true}'::jsonb,
    ARRAY['user', 'profile']
);
-- Result: {"user": {"profile": {"name": "Alice", "verified": true}}}
```

---

### jsonb_smart_patch_array

**Signature**: `jsonb_smart_patch_array(target jsonb, source jsonb, array_path text, match_key text, match_value jsonb) → jsonb`

**Description**: Update specific element within JSONB array. Combines search and update in single operation.

**Properties**: `IMMUTABLE STRICT PARALLEL SAFE`

**Use Case**: Targeted updates to array elements based on predicates.

**Example**:

```sql
SELECT jsonb_smart_patch_array(
    '{"items": [{"id": 1, "qty": 5}, {"id": 2, "qty": 10}]}'::jsonb,
    '{"qty": 15, "updated": true}'::jsonb,
    'items',
    'id',
    '2'::jsonb
);
-- Result: {"items": [{"id": 1, "qty": 5}, {"id": 2, "qty": 15, "updated": true}]}
```

---

## Helper Functions

### jsonb_extract_id

**Signature**: `jsonb_extract_id(data jsonb, key text DEFAULT 'id') → text`

**Description**: Extract ID values from JSONB objects. Defaults to "id" key for convenience.

**Properties**: `IMMUTABLE STRICT PARALLEL SAFE`

**Use Case**: Extract identifiers for joins or lookups.

**Example**:

```sql
SELECT jsonb_extract_id('{"id": "user_123", "name": "Alice"}'::jsonb);
-- Result: "user_123"

SELECT jsonb_extract_id('{"uuid": "abc-def-ghi", "name": "Bob"}'::jsonb, 'uuid');
-- Result: "abc-def-ghi"
```

---

### jsonb_array_contains_id

**Signature**: `jsonb_array_contains_id(data jsonb, array_path text, id_key text, id_value jsonb) → bool`

**Description**: Fast existence checking for array elements. Optimized for view maintenance workflows.

**Properties**: `IMMUTABLE STRICT PARALLEL SAFE`

**Use Case**: Check if an array contains an element with a specific ID before inserting.

**Example**:

```sql
SELECT jsonb_array_contains_id(
    '{"items": [{"id": 1, "name": "A"}, {"id": 2, "name": "B"}]}'::jsonb,
    'items',
    'id',
    '2'::jsonb
);
-- Result: true

SELECT jsonb_array_contains_id(
    '{"items": [{"id": 1, "name": "A"}, {"id": 2, "name": "B"}]}'::jsonb,
    'items',
    'id',
    '99'::jsonb
);
-- Result: false
```

---

### jsonb_delta_set_path

**Signature**: `jsonb_delta_set_path(target jsonb, path text, value jsonb) → jsonb`

**Description**: Set a value at any nested path in a JSONB document using dot notation and array indexing.

**Properties**: `IMMUTABLE STRICT PARALLEL SAFE`

**Use Case**: General-purpose path-based updates for complex JSONB structures.

**Syntax**: Same as `jsonb_delta_array_update_where_path` - supports dot notation and array indexing.

**Example**:

```sql
-- Set nested object field
SELECT jsonb_delta_set_path(
    '{"user": {"profile": {"settings": {}}}}'::jsonb,
    'user.profile.settings.theme',
    '"dark"'::jsonb
);
-- Result: {"user": {"profile": {"settings": {"theme": "dark"}}}}

-- Set array element
SELECT jsonb_delta_set_path(
    '{"items": []}'::jsonb,
    'items[0]',
    '{"name": "first item", "qty": 1}'::jsonb
);
-- Result: {"items": [{"name": "first item", "qty": 1}]}

-- Complex nested path
SELECT jsonb_delta_set_path(
    '{"orders": [{"items": [{}]}]}'::jsonb,
    'orders[0].items[0].price',
    '29.99'::jsonb
);
-- Result: {"orders": [{"items": [{"price": 29.99}]}]}
```

**Note**: Creates intermediate objects/arrays as needed. Use with caution as it may create deeply nested structures.

**See also**: [jsonb_delta_array_update_where_path](#jsonb_delta_array_update_where_path) for array-specific updates

---

## Performance Considerations

All functions in this extension are marked as:
- **IMMUTABLE**: Same inputs always produce same outputs (enables caching and query optimization)
- **STRICT**: Returns NULL if any input is NULL (PostgreSQL can short-circuit evaluation)
- **PARALLEL SAFE**: Can be safely executed in parallel query plans

For detailed performance benchmarks and comparisons with native PostgreSQL operations, see [PERFORMANCE.md](./PERFORMANCE.md).

## Error Handling

All functions validate their inputs and will raise PostgreSQL errors for:
- Invalid JSONB structure (when expecting objects but receiving arrays/scalars)
- Invalid path specifications
- Type mismatches

Example error:

```sql
SELECT jsonb_merge_shallow('"not an object"'::jsonb, '{}'::jsonb);
-- ERROR: target argument must be a JSONB object, got String
```

## See Also

- [PERFORMANCE.md](./PERFORMANCE.md) - Detailed performance benchmarks
- [ARCHITECTURE.md](./ARCHITECTURE.md) - Design decisions and implementation details
- [README.md](../README.md) - Quick start guide and overview
- [Rust API Documentation](https://docs.rs/jsonb_delta) - Generated from source code
