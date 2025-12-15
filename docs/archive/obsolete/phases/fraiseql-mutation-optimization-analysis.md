# FraiseQL Mutation Optimization Analysis for PostgreSQL Extension Specialists

**Date**: 2025-12-08
**Audience**: PostgreSQL extension developers, JSONB performance specialists
**Purpose**: Design optimal JSONB incremental view maintenance for GraphQL-style nested object composition

---

## Executive Summary

This document analyzes performance bottlenecks in FraiseQL's CQRS projection system and proposes PostgreSQL extension features for 10-15x mutation performance improvement through surgical JSONB updates.

**Key Findings**:
- Current approach: Full JSONB rebuild on every mutation (15-150ms per affected view)
- Target approach: Surgical path updates (1-10ms per affected view)
- Critical missing features: nested path merge, array element update, change detection

---

## Background: FraiseQL's Composition Pattern

### Architecture Overview

FraiseQL implements GraphQL-style nested object composition using PostgreSQL JSONB:

```
tb_dns_server (base table)
  ↓ (trigger updates)
v_dns_server.data (view projection: {id, ip, port, ...})
  ↓ (composed into)
tv_network_configuration.data (materialized view: {
    id, name, ...,
    dns_servers: [v_dns_server.data, ...],  ← array of composed objects
    gateway: v_gateway.data,                 ← composed object
    ...
})
  ↓ (composed into)
tv_allocation.data (top-level materialized view: {
    id, name, ...,
    network_configuration: tv_network_configuration.data,  ← nested composition
    machine: tv_machine.data,
    storage: tv_storage.data,
    ...
})
```

### Current Mutation Flow (Inefficient)

**Example**: User updates DNS server IP from `192.168.1.1` → `8.8.8.8`

```sql
-- Step 1: Base table mutation
UPDATE tb_dns_server SET ip = '8.8.8.8' WHERE id = 42;

-- Step 2: View refresh (trigger)
REFRESH MATERIALIZED VIEW v_dns_server;  -- or incremental trigger update

-- Step 3: Propagate to parent view (trigger on v_dns_server)
UPDATE tv_network_configuration
SET data = (
    SELECT jsonb_build_object(
        'id', nc.id,
        'name', nc.name,
        'dns_servers', (
            SELECT jsonb_agg(v.data ORDER BY v.id)
            FROM network_configuration_dns_servers ncd
            JOIN v_dns_server v ON v.id = ncd.dns_server_id
            WHERE ncd.network_configuration_id = nc.id
        ),
        'gateway', (SELECT data FROM v_gateway WHERE id = nc.gateway_id),
        -- ... 10+ more fields
    )
    FROM tb_network_configuration nc
    WHERE nc.id = tv_network_configuration.id
)
WHERE id IN (
    SELECT network_configuration_id
    FROM network_configuration_dns_servers
    WHERE dns_server_id = 42
);
-- Performance: 15-50ms per affected network_configuration

-- Step 4: Propagate to allocation view (trigger on tv_network_configuration)
UPDATE tv_allocation
SET data = (
    SELECT jsonb_build_object(
        'id', a.id,
        'name', a.name,
        'network_configuration', nc.data,
        'machine', m.data,
        'storage', s.data,
        -- ... 15+ more fields
    )
    FROM tb_allocation a
    LEFT JOIN tv_network_configuration nc ON nc.id = a.network_configuration_id
    LEFT JOIN tv_machine m ON m.id = a.machine_id
    LEFT JOIN tv_storage s ON s.id = a.storage_id
    WHERE a.id = tv_allocation.id
)
WHERE network_configuration_id IN (affected_ids);
-- Performance: 50-150ms per affected allocation
```

**Problem**: Changing 1 IP address triggers full JSONB reconstruction of 10-100+ records across multiple view levels.

**Cascading Cost Example**:
- 1 DNS server update
- → Affects 5 network configurations (rebuild 5 × 30ms = 150ms)
- → Affects 20 allocations (rebuild 20 × 100ms = 2000ms)
- **Total: 2.15 seconds for a single IP change**

---

## Proposed Solution: Surgical JSONB Updates

### Goal
Update **only the changed path** in the JSONB tree without rebuilding entire objects.

### Target Performance
- 1 DNS server update
- → Update 5 network configurations (5 × 2ms = 10ms) — **15x faster**
- → Update 20 allocations (20 × 5ms = 100ms) — **20x faster**
- **Total: 110ms (95% reduction)**

### Required Extension Features

---

## Feature 1: `jsonb_merge_at_path` - Nested Path Merging

### Problem Statement

Current `jsonb_merge_shallow` destroys nested data:

```sql
SELECT jsonb_merge_shallow(
    '{"network": {"dns_servers": [...], "gateway": {...}}}'::jsonb,
    '{"network": {"dns_servers": [updated_dns]}}'::jsonb
);
-- Result: {"network": {"dns_servers": [updated_dns]}}
-- Problem: gateway is LOST ❌
```

### Proposed Function

```sql
jsonb_merge_at_path(
    target jsonb,      -- Full JSONB object
    source jsonb,      -- Partial update to merge
    path text[]        -- Path where merge should occur
) RETURNS jsonb
```

**Behavior**:
1. Navigate to `target[path[0]][path[1]]...`
2. Shallow merge `source` into the object at that path
3. Return full `target` with only the nested path modified

**Example**:
```sql
SELECT jsonb_merge_at_path(
    target := '{
        "id": 17,
        "network": {
            "dns_servers": [{"id": 1, "ip": "old"}],
            "gateway": {"id": 5, "ip": "10.0.0.1"}
        }
    }'::jsonb,
    source := '{
        "dns_servers": [{"id": 1, "ip": "8.8.8.8"}]
    }'::jsonb,
    path := ARRAY['network']
);

-- Result:
-- {
--     "id": 17,
--     "network": {
--         "dns_servers": [{"id": 1, "ip": "8.8.8.8"}],  ← updated
--         "gateway": {"id": 5, "ip": "10.0.0.1"}        ← preserved ✓
--     }
-- }
```

### Edge Cases to Handle

```sql
-- 1. Path does not exist → create it
SELECT jsonb_merge_at_path(
    '{"id": 1}'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb,
    ARRAY['network', 'dns']
);
-- Result: {"id": 1, "network": {"dns": {"ip": "8.8.8.8"}}}

-- 2. Empty path → equivalent to jsonb_merge_shallow
SELECT jsonb_merge_at_path(
    '{"a": 1}'::jsonb,
    '{"b": 2}'::jsonb,
    ARRAY[]::text[]
);
-- Result: {"a": 1, "b": 2}

-- 3. Path points to non-object → error or replace?
SELECT jsonb_merge_at_path(
    '{"network": "string_value"}'::jsonb,
    '{"dns": "data"}'::jsonb,
    ARRAY['network']
);
-- Recommendation: ERROR (cannot merge into non-object)
```

### Implementation Considerations for PostgreSQL Specialists

**Option A: Recursive `jsonb_set` approach**
```c
// Pseudo-code
JsonbValue* jsonb_merge_at_path(target, source, path) {
    if (path.length == 0) {
        return jsonb_merge_shallow(target, source);
    }

    // Navigate to parent of merge location
    JsonbValue* nested = jsonb_get_nested(target, path);
    JsonbValue* merged = jsonb_merge_shallow(nested, source);

    // Use jsonb_set to replace at path
    return jsonb_set(target, path, merged, create_if_missing=true);
}
```

**Option B: In-place modification with JSONB iterator**
```c
// Directly modify the JSONB container at the target path
// Avoids full object rebuild
// More complex but potentially faster
```

**Performance Question**: Which approach is faster for:
- Small objects (10 keys, path depth 2)?
- Large objects (150 keys, path depth 4)?
- Deep nesting (path depth 6-8)?

---

## Feature 2: `jsonb_array_update_where` - Surgical Array Element Updates

### Problem Statement

Updating a single array element requires rebuilding the entire array:

```sql
-- Current approach: rebuild entire dns_servers array
UPDATE tv_network_configuration
SET data = jsonb_set(
    data,
    '{dns_servers}',
    (SELECT jsonb_agg(v.data ORDER BY v.id)
     FROM network_configuration_dns_servers ncd
     JOIN v_dns_server v ON v.id = ncd.dns_server_id
     WHERE ncd.network_configuration_id = 17)
)
WHERE id = 17;
-- Cost: O(n) where n = array length (e.g., 50 DNS servers = rebuild all 50)
```

### Proposed Function

```sql
jsonb_array_update_where(
    target jsonb,         -- Full JSONB object
    array_path text[],    -- Path to the array
    match_key text,       -- Key to match on (e.g., 'id')
    match_value jsonb,    -- Value to match (e.g., '42')
    updates jsonb         -- Fields to merge into matched element
) RETURNS jsonb
```

**Behavior**:
1. Navigate to array at `target[array_path]`
2. Find element where `element[match_key] = match_value`
3. Shallow merge `updates` into that element only
4. Return full `target` with only one array element modified

**Example**:
```sql
SELECT jsonb_array_update_where(
    target := '{
        "id": 17,
        "dns_servers": [
            {"id": 1, "ip": "1.1.1.1", "port": 53},
            {"id": 2, "ip": "8.8.8.8", "port": 53},
            {"id": 3, "ip": "9.9.9.9", "port": 53}
        ]
    }'::jsonb,
    array_path := ARRAY['dns_servers'],
    match_key := 'id',
    match_value := '2'::jsonb,
    updates := '{"ip": "8.8.4.4", "status": "updated"}'::jsonb
);

-- Result:
-- {
--     "id": 17,
--     "dns_servers": [
--         {"id": 1, "ip": "1.1.1.1", "port": 53},           ← unchanged
--         {"id": 2, "ip": "8.8.4.4", "port": 53, "status": "updated"},  ← merged
--         {"id": 3, "ip": "9.9.9.9", "port": 53}            ← unchanged
--     ]
-- }
```

### Edge Cases to Handle

```sql
-- 1. Multiple matches → update first only or all?
-- Recommendation: Update ALL matching elements (most useful for FraiseQL)

-- 2. No matches → error or no-op?
-- Recommendation: Return target unchanged (no-op)

-- 3. match_value is null
SELECT jsonb_array_update_where(
    target := '{"items": [{"id": null, "name": "a"}, {"id": 1, "name": "b"}]}'::jsonb,
    array_path := ARRAY['items'],
    match_key := 'id',
    match_value := 'null'::jsonb,
    updates := '{"status": "updated"}'::jsonb
);
-- Should match elements where id IS NULL

-- 4. Array contains non-objects → skip or error?
-- Recommendation: Skip non-object elements silently
```

### Variant: `jsonb_array_remove_where`

```sql
jsonb_array_remove_where(
    target jsonb,
    array_path text[],
    match_key text,
    match_value jsonb
) RETURNS jsonb

-- Example: Remove DNS server from array
SELECT jsonb_array_remove_where(
    '{"dns_servers": [{"id": 1}, {"id": 2}, {"id": 3}]}'::jsonb,
    ARRAY['dns_servers'],
    'id',
    '2'::jsonb
);
-- Result: {"dns_servers": [{"id": 1}, {"id": 3}]}
```

### Implementation Considerations

**Option A: Full array rebuild with filter**
```c
// 1. Iterate over array elements
// 2. For matching elements, merge updates
// 3. Rebuild array from modified elements
// Complexity: O(n) where n = array length
```

**Option B: In-place modification**
```c
// 1. Find matching element index
// 2. Extract element, merge, re-insert at same index
// 3. Avoid rebuilding non-matching elements
// Complexity: O(m) where m = number of matches (typically 1)
```

**Performance Question**:
- For arrays with 1-10 elements, does in-place modification matter?
- For arrays with 100+ elements, how much faster is in-place?
- What's the break-even point?

---

## Feature 3: `jsonb_has_path_changed` - Change Detection

### Problem Statement

Triggers fire on every UPDATE even when JSONB field hasn't changed or changed in irrelevant paths:

```sql
-- User updates allocation name (unrelated to network config)
UPDATE tb_allocation SET name = 'New Name' WHERE id = 1;

-- Current: Trigger fires and rebuilds tv_allocation.data (expensive!)
-- Desired: Detect that 'data' column didn't change, skip rebuild
```

### Proposed Function (Simple Version)

```sql
jsonb_has_path_changed(
    old_jsonb jsonb,
    new_jsonb jsonb,
    path text[]
) RETURNS boolean
```

**Behavior**:
- Return `TRUE` if `old_jsonb[path] != new_jsonb[path]`
- Return `FALSE` if values are equal (deep comparison)
- Return `TRUE` if path exists in only one of the arguments

**Example**:
```sql
-- In trigger function
CREATE FUNCTION propagate_allocation_changes()
RETURNS TRIGGER AS $$
BEGIN
    -- Only propagate if relevant paths changed
    IF jsonb_has_path_changed(OLD.data, NEW.data, ARRAY['network_configuration']) THEN
        PERFORM propagate_to_parent_views('allocation', NEW.id);
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;
```

### Proposed Function (Advanced Version)

```sql
jsonb_detect_changes(
    old_jsonb jsonb,
    new_jsonb jsonb,
    watch_paths text[][] DEFAULT NULL  -- Array of paths to watch
) RETURNS text[]  -- Returns array of changed paths
```

**Example**:
```sql
SELECT jsonb_detect_changes(
    old_jsonb := '{"a": 1, "b": {"c": 2}, "d": 3}'::jsonb,
    new_jsonb := '{"a": 1, "b": {"c": 99}, "d": 4}'::jsonb,
    watch_paths := ARRAY[
        ARRAY['a'],
        ARRAY['b', 'c'],
        ARRAY['d']
    ]
);
-- Result: ARRAY['b.c', 'd']  (a unchanged, b.c and d changed)
```

### Implementation Considerations

**Naive approach**: Deserialize and compare
```c
// Extract value at path for both old and new
// Use jsonb_cmp() or deep equality check
// Problem: Expensive for deep paths in large objects
```

**Optimized approach**: Hash comparison
```c
// 1. Compute hash of jsonb[path] for old and new
// 2. Compare hashes first (fast rejection)
// 3. Only deserialize if hashes match (collision check)
```

**Performance Question**:
- For paths at depth 1-2, is hash overhead worth it?
- For paths at depth 5+, does hash save significant time?
- What about large nested objects (10KB+ at path)?

---

## Feature 4: `jsonb_array_upsert_where` - Array Element Insert-or-Update

### Problem Statement

Adding a new element to an array while ensuring no duplicates:

```sql
-- Current: Check existence, then either update or append
DO $$
DECLARE
    element_exists boolean;
BEGIN
    -- Check if DNS server is already in array
    SELECT EXISTS(
        SELECT 1 FROM jsonb_array_elements(data->'dns_servers') AS elem
        WHERE elem->>'id' = '42'
    ) INTO element_exists
    FROM tv_network_configuration WHERE id = 17;

    IF element_exists THEN
        -- Update existing element
        UPDATE tv_network_configuration
        SET data = jsonb_array_update_where(...)
        WHERE id = 17;
    ELSE
        -- Append new element
        UPDATE tv_network_configuration
        SET data = jsonb_set(data, '{dns_servers}',
                             (data->'dns_servers') || new_dns_data)
        WHERE id = 17;
    END IF;
END $$;
```

### Proposed Function

```sql
jsonb_array_upsert_where(
    target jsonb,
    array_path text[],
    match_key text,
    match_value jsonb,
    element jsonb        -- Full element to insert or merge
) RETURNS jsonb
```

**Behavior**:
- If element with `match_key = match_value` exists: merge `element` into it
- If no match exists: append `element` to array
- Return modified `target`

**Example**:
```sql
-- Add or update DNS server in one operation
SELECT jsonb_array_upsert_where(
    target := '{"dns_servers": [{"id": 1, "ip": "1.1.1.1"}]}'::jsonb,
    array_path := ARRAY['dns_servers'],
    match_key := 'id',
    match_value := '2'::jsonb,
    element := '{"id": 2, "ip": "8.8.8.8", "port": 53}'::jsonb
);
-- Result: {"dns_servers": [{"id": 1, "ip": "1.1.1.1"}, {"id": 2, "ip": "8.8.8.8", "port": 53}]}

-- Update existing element
SELECT jsonb_array_upsert_where(
    target := '{"dns_servers": [{"id": 1, "ip": "1.1.1.1"}]}'::jsonb,
    array_path := ARRAY['dns_servers'],
    match_key := 'id',
    match_value := '1'::jsonb,
    element := '{"id": 1, "ip": "8.8.4.4"}'::jsonb
);
-- Result: {"dns_servers": [{"id": 1, "ip": "8.8.4.4"}]}  (ip updated)
```

---

## Feature 5 (Advanced): Bulk Path Updates - `jsonb_bulk_merge_at_paths`

### Problem Statement

When updating 100 DNS servers, we make 100 separate calls to update parent views:

```sql
-- Inefficient: 100 separate updates
FOR dns_id IN (SELECT id FROM updated_dns_servers) LOOP
    UPDATE tv_network_configuration
    SET data = jsonb_array_update_where(data, ...)
    WHERE id IN (SELECT network_configuration_id
                 FROM network_configuration_dns_servers
                 WHERE dns_server_id = dns_id);
END LOOP;
```

### Proposed Function

```sql
jsonb_bulk_merge_at_paths(
    target jsonb,
    path_updates jsonb  -- Array of {path: [...], updates: {...}}
) RETURNS jsonb
```

**Example**:
```sql
-- Apply multiple path updates in one pass
SELECT jsonb_bulk_merge_at_paths(
    target := '{
        "network": {"dns_servers": [...]},
        "machine": {"hostname": "old"},
        "storage": {"size": 100}
    }'::jsonb,
    path_updates := '[
        {"path": ["network", "dns_servers"], "updates": {"status": "updated"}},
        {"path": ["machine"], "updates": {"hostname": "new"}},
        {"path": ["storage"], "updates": {"size": 200}}
    ]'::jsonb
);
-- Result: All three paths updated in single traversal
```

**Performance Benefit**:
- Single JSONB traversal instead of N traversals
- Reduces overhead from repeated deserialization/serialization

**Implementation Challenge**:
- Handling overlapping paths (e.g., ['a'] and ['a', 'b'])
- Order of application (does it matter?)

---

## Scope Building System (Non-Extension Feature)

### Problem Statement

How do we efficiently determine which views need updates when a base table changes?

**Example**: DNS server 42 is updated
- Which `tv_network_configuration` records contain this DNS server?
- Which `tv_allocation` records depend on those configurations?

### Proposed Solution: Dependency Graph Metadata

```sql
CREATE TABLE jsonb_ivm_dependency_graph (
    source_entity text,      -- 'v_dns_server'
    source_id bigint,        -- 42
    target_view text,        -- 'tv_network_configuration'
    target_id bigint,        -- 17
    jsonb_path text[],       -- ['dns_servers', '0']  (position in array)
    updated_at timestamptz DEFAULT now(),
    PRIMARY KEY (source_entity, source_id, target_view, target_id)
);

CREATE INDEX idx_dependency_source ON jsonb_ivm_dependency_graph(source_entity, source_id);
CREATE INDEX idx_dependency_target ON jsonb_ivm_dependency_graph(target_view, target_id);
```

**Population**: Triggers on relationship tables

```sql
-- When a DNS server is added to a network configuration
CREATE TRIGGER track_dns_dependency
AFTER INSERT OR UPDATE OR DELETE ON network_configuration_dns_servers
FOR EACH ROW
EXECUTE FUNCTION track_jsonb_dependency(
    'v_dns_server',
    'tv_network_configuration',
    ARRAY['dns_servers']
);
```

**Usage**: Efficient scope resolution

```sql
-- Find all views affected by DNS server 42
SELECT DISTINCT target_view, array_agg(target_id) as target_ids
FROM jsonb_ivm_dependency_graph
WHERE source_entity = 'v_dns_server'
  AND source_id = 42
GROUP BY target_view;

-- Bulk update all affected network configurations
UPDATE tv_network_configuration
SET data = jsonb_array_update_where(
    data,
    ARRAY['dns_servers'],
    'id',
    '42'::jsonb,
    (SELECT data FROM v_dns_server WHERE id = 42)
)
WHERE id IN (SELECT target_id FROM affected_configs);
```

### Alternative: Materialized Path in JSONB

Store dependency metadata directly in the JSONB:

```sql
-- Each tv_network_configuration.data includes dependency hints
{
    "id": 17,
    "dns_servers": [
        {"id": 42, "ip": "8.8.8.8", "_source": "v_dns_server:42"}
    ],
    "_dependencies": {
        "v_dns_server": [42, 43, 44],
        "v_gateway": [5]
    }
}
```

**Pros**: No separate dependency table
**Cons**: Increases JSONB size, harder to query efficiently

---

## Performance Benchmarking Requirements

To validate these features, PostgreSQL specialists should benchmark:

### Test Scenarios

1. **Small objects, shallow nesting** (10 keys, depth 2)
   - Current: Full rebuild
   - Proposed: `jsonb_merge_at_path`
   - Expected improvement: 5-10x

2. **Large objects, deep nesting** (150 keys, depth 5)
   - Current: Full rebuild
   - Proposed: `jsonb_merge_at_path`
   - Expected improvement: 10-20x

3. **Array with few elements** (5 elements)
   - Current: Full array rebuild
   - Proposed: `jsonb_array_update_where`
   - Expected improvement: 2-3x

4. **Array with many elements** (100 elements)
   - Current: Full array rebuild
   - Proposed: `jsonb_array_update_where`
   - Expected improvement: 20-50x

5. **Cascading updates** (3 levels deep, 1 → 10 → 100 records)
   - Current: Full rebuild at each level
   - Proposed: Surgical updates with change detection
   - Expected improvement: 15-30x overall

### Benchmark SQL Script Template

```sql
-- Setup test data
CREATE TABLE test_composition (
    id bigint PRIMARY KEY,
    data jsonb
);

INSERT INTO test_composition (id, data)
SELECT
    id,
    jsonb_build_object(
        'id', id,
        'name', 'test_' || id,
        'network', jsonb_build_object(
            'dns_servers', (
                SELECT jsonb_agg(jsonb_build_object('id', i, 'ip', '192.168.1.' || i))
                FROM generate_series(1, 50) i
            ),
            'gateway', jsonb_build_object('id', 1, 'ip', '10.0.0.1')
        ),
        'machine', jsonb_build_object('id', id, 'hostname', 'host_' || id),
        'metadata', jsonb_build_object(/* 100 additional keys */)
    )
FROM generate_series(1, 10000) id;

-- Benchmark 1: Full rebuild (current approach)
EXPLAIN ANALYZE
UPDATE test_composition
SET data = (
    SELECT jsonb_build_object(
        'id', id,
        'name', 'test_' || id,
        'network', jsonb_build_object(
            'dns_servers', (
                SELECT jsonb_agg(
                    CASE WHEN i = 25
                         THEN jsonb_build_object('id', 25, 'ip', '8.8.8.8')
                         ELSE jsonb_build_object('id', i, 'ip', '192.168.1.' || i)
                    END
                )
                FROM generate_series(1, 50) i
            ),
            'gateway', jsonb_build_object('id', 1, 'ip', '10.0.0.1')
        ),
        'machine', data->'machine',
        'metadata', data->'metadata'
    )
    FROM test_composition tc
    WHERE tc.id = test_composition.id
)
WHERE id BETWEEN 1 AND 100;

-- Benchmark 2: Surgical update (proposed approach)
EXPLAIN ANALYZE
UPDATE test_composition
SET data = jsonb_array_update_where(
    data,
    ARRAY['network', 'dns_servers'],
    'id',
    '25'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb
)
WHERE id BETWEEN 1 AND 100;

-- Compare execution time and I/O
```

---

## Open Questions for PostgreSQL Specialists

### 1. Optimal JSONB Traversal Strategy
- **Question**: For `jsonb_merge_at_path` with deep paths (depth 5-8), is it faster to:
  - Use recursive `jsonb_set` calls?
  - Deserialize to C struct, modify, re-serialize?
  - Use JSONB iterator API for in-place modification?

### 2. Array Element Search Performance
- **Question**: For `jsonb_array_update_where`, what's the fastest way to find matching elements?
  - Linear scan with `jsonb_array_elements`?
  - Build temporary hash table of array elements?
  - Use GIN index on JSONB array (if array is at top level)?

### 3. Change Detection Granularity
- **Question**: Should `jsonb_detect_changes` return:
  - Boolean (any change detected)?
  - Array of changed paths (['a.b.c', 'd.e'])?
  - Detailed diff with old/new values?
  - What's the performance trade-off for each level of detail?

### 4. Memory Management for Large JSONB
- **Question**: When merging 10MB JSONB objects, how do we avoid:
  - Double memory allocation (old + new)?
  - Stack overflow from deep recursion?
  - TOASTed value decompression overhead?

### 5. Parallel Safety and Locking
- **Question**: Are these functions safe for:
  - `PARALLEL SAFE` marking?
  - Concurrent updates to same JSONB (row-level locking sufficient)?
  - Use in triggers (re-entrancy concerns)?

### 6. NULL and Missing Path Semantics
- **Question**: What should happen when:
  - `jsonb_merge_at_path` path doesn't exist (create vs error)?
  - `jsonb_array_update_where` finds no matches (no-op vs error)?
  - `jsonb_has_path_changed` compares NULL vs missing key (equal vs different)?

### 7. Type Safety vs Flexibility
- **Question**: Should functions:
  - Enforce strict types (error on array/scalar at merge path)?
  - Allow coercion (string → number, etc.)?
  - Support "force" flag to replace non-objects?

---

## Recommended Implementation Priority

### Phase 1: Core Features (v0.2.0-alpha2)
1. ✅ **`jsonb_merge_at_path`** (highest impact, enables nested updates)
2. ✅ **`jsonb_array_update_where`** (critical for array composition)
3. ✅ **`jsonb_has_path_changed`** (simple boolean version, optimize triggers)

**Estimated development time**: 1-2 weeks
**Expected performance gain**: 10-15x for FraiseQL mutations

### Phase 2: Advanced Features (v0.3.0-alpha3)
4. ⏳ **`jsonb_array_remove_where`** (complete array CRUD)
5. ⏳ **`jsonb_array_upsert_where`** (simplify insert-or-update logic)
6. ⏳ **`jsonb_detect_changes`** (advanced version with path array return)

**Estimated development time**: 1-2 weeks
**Expected performance gain**: Additional 2-3x improvement

### Phase 3: Optimization Features (v0.4.0-alpha4)
7. ⏳ **`jsonb_bulk_merge_at_paths`** (batch operations)
8. ⏳ Dependency graph system (scope building)
9. ⏳ Performance tuning based on real-world benchmarks

**Estimated development time**: 2-3 weeks
**Expected performance gain**: 2x additional improvement for bulk operations

---

## Real-World FraiseQL Usage Example

### Scenario: User updates allocation name + DNS server IP (2 mutations)

```sql
BEGIN;

-- Mutation 1: Update allocation name (base table field)
UPDATE tb_allocation SET name = 'Production Server' WHERE id = 100;

-- Trigger: Should detect that 'name' is not in data JSONB, skip propagation
CREATE TRIGGER check_allocation_data_changes
AFTER UPDATE ON tb_allocation
FOR EACH ROW
EXECUTE FUNCTION smart_allocation_propagation();

CREATE FUNCTION smart_allocation_propagation()
RETURNS TRIGGER AS $$
BEGIN
    -- Only rebuild data JSONB if relevant fields changed
    -- name is NOT in data, so skip expensive rebuild
    IF NOT (
        NEW.network_configuration_id IS DISTINCT FROM OLD.network_configuration_id OR
        NEW.machine_id IS DISTINCT FROM OLD.machine_id OR
        NEW.storage_id IS DISTINCT FROM OLD.storage_id
        -- ... other FK fields that affect data JSONB
    ) THEN
        RETURN NEW;  -- Skip propagation
    END IF;

    -- Relevant field changed, rebuild data
    -- (but use surgical update instead of full rebuild)
    -- ...
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Mutation 2: Update DNS server IP (JSONB field in composition hierarchy)
UPDATE tb_dns_server SET ip = '8.8.8.8' WHERE id = 42;

-- Trigger 1: Update v_dns_server.data
CREATE TRIGGER sync_dns_view
AFTER UPDATE ON tb_dns_server
FOR EACH ROW
EXECUTE FUNCTION update_v_dns_server();

CREATE FUNCTION update_v_dns_server()
RETURNS TRIGGER AS $$
BEGIN
    UPDATE v_dns_server
    SET data = jsonb_build_object(
        'id', NEW.id,
        'ip', NEW.ip,
        'port', NEW.port,
        'status', NEW.status
    )
    WHERE id = NEW.id;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger 2: Propagate to tv_network_configuration (surgical update)
CREATE TRIGGER propagate_dns_to_netconfig
AFTER UPDATE ON v_dns_server
FOR EACH ROW
EXECUTE FUNCTION surgical_netconfig_update();

CREATE FUNCTION surgical_netconfig_update()
RETURNS TRIGGER AS $$
BEGIN
    -- Use jsonb_array_update_where for surgical update
    UPDATE tv_network_configuration
    SET data = jsonb_array_update_where(
        data,
        ARRAY['dns_servers'],    -- Path to array
        'id',                     -- Match on id field
        to_jsonb(NEW.id),        -- Match this DNS server
        NEW.data                 -- Merge updated data
    )
    WHERE id IN (
        SELECT network_configuration_id
        FROM network_configuration_dns_servers
        WHERE dns_server_id = NEW.id
    );
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

-- Trigger 3: Propagate to tv_allocation (surgical update with change detection)
CREATE TRIGGER propagate_netconfig_to_allocation
AFTER UPDATE ON tv_network_configuration
FOR EACH ROW
EXECUTE FUNCTION surgical_allocation_update();

CREATE FUNCTION surgical_allocation_update()
RETURNS TRIGGER AS $$
BEGIN
    -- Only update if network_configuration JSONB actually changed
    IF NOT jsonb_has_path_changed(OLD.data, NEW.data, ARRAY['network_configuration']) THEN
        RETURN NEW;  -- Skip if unchanged
    END IF;

    -- Use jsonb_merge_at_path for surgical nested update
    UPDATE tv_allocation
    SET data = jsonb_merge_at_path(
        data,
        jsonb_build_object('network_configuration', NEW.data),
        ARRAY[]  -- Merge at root level
    )
    WHERE network_configuration_id = NEW.id;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

COMMIT;
```

### Performance Comparison

| Approach | Operations | Time (for 1 DNS update → 5 configs → 20 allocations) |
|----------|-----------|--------------------------------------------------|
| **Full rebuild (current)** | Rebuild 5 configs + 20 allocations | ~2150ms |
| **Shallow merge (v0.1.0)** | Shallow merge (loses nested data) | ~50ms but **incorrect** ❌ |
| **Surgical updates (v0.2.0 proposed)** | Array update + path merge + change detection | ~110ms (95% faster) ✅ |

---

## Summary for PostgreSQL Specialists

### Core Challenge
FraiseQL composes nested JSONB objects across 3-5 levels of views. Single base table mutation cascades through entire hierarchy, requiring JSONB updates at each level. Current approach rebuilds entire JSONB objects (expensive). Need surgical update functions.

### Required Features (Priority Order)
1. **`jsonb_merge_at_path`** - Merge at nested path without destroying siblings
2. **`jsonb_array_update_where`** - Update single array element by matching key
3. **`jsonb_has_path_changed`** - Skip unnecessary updates via change detection

### Success Criteria
- 10-15x performance improvement for single mutations
- Correctness: No data loss from shallow merges
- Scalability: Handle 3-5 level composition hierarchies with 100+ records per level

### Key Design Questions
- Best JSONB traversal strategy for deep paths?
- In-place modification vs rebuild for array updates?
- Change detection granularity (boolean vs path array vs full diff)?

### Next Steps
1. Review proposed function signatures and behavior
2. Identify optimal implementation approaches (see Open Questions)
3. Build performance benchmarks for small/large objects
4. Implement Phase 1 features with TDD approach
5. Validate with real FraiseQL schema (if available for testing)

---

**Contact**: For questions or clarifications on FraiseQL's specific use cases, please consult the FraiseQL documentation or reach out to the FraiseQL maintainers.

**Codebase**: https://github.com/fraiseql/jsonb_ivm
**Related Project**: https://github.com/fraiseql/fraiseql (GraphQL framework using this JSONB composition pattern)
