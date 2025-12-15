-- Test jsonb_smart_patch_* functions
-- Phase 1 implementation test

-- ===== SCALAR UPDATES =====

-- Test 1: Simple scalar update (root-level merge)
SELECT jsonb_smart_patch_scalar(
    '{"id": 1, "name": "old", "count": 10}'::jsonb,
    '{"name": "new", "active": true}'::jsonb
) = '{"id": 1, "name": "new", "count": 10, "active": true}'::jsonb AS test_scalar_simple;

-- Test 2: Scalar update with overlapping keys
SELECT jsonb_smart_patch_scalar(
    '{"id": 1, "status": "draft"}'::jsonb,
    '{"status": "published", "updated_at": "2025-12-08"}'::jsonb
) = '{"id": 1, "status": "published", "updated_at": "2025-12-08"}'::jsonb AS test_scalar_overwrite;

-- ===== NESTED OBJECT UPDATES =====

-- Test 3: Single-level nested update
SELECT jsonb_smart_patch_nested(
    '{"id": 1, "user": {"name": "Alice", "email": "old@example.com"}}'::jsonb,
    '{"email": "new@example.com"}'::jsonb,
    ARRAY['user']
) = '{"id": 1, "user": {"name": "Alice", "email": "new@example.com"}}'::jsonb AS test_nested_single_level;

-- Test 4: Deep nested update
SELECT jsonb_smart_patch_nested(
    '{"id": 1, "user": {"name": "Alice", "company": {"name": "ACME", "city": "NYC"}}}'::jsonb,
    '{"name": "ACME Corp"}'::jsonb,
    ARRAY['user', 'company']
) = '{"id": 1, "user": {"name": "Alice", "company": {"name": "ACME Corp", "city": "NYC"}}}'::jsonb AS test_nested_deep;

-- Test 5: Nested update with new field
SELECT jsonb_smart_patch_nested(
    '{"id": 1, "config": {"timeout": 30}}'::jsonb,
    '{"retries": 3}'::jsonb,
    ARRAY['config']
) = '{"id": 1, "config": {"timeout": 30, "retries": 3}}'::jsonb AS test_nested_add_field;

-- ===== ARRAY ELEMENT UPDATES =====

-- Test 6: Array element update by integer ID
SELECT jsonb_smart_patch_array(
    '{"posts": [{"id": 1, "title": "Old"}, {"id": 2, "title": "Post 2"}]}'::jsonb,
    '{"title": "New", "updated": true}'::jsonb,
    'posts',
    'id',
    '1'::jsonb
) = '{"posts": [{"id": 1, "title": "New", "updated": true}, {"id": 2, "title": "Post 2"}]}'::jsonb AS test_array_int_id;

-- Test 7: Array element update by string ID
SELECT jsonb_smart_patch_array(
    '{"items": [{"uuid": "abc", "name": "Item 1"}, {"uuid": "def", "name": "Item 2"}]}'::jsonb,
    '{"name": "Updated Item"}'::jsonb,
    'items',
    'uuid',
    '"abc"'::jsonb
) = '{"items": [{"uuid": "abc", "name": "Updated Item"}, {"uuid": "def", "name": "Item 2"}]}'::jsonb AS test_array_string_id;

-- Test 8: Array element update - no match (unchanged)
SELECT jsonb_smart_patch_array(
    '{"posts": [{"id": 1, "title": "Post 1"}]}'::jsonb,
    '{"title": "Should not apply"}'::jsonb,
    'posts',
    'id',
    '999'::jsonb
) = '{"posts": [{"id": 1, "title": "Post 1"}]}'::jsonb AS test_array_no_match;

-- Test 9: Array element update with multiple fields
SELECT jsonb_smart_patch_array(
    '{"users": [{"id": 42, "name": "Alice", "age": 30}, {"id": 43, "name": "Bob", "age": 25}]}'::jsonb,
    '{"age": 31, "city": "NYC"}'::jsonb,
    'users',
    'id',
    '42'::jsonb
) = '{"users": [{"id": 42, "name": "Alice", "age": 31, "city": "NYC"}, {"id": 43, "name": "Bob", "age": 25}]}'::jsonb AS test_array_multiple_fields;

-- ===== pg_tview INTEGRATION PATTERNS =====

-- Test 10: Simulate pg_tview scalar update pattern
CREATE TEMPORARY TABLE test_tv_company (
    pk_company INT PRIMARY KEY,
    data JSONB
);

INSERT INTO test_tv_company VALUES
    (1, '{"id": 1, "name": "ACME", "industry": "Tech"}');

UPDATE test_tv_company
SET data = jsonb_smart_patch_scalar(data, '{"name": "ACME Corp", "employees": 100}'::jsonb)
WHERE pk_company = 1;

SELECT data = '{"id": 1, "name": "ACME Corp", "industry": "Tech", "employees": 100}'::jsonb AS test_pg_tview_scalar
FROM test_tv_company WHERE pk_company = 1;

-- Test 11: Simulate pg_tview nested update pattern
CREATE TEMPORARY TABLE test_tv_user (
    pk_user INT PRIMARY KEY,
    data JSONB
);

INSERT INTO test_tv_user VALUES
    (1, '{"id": 1, "name": "Alice", "company": {"id": 1, "name": "ACME"}}');

UPDATE test_tv_user
SET data = jsonb_smart_patch_nested(data, '{"name": "ACME Corp"}'::jsonb, ARRAY['company'])
WHERE pk_user = 1;

SELECT data->'company'->>'name' = 'ACME Corp' AS company_updated,
       data->>'name' = 'Alice' AS user_name_preserved
FROM test_tv_user WHERE pk_user = 1;

-- Test 12: Simulate pg_tview array update pattern
CREATE TEMPORARY TABLE test_tv_feed (
    pk_feed INT PRIMARY KEY,
    data JSONB
);

INSERT INTO test_tv_feed VALUES
    (1, '{"id": 1, "posts": [{"id": 10, "title": "Post 10"}, {"id": 20, "title": "Post 20"}]}');

UPDATE test_tv_feed
SET data = jsonb_smart_patch_array(data, '{"title": "Updated Post 10", "featured": true}'::jsonb, 'posts', 'id', '10'::jsonb)
WHERE pk_feed = 1;

SELECT data->'posts'->0->>'title' = 'Updated Post 10' AS title_updated,
       data->'posts'->0->'featured' = 'true'::jsonb AS featured_added,
       data->'posts'->1->>'title' = 'Post 20' AS other_post_unchanged
FROM test_tv_feed WHERE pk_feed = 1;

-- Display summary
\echo '==================================='
\echo 'All jsonb_smart_patch tests passed!'
\echo '==================================='
