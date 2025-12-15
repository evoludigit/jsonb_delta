# pg_tview Integration Examples

Complete CRUD workflow examples for incremental view maintenance using jsonb_delta.

---

## Table of Contents

- [Overview](#overview)
- [Architecture Context](#architecture-context)
- [Quick Reference](#quick-reference)
- [Example 1: UPDATE - Scalar Field Change](#example-1-update---scalar-field-change)
- [Example 2: UPDATE - Nested Object Change](#example-2-update---nested-object-change)
- [Example 3: UPDATE - Array Element Change](#example-3-update---array-element-change)
- [Example 4: DELETE - Remove Array Element](#example-4-delete---remove-array-element)
- [Example 5: INSERT - Add Array Element](#example-5-insert---add-array-element)
- [Example 6: Deep Merge - Nested Dependencies](#example-6-deep-merge---nested-dependencies)
- [Function Selection Guide](#function-selection-guide)
- [Error Handling](#error-handling)
- [Performance Tuning](#performance-tuning)

---

## Overview

This guide shows how to use `jsonb_delta` functions in a CQRS architecture with projection tables. Each example demonstrates surgical JSONB updates for incremental view maintenance.

### Key Features

- ✅ **Complete CRUD**: INSERT, UPDATE, and DELETE operations for JSONB arrays
- ✅ **Smart Patch API**: Unified functions for different update patterns
- ✅ **Deep Merge**: Recursive updates for complex nested structures
- ✅ **Helper Functions**: Utilities for ID extraction and existence checking

---

## Architecture Context

### Typical CQRS/pg_tview Table Structure

```sql
-- Leaf view (source of truth)
CREATE TABLE v_company (
    pk INTEGER PRIMARY KEY,
    id UUID,
    name TEXT,
    industry TEXT,
    data JSONB
);

-- Projection tables (denormalized with JSONB)
CREATE TABLE tv_user (
    pk INTEGER PRIMARY KEY,
    id UUID,
    fk_company INTEGER,
    company_id UUID,
    data JSONB  -- Contains nested company object
);

CREATE TABLE tv_feed (
    pk INTEGER PRIMARY KEY,
    data JSONB  -- Contains array of posts with nested authors
);
```

### Dependency Hierarchy

```text
┌─────────────┐
│  v_company  │ ← Leaf view (UPDATE company.name)
└─────────────┘
      │ embedded as object
      ↓
┌─────────────┐
│  tv_user    │ ← Propagate change to data.company.name
└─────────────┘
      │ embedded in array
      ↓
┌─────────────┐
│  tv_post    │ ← Propagate to data.author.company.name
└─────────────┘
      │ aggregated in array
      ↓
┌─────────────┐
│  tv_feed    │ ← Propagate to data.posts[].author.company.name
└─────────────┘
```

---

## Quick Reference

| Operation | Function | Use Case |
|-----------|----------|----------|
| Update top-level field | `jsonb_smart_patch_scalar()` | Company name change |
| Update nested object | `jsonb_smart_patch_nested()` | User's company data |
| Update array element | `jsonb_smart_patch_array()` | Post title in feed |
| Delete array element | `jsonb_array_delete_where()` | Remove post from feed |
| Insert array element | `jsonb_array_insert_where()` | Add post to feed (sorted) |
| Recursive nested merge | `jsonb_deep_merge()` | Multi-level cascades |
| Extract ID safely | `jsonb_extract_id()` | Get UUID from JSONB |
| Check array contains | `jsonb_array_contains_id()` | Filter affected rows |

---

## Example 1: UPDATE - Scalar Field Change

**Scenario**: Company renames from "ACME Corp" to "ACME Corporation"

### Before (Traditional SQL - Re-aggregation)

```sql
-- Step 1: Update leaf view
UPDATE v_company
SET name = 'ACME Corporation',
    data = jsonb_set(data, '{name}', '"ACME Corporation"')
WHERE pk = 1;

-- Step 2: Propagate to tv_user (rebuild entire company object)
UPDATE tv_user
SET data = jsonb_set(
    data,
    '{company}',
    (SELECT data FROM v_company WHERE pk = fk_company)
)
WHERE fk_company = 1;

-- Performance: O(n) rows × O(m) JSONB rebuild = O(n*m)
-- For 50 users: ~8-12ms
```

### After (jsonb_delta - Surgical Update)

```sql
-- Step 1: Update leaf view (same)
UPDATE v_company
SET name = 'ACME Corporation',
    data = jsonb_set(data, '{name}', '"ACME Corporation"')
WHERE pk = 1;

-- Step 2: Propagate using smart patch (surgical)
UPDATE tv_user
SET data = jsonb_smart_patch_nested(
    data,
    '{"name": "ACME Corporation"}'::jsonb,
    ARRAY['company']
)
WHERE fk_company = 1;

-- Performance: O(n) rows × O(1) merge = O(n)
-- For 50 users: ~2-3ms (3-4× faster)
```

**Key Benefits**:
- ✅ Only updates changed fields (preserves `company.industry`, `company.id`, etc.)
- ✅ 3-4× faster than full object rebuild
- ✅ Single code path (no need to check if object exists)

---

## Example 2: UPDATE - Nested Object Change

**Scenario**: Company updates affect deeply nested structures (user → post → feed)

### Before (Traditional SQL - Multiple Passes)

```sql
-- Step 1: Update leaf view
UPDATE v_company SET data = '{"name": "New Name", "industry": "Tech"}'::jsonb WHERE pk = 1;

-- Step 2: Update tv_user (50 rows)
UPDATE tv_user
SET data = jsonb_set(data, '{company}', (SELECT data FROM v_company WHERE pk = 1))
WHERE fk_company = 1;

-- Step 3: Update tv_post (500 rows - nested in data.author.company)
UPDATE tv_post
SET data = jsonb_set(
    data,
    '{author,company}',
    (SELECT data FROM v_company WHERE pk = company_pk)
)
FROM (
    SELECT DISTINCT tu.company_id::text AS company_pk
    FROM tv_user tu
    WHERE tu.fk_company = 1
) affected
WHERE (data->'author'->>'company_id') = affected.company_pk;

-- Total time: ~35-50ms
```

### After (jsonb_delta - Deep Merge)

```sql
-- Step 1: Update leaf view (same)
UPDATE v_company SET data = '{"name": "New Name", "industry": "Tech"}'::jsonb WHERE pk = 1;

-- Step 2: Update tv_user (50 rows)
UPDATE tv_user
SET data = jsonb_smart_patch_nested(data, (SELECT data FROM v_company WHERE pk = 1), ARRAY['company'])
WHERE fk_company = 1;

-- Step 3: Update tv_post (500 rows - deep merge)
UPDATE tv_post
SET data = jsonb_deep_merge(
    data,
    jsonb_build_object(
        'author',
        jsonb_build_object(
            'company',
            (SELECT data FROM v_company WHERE pk = 1)
        )
    )
)
WHERE fk_user IN (SELECT pk FROM tv_user WHERE fk_company = 1);

-- Total time: ~15-20ms (2× faster)
```

**Key Benefits**:
- ✅ Preserves all non-updated nested fields
- ✅ Single merge operation vs multiple `jsonb_set()` calls
- ✅ 2× faster than traditional approach

---

## Example 3: UPDATE - Array Element Change

**Scenario**: Update post title in a feed containing 100 posts

### Before - Example 3 (Traditional SQL - Re-aggregation)

```sql
-- Update the post first
UPDATE tv_post
SET data = jsonb_set(data, '{title}', '"Updated Title"')
WHERE pk = 42;

-- Re-aggregate entire feed (SLOW!)
UPDATE tv_feed
SET data = jsonb_build_object(
    'posts',
    (
        SELECT jsonb_agg(p.data ORDER BY (p.data->>'created_at')::timestamptz DESC)
        FROM tv_post p
        WHERE p.pk IN (
            SELECT (jsonb_array_elements(tv_feed.data->'posts')->>'id')::uuid
            FROM tv_post
            WHERE pk = 42
        )
        LIMIT 100
    )
)
WHERE pk = 1;

-- Performance: ~18-25ms (re-aggregates all 100 posts)
```

### After (jsonb_delta - Surgical Array Update)

```sql
-- Update the post first
UPDATE tv_post
SET data = jsonb_set(data, '{title}', '"Updated Title"')
WHERE pk = 42;

-- Surgically update only the affected element
UPDATE tv_feed
SET data = jsonb_smart_patch_array(
    data,
    '{"title": "Updated Title"}'::jsonb,
    'posts',
    'id',
    (SELECT data->'id' FROM tv_post WHERE pk = 42)
)
WHERE jsonb_array_contains_id(
    data,
    'posts',
    'id',
    (SELECT data->'id' FROM tv_post WHERE pk = 42)
);

-- Performance: ~3-5ms (updates only 1 element)
```

**Key Benefits**:
- ✅ 5-7× faster (surgical update vs re-aggregation)
- ✅ Preserves array order automatically
- ✅ `jsonb_array_contains_id()` efficiently filters affected feeds

---

## Example 4: DELETE - Remove Array Element

**Scenario**: Delete a post from the feed

### Before - Example 4 (Traditional SQL - Re-aggregation)

```sql
-- Step 1: Delete the post
DELETE FROM tv_post WHERE pk = 42;

-- Step 2: Re-aggregate feed (rebuild entire array)
UPDATE tv_feed
SET data = jsonb_build_object(
    'posts',
    (
        SELECT jsonb_agg(data ORDER BY (data->>'created_at')::timestamptz DESC)
        FROM tv_post
        WHERE pk != 42
        LIMIT 100
    )
)
WHERE pk = 1;

-- Performance: ~20-30ms (re-aggregates 99 remaining posts)
```

### After (jsonb_delta - Surgical Deletion)

```sql
-- Step 1: Get post ID before deletion
DO $$
DECLARE
    post_id_to_delete jsonb;
BEGIN
    -- Extract ID before deletion
    SELECT data->'id' INTO post_id_to_delete FROM tv_post WHERE pk = 42;

    -- Delete the post
    DELETE FROM tv_post WHERE pk = 42;

    -- Surgically remove from feed
    UPDATE tv_feed
    SET data = jsonb_array_delete_where(
        data,
        'posts',
        'id',
        post_id_to_delete
    )
    WHERE jsonb_array_contains_id(data, 'posts', 'id', post_id_to_delete);
END $$;

-- Performance: ~4-6ms (removes 1 element in-place)
```

#### Alternative: Using a trigger

```sql
-- Better approach: Use BEFORE DELETE trigger to capture ID
CREATE OR REPLACE FUNCTION propagate_post_deletion()
RETURNS TRIGGER AS $$
BEGIN
    -- Surgically remove from all feeds
    UPDATE tv_feed
    SET data = jsonb_array_delete_where(
        data,
        'posts',
        'id',
        OLD.data->'id'
    )
    WHERE jsonb_array_contains_id(data, 'posts', 'id', OLD.data->'id');

    RETURN OLD;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER post_deletion_propagation
BEFORE DELETE ON tv_post
FOR EACH ROW
EXECUTE FUNCTION propagate_post_deletion();
```

**Key Benefits**:
- ✅ 5-7× faster than re-aggregation
- ✅ Preserves array order
- ✅ Completes DELETE CRUD operation (was missing in v0.2.0)

---

## Example 5: INSERT - Add Array Element

**Scenario**: Add a new post to the feed (sorted by `created_at DESC`)

### Before - Example 5 (Traditional SQL - Re-aggregation)

```sql
-- Step 1: Insert new post
INSERT INTO tv_post (pk, id, fk_user, user_id, data)
VALUES (
    1001,
    gen_random_uuid(),
    5,
    (SELECT id FROM v_user WHERE pk = 5),
    jsonb_build_object(
        'id', gen_random_uuid(),
        'title', 'New Post',
        'created_at', now(),
        'content', 'Content here'
    )
);

-- Step 2: Re-aggregate feed (rebuild entire array)
UPDATE tv_feed
SET data = jsonb_build_object(
    'posts',
    (
        SELECT jsonb_agg(data ORDER BY (data->>'created_at')::timestamptz DESC)
        FROM tv_post
        LIMIT 100
    )
)
WHERE pk = 1;

-- Performance: ~22-35ms (re-aggregates 100 posts)
```

### After (jsonb_delta - Sorted Insertion)

```sql
-- Step 1: Insert new post (same)
INSERT INTO tv_post (pk, id, fk_user, user_id, data)
VALUES (
    1001,
    gen_random_uuid(),
    5,
    (SELECT id FROM v_user WHERE pk = 5),
    jsonb_build_object(
        'id', gen_random_uuid(),
        'title', 'New Post',
        'created_at', now(),
        'content', 'Content here'
    )
);

-- Step 2: Surgically insert into feed (maintains sort order)
UPDATE tv_feed
SET data = jsonb_array_insert_where(
    data,
    'posts',
    (SELECT data FROM tv_post WHERE pk = 1001),
    'created_at',  -- sort by this field
    'DESC'         -- descending order
)
WHERE pk = 1;

-- Performance: ~5-8ms (inserts 1 element, maintains order)
```

**Key Features**:
- ✅ Automatically maintains sort order (no manual sorting needed)
- ✅ 4-6× faster than re-aggregation
- ✅ Completes INSERT CRUD operation (was missing in v0.2.0)
- ✅ `sort_key` and `sort_order` are optional (can be NULL for unsorted)

**Unsorted insertion** (when order doesn't matter):

```sql
UPDATE tv_feed
SET data = jsonb_array_insert_where(
    data,
    'posts',
    (SELECT data FROM tv_post WHERE pk = 1001),
    NULL,  -- no sorting
    NULL
)
WHERE pk = 1;
```

---

## Example 6: Deep Merge - Nested Dependencies

**Scenario**: Update company affects posts through nested author.company structure

### Before (Traditional SQL - Manual Path Manipulation)

```sql
-- Update posts with new company data (nested 2 levels deep)
UPDATE tv_post
SET data = jsonb_set(
    jsonb_set(
        data,
        '{author,company,name}',
        '"New Company Name"'::jsonb
    ),
    '{author,company,industry}',
    '"New Industry"'::jsonb
)
WHERE fk_user IN (SELECT pk FROM tv_user WHERE fk_company = 1);

-- Problem: Must specify every changed field explicitly
-- Problem: Overwrites other company fields if not careful
-- Performance: O(n × k) where k = number of updated fields
```

### After - Deep Merge Pattern (jsonb_delta - Deep Merge)

```sql
-- Recursively merge company changes at any depth
UPDATE tv_post
SET data = jsonb_deep_merge(
    data,
    jsonb_build_object(
        'author',
        jsonb_build_object(
            'company',
            (SELECT data FROM v_company WHERE pk = 1)
        )
    )
)
WHERE fk_user IN (SELECT pk FROM tv_user WHERE fk_company = 1);

-- Performance: O(n) single-pass deep merge
```

**Key Benefits**:
- ✅ Preserves all non-updated nested fields automatically
- ✅ Single function call vs multiple `jsonb_set()` operations
- ✅ 2× faster than manual path manipulation
- ✅ Handles arbitrary nesting depth

---

## Function Selection Guide

### Decision Tree: Which Function to Use?

```text
What are you updating?
├─ Top-level scalar field (e.g., user.name)
│  └─ Use: jsonb_smart_patch_scalar()
│
├─ Nested object (e.g., user.company)
│  ├─ Single level deep?
│  │  └─ Use: jsonb_smart_patch_nested()
│  └─ Multiple levels deep?
│     └─ Use: jsonb_deep_merge()
│
├─ Array element (e.g., feed.posts[id=42])
│  ├─ UPDATE existing element?
│  │  └─ Use: jsonb_smart_patch_array()
│  ├─ DELETE element?
│  │  └─ Use: jsonb_array_delete_where()
│  └─ INSERT new element?
│     └─ Use: jsonb_array_insert_where()
│
└─ Batch operations?
   ├─ Multiple elements in same array?
   │  └─ Use: jsonb_array_update_where_batch()
   └─ Same element across multiple rows?
      └─ Use: jsonb_array_update_multi_row()
```

---

## Error Handling

### NULL Behavior

Most functions are marked `STRICT`, meaning they return `NULL` if **any** parameter is `NULL`:

```sql
-- These all return NULL (not errors)
SELECT jsonb_smart_patch_scalar(NULL, '{"name": "test"}'::jsonb);  -- NULL
SELECT jsonb_smart_patch_scalar('{"a": 1}'::jsonb, NULL);          -- NULL
```

**Exception**: `jsonb_array_insert_where()` allows NULL for `sort_key` and `sort_order`:

```sql
-- This is valid (unsorted insertion)
SELECT jsonb_array_insert_where(
    '{"posts": []}'::jsonb,
    'posts',
    '{"id": 1}'::jsonb,
    NULL,  -- OK: means "don't sort"
    NULL   -- OK
);
```

### Missing Paths/Keys

Functions return the **original JSONB unchanged** if paths/keys don't exist:

```sql
-- Array path doesn't exist → returns original
SELECT jsonb_array_delete_where(
    '{"other": "data"}'::jsonb,
    'posts',  -- doesn't exist
    'id',
    '42'::jsonb
);
-- Result: {"other": "data"}
```

---

## Performance Tuning

### 1. Use Containment Checks to Filter Rows

**Bad** (updates all rows):

```sql
UPDATE tv_feed
SET data = jsonb_array_delete_where(data, 'posts', 'id', '42'::jsonb);
```

**Good** (only updates affected rows):

```sql
UPDATE tv_feed
SET data = jsonb_array_delete_where(data, 'posts', 'id', '42'::jsonb)
WHERE jsonb_array_contains_id(data, 'posts', 'id', '42'::jsonb);
```

### 2. Create Indexes on Foreign Keys

```sql
-- Essential for fast propagation queries
CREATE INDEX idx_tv_user_fk_company ON tv_user(fk_company);
CREATE INDEX idx_tv_post_fk_user ON tv_post(fk_user);

-- Enable fast filtering with jsonb_array_contains_id()
CREATE INDEX idx_tv_feed_posts_gin ON tv_feed USING gin((data->'posts'));
```

---

## Summary

| Before (Native SQL) | After (jsonb_delta) | Speedup |
|---------------------|-------------------|---------|
| Re-aggregate 100-element array | Surgical array update | **5-7×** |
| Multiple `jsonb_set()` for nested | Deep merge | **2×** |
| Manual path manipulation | Smart patch API | **3-4×** |
| Re-build feed on INSERT/DELETE | Surgical INSERT/DELETE | **4-6×** |

**Key Takeaways**:
- ✅ Complete CRUD support (INSERT/DELETE now available)
- ✅ 40-60% code reduction in refresh logic
- ✅ 2-7× performance improvements
- ✅ Simplified API with smart patch functions
- ✅ Production-ready error handling

For more examples, see:
- `test/benchmark_pg_tview_helpers.sql` - Complete benchmark suite
- `test/smoke_test_v0.1.0.sql` - Quick validation tests
- `README.md` - API reference and quick start
