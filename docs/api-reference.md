# API Reference

Complete reference for all jsonb_delta functions.

## Function Index

### Smart Patch Functions

- [jsonb_smart_patch_scalar](#jsonb_smart_patch_scalar) - Intelligent shallow merge

### Array CRUD Functions

- [jsonb_array_delete_where](#jsonb_array_delete_where) - Delete array element
- [jsonb_array_insert_where](#jsonb_array_insert_where) - Insert sorted element
- [jsonb_array_contains_id](#jsonb_array_contains_id) - Check if array contains element

### Deep Merge

- [jsonb_deep_merge](#jsonb_deep_merge) - Recursive deep merge

### Utility Functions

- [jsonb_extract_id](#jsonb_extract_id) - Safely extract ID field

### Core Functions

- [jsonb_array_update_where](#jsonb_array_update_where) - Single element update
- [jsonb_array_update_where_batch](#jsonb_array_update_where_batch) - Batch updates
- [jsonb_array_update_multi_row](#jsonb_array_update_multi_row) - Multi-row updates
- [jsonb_merge_at_path](#jsonb_merge_at_path) - Merge at path
- [jsonb_merge_shallow](#jsonb_merge_shallow) - Shallow merge

---

## jsonb_smart_patch_scalar

Intelligently merge source JSONB into target at the top level.

### jsonb_smart_patch_scalar - Signature

```sql
jsonb_smart_patch_scalar(target jsonb, source jsonb) RETURNS jsonb
```

### jsonb_smart_patch_scalar - Behavior

- Finds array at `array_path`
- Searches for element where `element[match_key] = match_value`
- Applies smart patch to that element only
- Returns original if no match found (graceful)

### jsonb_smart_patch_scalar - Performance

- Walks the binary jsonb form directly instead of round-tripping through a text serialization
- Optimized integer ID matching with SIMD-friendly loops
- Not separately benchmarked against native SQL; for measured array update/delete ratios (1.6–3.9× vs re-aggregation) see [../benchmarks/2026-07-21-issue15-ccx13.md](../benchmarks/2026-07-21-issue15-ccx13.md)

### jsonb_smart_patch_scalar - Examples

**Update task status**:

```sql
SELECT jsonb_smart_patch_array(
    '{"tasks": [
        {"id": 1, "name": "Task 1", "status": "pending"},
        {"id": 2, "name": "Task 2", "status": "pending"}
    ]}'::jsonb,
    '{"status": "completed"}'::jsonb,
    'tasks',
    'id',
    '2'::jsonb
);
-- Result: task #2 status changed to "completed"
```

**Update nested array**:

```sql
SELECT jsonb_smart_patch_array(
    '{"project": {"members": [
        {"user_id": 10, "role": "viewer"},
        {"user_id": 20, "role": "editor"}
    ]}}'::jsonb,
    '{"role": "admin"}'::jsonb,
    'members',
    'user_id',
    '20'::jsonb
);
-- Result: user_id 20 role changed to "admin"
```

**Real-world usage**:

```sql
-- Update post title in feed projection
UPDATE tv_feed
SET data = jsonb_smart_patch_array(
    data,
    '{"title": "Updated Post Title"}'::jsonb,
    'posts',
    'id',
    '"abc-123"'::jsonb
)
WHERE pk_feed = 1;
```

---

## jsonb_array_delete_where

Delete a specific element from a JSONB array by matching a key-value pair.

### jsonb_array_delete_where - Signature

```sql
jsonb_array_delete_where(
    target jsonb,
    array_path text,
    match_key text,
    match_value jsonb
) RETURNS jsonb
```

### jsonb_array_delete_where - Behavior

- Finds array at `array_path`
- Removes element where `element[match_key] = match_value`
- Returns original if no match found (graceful)

### jsonb_array_delete_where - Performance

- **1.6–3.8× faster** than SQL re-aggregation for a single-element delete, rising with array size (measured; see [../benchmarks/2026-07-21-issue15-ccx13.md](../benchmarks/2026-07-21-issue15-ccx13.md))
- Single-pass array reconstruction

### jsonb_array_delete_where - Examples

**Delete task by ID**:

```sql
SELECT jsonb_array_delete_where(
    '{"tasks": [
        {"id": 1, "name": "Task 1"},
        {"id": 2, "name": "Task 2"},
        {"id": 3, "name": "Task 3"}
    ]}'::jsonb,
    'tasks',
    'id',
    '2'::jsonb
);
-- Result: task #2 removed, tasks #1 and #3 remain
```

**No match** (graceful):

```sql
SELECT jsonb_array_delete_where(
    '{"tasks": [{"id": 1}]}'::jsonb,
    'tasks',
    'id',
    '999'::jsonb
);
-- Result: {"tasks": [{"id": 1}]} (unchanged)
```

**Real-world usage**:

```sql
-- Delete post from feed projection
UPDATE tv_feed
SET data = jsonb_array_delete_where(
    data,
    'posts',
    'id',
    '"post-to-delete"'::jsonb
)
WHERE pk_feed = 1;
```

---

## jsonb_array_insert_where

Insert an element into a JSONB array at the correct position based on sorting.

### jsonb_array_insert_where - Signature

```sql
jsonb_array_insert_where(
    target jsonb,
    array_path text,
    new_element jsonb,
    sort_key text,
    sort_order text  -- 'ASC' or 'DESC'
) RETURNS jsonb
```

### jsonb_array_insert_where - Behavior

- Finds array at `array_path`
- Inserts `new_element` at position maintaining sort order by `sort_key`
- Supports both numeric and text sorting
- If `sort_key` is NULL, appends to end (unsorted insertion)
- Returns original if path invalid (graceful)

### jsonb_array_insert_where - Performance

- Rebuilds only the changed array spine instead of re-aggregating the whole array
- Optimized insertion point search (binary search on sorted arrays)
- Insert is not in the measured matrix; for the measured update/delete ratios (1.6–3.9× vs re-aggregation) see [../benchmarks/2026-07-21-issue15-ccx13.md](../benchmarks/2026-07-21-issue15-ccx13.md)

### jsonb_array_insert_where - Examples

**Insert task sorted by priority (ascending)**:

```sql
SELECT jsonb_array_insert_where(
    '{"tasks": [
        {"id": 1, "priority": 1},
        {"id": 3, "priority": 3}
    ]}'::jsonb,
    'tasks',
    '{"id": 2, "priority": 2}'::jsonb,
    'priority',
    'ASC'
);
-- Result: task #2 inserted between #1 and #3
```

**Insert sorted by name (descending)**:

```sql
SELECT jsonb_array_insert_where(
    '{"users": [
        {"name": "Charlie"},
        {"name": "Alice"}
    ]}'::jsonb,
    'users',
    '{"name": "Bob"}'::jsonb,
    'name',
    'DESC'
);
-- Result: Bob inserted between Charlie and Alice
```

**Unsorted insertion** (append to end):

```sql
SELECT jsonb_array_insert_where(
    '{"items": [1, 2, 3]}'::jsonb,
    'items',
    '4'::jsonb,
    NULL,  -- no sorting
    NULL
);
-- Result: {"items": [1, 2, 3, 4]}
```

**Real-world usage**:

```sql
-- Insert new post in feed, sorted by created_at descending
UPDATE tv_feed
SET data = jsonb_array_insert_where(
    data,
    'posts',
    '{"id": "new-post", "title": "New Post", "created_at": "2025-12-08"}'::jsonb,
    'created_at',
    'DESC'
)
WHERE pk_feed = 1;
```

---

## jsonb_array_contains_id

Check if a JSONB array contains an element with a specific key-value pair.

### Signature

```sql
jsonb_array_contains_id(
    data jsonb,
    array_path text,
    id_key text,
    id_value jsonb
) RETURNS boolean
```

### Behavior

- Finds array at `array_path`
- Returns `true` if any element has `element[id_key] = id_value`
- Returns `false` if no match or path invalid

### Performance

- Optimized linear search with early exit

### Examples

**Check if task exists**:

```sql
SELECT jsonb_array_contains_id(
    '{"tasks": [{"id": 1}, {"id": 2}]}'::jsonb,
    'tasks',
    'id',
    '2'::jsonb
);
-- Result: true
```

**Element doesn't exist**:

```sql
SELECT jsonb_array_contains_id(
    '{"tasks": [{"id": 1}]}'::jsonb,
    'tasks',
    'id',
    '999'::jsonb
);
-- Result: false
```

**Real-world usage**:

```sql
-- Find feeds that contain a specific post
SELECT pk FROM tv_feed
WHERE jsonb_array_contains_id(
    data,
    'posts',
    'id',
    '"abc-123"'::jsonb
);
```

---

## jsonb_deep_merge

Recursively merge two JSONB objects, deeply merging nested objects.

### jsonb_deep_merge - Signature

```sql
jsonb_deep_merge(target jsonb, source jsonb) RETURNS jsonb
```

### jsonb_deep_merge - Behavior

- Recursively merges all nested objects
- Arrays/scalars in source replace target values
- NULL handling: Returns NULL if either input is NULL (STRICT)

### jsonb_deep_merge - Performance

- **At parity** with the native `||` operator (both walk jsonb's binary form); the value is deep (recursive) merge semantics `||` does not provide, not raw speed. Measured parity in [../benchmarks/2026-07-21-issue15-ccx13.md](../benchmarks/2026-07-21-issue15-ccx13.md)
- Optimized recursive algorithm

### jsonb_deep_merge - Examples

**Deep merge nested objects**:

```sql
SELECT jsonb_deep_merge(
    '{"a": {"b": {"c": 1, "d": 2}}, "x": 10}'::jsonb,
    '{"a": {"b": {"c": 99}}, "y": 20}'::jsonb
);
-- Result: {"a": {"b": {"c": 99, "d": 2}}, "x": 10, "y": 20}
-- Note: "d": 2 is preserved (deep merge)
```

**Compare with shallow merge**:

```sql
-- Shallow merge (loses nested fields)
SELECT '{"a": {"b": {"c": 1, "d": 2}}}'::jsonb || '{"a": {"b": {"c": 99}}}'::jsonb;
-- Result: {"a": {"b": {"c": 99}}} -- "d" is LOST!

-- Deep merge (preserves nested fields)
SELECT jsonb_deep_merge(
    '{"a": {"b": {"c": 1, "d": 2}}}'::jsonb,
    '{"a": {"b": {"c": 99}}}'::jsonb
);
-- Result: {"a": {"b": {"c": 99, "d": 2}}} -- "d" is PRESERVED!
```

**Arrays are replaced, not merged**:

```sql
SELECT jsonb_deep_merge(
    '{"tags": ["old1", "old2"]}'::jsonb,
    '{"tags": ["new"]}'::jsonb
);
-- Result: {"tags": ["new"]}
```

---

## jsonb_extract_id

Safely extract an ID field from JSONB data with type validation.

### jsonb_extract_id - Signature

```sql
jsonb_extract_id(data jsonb, key text DEFAULT 'id') RETURNS text
```

### jsonb_extract_id - Behavior

- Extracts `data[key]` as text
- Validates it's a number or string
- Returns NULL if invalid type or missing

### jsonb_extract_id - Examples

**Extract integer ID**:

```sql
SELECT jsonb_extract_id('{"id": 42}'::jsonb);
-- Result: '42'
```

**Extract UUID**:

```sql
SELECT jsonb_extract_id('{"uuid": "123e4567-e89b"}'::jsonb, 'uuid');
-- Result: '123e4567-e89b'
```

**Invalid type** (graceful):

```sql
SELECT jsonb_extract_id('{"id": {"nested": "object"}}'::jsonb);
-- Result: NULL
```

**Real-world usage**:

```sql
-- Extract post ID for processing
SELECT jsonb_extract_id(data, 'post_id')
FROM tv_feed
WHERE pk_feed = 1;
```

---

## Legacy Functions Reference (v0.1.0-v0.2.0)

The following functions provide core functionality. For complex updates, consider using the smart patch functions above.

### jsonb_array_update_where

Updates a single element in a JSONB array by matching a key-value predicate.

**Signature**:

```sql
jsonb_array_update_where(
    target jsonb,
    array_path text,
    match_key text,
    match_value jsonb,
    updates jsonb
) RETURNS jsonb
```

**Performance**: O(n) where n = array length. Measured **1.7–2.5×** faster than native SQL re-aggregation for integer keys (up to ~3.9× for text keys), and the ratio **rises** with array size rather than falling. See [../benchmarks/2026-07-21-issue15-ccx13.md](../benchmarks/2026-07-21-issue15-ccx13.md).

**Example**:

```sql
SELECT jsonb_array_update_where(
    '{"dns_servers": [{"id": 42, "ip": "1.1.1.1"}, {"id": 43, "ip": "2.2.2.2"}]}'::jsonb,
    'dns_servers',
    'id',
    '42'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb
);
-- Result: {"dns_servers": [{"id": 42, "ip": "8.8.8.8"}, {"id": 43, "ip": "2.2.2.2"}]}
```

---

### jsonb_array_update_where_batch

Batch update multiple elements in a JSONB array in a single pass.

**Signature**:

```sql
jsonb_array_update_where_batch(
    target jsonb,
    array_path text,
    match_key text,
    updates_array jsonb
) RETURNS jsonb
```

**Performance**: one pass over the array instead of one pass per update. Measured at parity (~1.07×) with native re-aggregation for 10 updates in a 100-element array; the current implementation scans the spec list per element (O(elements × specs)), so `jsonb_apply_changeset` overtakes it at high update counts. See [../benchmarks/2026-07-21-issue15-ccx13.md](../benchmarks/2026-07-21-issue15-ccx13.md).

**Example**:

```sql
SELECT jsonb_array_update_where_batch(
    '{"dns_servers": [{"id": 1}, {"id": 2}, {"id": 3}]}'::jsonb,
    'dns_servers',
    'id',
    '[
        {"match_value": 1, "updates": {"ip": "1.1.1.1"}},
        {"match_value": 2, "updates": {"ip": "2.2.2.2"}}
    ]'::jsonb
);
```

---

### jsonb_array_update_multi_row

Update arrays across multiple JSONB documents in one call.

**Signature**:

```sql
jsonb_array_update_multi_row(
    targets jsonb[],
    array_path text,
    match_key text,
    match_value jsonb,
    updates jsonb
) RETURNS jsonb[]
```

**Performance**: amortizes per-call FFI overhead across the batch. **Not substantiated against native SQL** — not in the CCX13 matrix (needs an array-of-documents fixture); dev-host measured only ~2× binary-vs-serde. The earlier "~4×" figure predates measurement. See [../benchmarks/2026-07-21-issue15-ccx13.md](../benchmarks/2026-07-21-issue15-ccx13.md).

**Example**:

```sql
-- Update 100 network configurations in one call
UPDATE tv_network_configuration
SET data = batch_result[ordinality]
FROM (
    SELECT unnest(
        jsonb_array_update_multi_row(
            array_agg(data ORDER BY id),
            'dns_servers',
            'id',
            '42'::jsonb,
            '{"ip": "8.8.8.8"}'::jsonb
        )
    ) WITH ORDINALITY AS batch_result
    FROM tv_network_configuration
    WHERE network_id = 17
) batch;
```

---

### jsonb_merge_at_path

Merges a JSONB object at a specific nested path.

**Signature**:

```sql
jsonb_merge_at_path(target jsonb, source jsonb, path text[]) RETURNS jsonb
```

**Performance**: O(depth) where depth = path length. Efficient for deep updates.

**Example**:

```sql
SELECT jsonb_merge_at_path(
    '{"config": {"name": "old", "ttl": 300}}'::jsonb,
    '{"name": "new"}'::jsonb,
    ARRAY['config']
);
-- Result: {"config": {"name": "new", "ttl": 300}}
```

---

### jsonb_merge_shallow

Merges top-level keys from source into target (shallow merge).

**Signature**:

```sql
jsonb_merge_shallow(target jsonb, source jsonb) RETURNS jsonb
```

**Behavior**: Source keys overwrite target keys on conflict. Nested objects are replaced, not recursively merged.

**Example**:

```sql
SELECT jsonb_merge_shallow(
    '{"a": 1, "b": 2}'::jsonb,
    '{"b": 99, "c": 3}'::jsonb
);
-- Result: {"a": 1, "b": 99, "c": 3}
```

---

## Error Handling

All functions follow these principles:

### NULL Handling

All functions are **STRICT**: return NULL if any required input is NULL.

```sql
-- Returns NULL (not an error)
SELECT jsonb_smart_patch_scalar(NULL, '{"name": "test"}'::jsonb);  -- NULL
SELECT jsonb_smart_patch_scalar('{"a": 1}'::jsonb, NULL);          -- NULL
```

**Exception**: `jsonb_array_insert_where()` allows NULL for `sort_key` and `sort_order` parameters for unsorted insertion.

### Graceful Failures

- Invalid paths → return original unchanged
- Missing keys → return original unchanged
- Type mismatches → return original unchanged
- No element matches → return original unchanged

```sql
-- Path doesn't exist → returns original unchanged
SELECT jsonb_array_delete_where(
    '{"other": "data"}'::jsonb,
    'posts',
    'id',
    '42'::jsonb
);
-- Result: {"other": "data"} (unchanged)
```

### No Exceptions

Functions never throw errors on invalid input. Defensive coding ensures stability in production.

### Best Practices

```sql
-- 1. Check if operation succeeded (compare result)
UPDATE tv_feed
SET data = jsonb_array_delete_where(data, 'posts', 'id', '42'::jsonb)
WHERE jsonb_array_contains_id(data, 'posts', 'id', '42'::jsonb);
-- Only updates rows that actually contain the element

-- 2. Handle NULL parameters explicitly
SELECT COALESCE(
    jsonb_smart_patch_scalar(data, updates),
    data  -- fallback if function returns NULL
) FROM tv_company;

-- 3. Validate JSONB structure before calling
SELECT
    CASE
        WHEN jsonb_typeof(data->'posts') = 'array'
        THEN jsonb_array_update_where(data, 'posts', 'id', id_val, updates)
        ELSE data  -- return unchanged if not an array
    END
FROM tv_feed;
```

---

## Performance Characteristics

Measured on a rentable machine profile (Hetzner CCX13, PostgreSQL 17.10, release
build). Ratios are `native_median / jsonb_delta_median`, so > 1 is faster than
the native baseline. Full method and per-size numbers:
[../benchmarks/2026-07-21-issue15-ccx13.md](../benchmarks/2026-07-21-issue15-ccx13.md).

| Operation | Speedup vs native | Notes |
|-----------|-------------------|-------|
| `jsonb_array_update_where` (single element) | **1.7–2.5×** | rises with array size |
| `jsonb_array_delete_where` (single element) | **1.6–3.8×** | rises with array size |
| `jsonb_merge_shallow` vs `\|\|` | **parity (~1.0×)** | not a speedup over the built-in |
| `jsonb_array_update_where_batch` vs re-aggregation | **parity (~1.07×)** | single pass; O(elements × specs) today |
| `jsonb_apply_changeset` vs N chained patches | **up to ~21×** at high N | small loss at 1–2 edits |

The earlier per-function millisecond table on this page cited figures that
predated measurement; issue #15 was correct that they did not reproduce. The
ratios above replace them.

---

## Version History

- **v0.1.0** - Complete JSONB CRUD operations for CQRS architectures (2025-12)

See [CHANGELOG.md](../changelog.md) for detailed version history.
