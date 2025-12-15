-- =============================================================================
-- Smoke Test Suite for v0.1.0 JSONB Functions
-- =============================================================================
-- Quick validation that all v0.1.0 functions work correctly
-- Run with: psql -d your_database -f test/smoke_test_v0.1.0.sql
--
-- Expected output: All tests should return 't' (true)
-- =============================================================================

\echo ''
\echo '==================================================================='
\echo 'JSONB v0.1.0 Smoke Test Suite'
\echo '==================================================================='
\echo ''

-- -----------------------------------------------------------------------------
-- Phase 1: Smart Patch Functions
-- -----------------------------------------------------------------------------

\echo '--- Phase 1: Smart Patch Functions ---'
\echo ''

\echo 'Test 1.1: jsonb_smart_patch_scalar merges two objects'
SELECT jsonb_smart_patch_scalar(
    '{"a": 1, "b": 2}'::jsonb,
    '{"b": 3, "c": 4}'::jsonb
) = '{"a": 1, "b": 3, "c": 4}'::jsonb AS test_passed;

\echo 'Test 1.2: jsonb_smart_patch_nested updates nested object at path'
SELECT jsonb_smart_patch_nested(
    '{"user": {"name": "Alice", "age": 30}}'::jsonb,
    '{"age": 31, "city": "NYC"}'::jsonb,
    ARRAY['user']
) = '{"user": {"name": "Alice", "age": 31, "city": "NYC"}}'::jsonb AS test_passed;

\echo 'Test 1.3: jsonb_smart_patch_array updates element in array'
SELECT jsonb_smart_patch_array(
    '{"items": [{"id": 1, "name": "A"}, {"id": 2, "name": "B"}]}'::jsonb,
    '{"name": "Updated"}'::jsonb,
    'items',
    'id',
    '2'::jsonb
) -> 'items' -> 1 ->> 'name' = 'Updated' AS test_passed;

\echo ''

-- -----------------------------------------------------------------------------
-- Phase 2: Array CRUD Operations
-- -----------------------------------------------------------------------------

\echo '--- Phase 2: Array CRUD Operations ---'
\echo ''

\echo 'Test 2.1: jsonb_array_delete_where removes element by id'
SELECT jsonb_array_delete_where(
    '{"items": [{"id": 1}, {"id": 2}, {"id": 3}]}'::jsonb,
    'items',
    'id',
    '2'::jsonb
) -> 'items' -> 1 ->> 'id' = '3' AS test_passed;

\echo 'Test 2.2: jsonb_array_insert_where appends to empty array'
SELECT jsonb_array_insert_where(
    '{"items": []}'::jsonb,
    'items',
    '{"id": 1, "name": "First"}'::jsonb,
    NULL,
    NULL
) -> 'items' -> 0 ->> 'id' = '1' AS test_passed;

\echo 'Test 2.3: jsonb_array_insert_where with sorting'
SELECT jsonb_array_insert_where(
    '{"posts": [{"id": 1, "created_at": "2025-12-01"}]}'::jsonb,
    'posts',
    '{"id": 2, "created_at": "2025-12-08"}'::jsonb,
    'created_at',
    'DESC'
) -> 'posts' -> 0 ->> 'id' = '2' AS test_passed;

\echo ''

-- -----------------------------------------------------------------------------
-- Phase 3: Deep Operations
-- -----------------------------------------------------------------------------

\echo '--- Phase 3: Deep Operations ---'
\echo ''

\echo 'Test 3.1: jsonb_deep_merge preserves nested fields'
SELECT jsonb_deep_merge(
    '{"a": 1, "b": {"c": 2, "d": 3}}'::jsonb,
    '{"b": {"d": 4, "e": 5}}'::jsonb
) = '{"a": 1, "b": {"c": 2, "d": 4, "e": 5}}'::jsonb AS test_passed;

\echo 'Test 3.2: jsonb_deep_merge handles deep nesting'
SELECT jsonb_deep_merge(
    '{"a": {"b": {"c": 1}}}'::jsonb,
    '{"a": {"b": {"d": 2}}}'::jsonb
) = '{"a": {"b": {"c": 1, "d": 2}}}'::jsonb AS test_passed;

\echo ''

-- -----------------------------------------------------------------------------
-- Phase 4: Helper Functions
-- -----------------------------------------------------------------------------

\echo '--- Phase 4: Helper Functions ---'
\echo ''

\echo 'Test 4.1: jsonb_extract_id extracts id (default key)'
SELECT jsonb_extract_id(
    '{"id": "abc-123", "name": "test"}'::jsonb
) = 'abc-123' AS test_passed;

\echo 'Test 4.2: jsonb_extract_id with custom key'
SELECT jsonb_extract_id(
    '{"userId": "xyz-456", "name": "test"}'::jsonb,
    'userId'
) = 'xyz-456' AS test_passed;

\echo 'Test 4.3: jsonb_array_contains_id finds element'
SELECT jsonb_array_contains_id(
    '{"items": [{"id": 1}, {"id": 2}, {"id": 3}]}'::jsonb,
    'items',
    'id',
    '2'::jsonb
) = true AS test_passed;

\echo 'Test 4.4: jsonb_array_contains_id returns false if not found'
SELECT jsonb_array_contains_id(
    '{"items": [{"id": 1}, {"id": 2}]}'::jsonb,
    'items',
    'id',
    '99'::jsonb
) = false AS test_passed;

\echo ''

-- -----------------------------------------------------------------------------
-- Phase 5: v0.2.0 Batch Operations
-- -----------------------------------------------------------------------------

\echo '--- Phase 5: Batch Operations (v0.2.0) ---'
\echo ''

\echo 'Test 5.1: jsonb_array_update_where updates single element'
SELECT jsonb_array_update_where(
    '{"dns": [{"id": 1, "ip": "1.1.1.1"}, {"id": 2, "ip": "2.2.2.2"}]}'::jsonb,
    'dns',
    'id',
    '1'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb
) -> 'dns' -> 0 ->> 'ip' = '8.8.8.8' AS test_passed;

\echo 'Test 5.2: jsonb_array_update_where_batch updates multiple elements'
SELECT jsonb_array_update_where_batch(
    '{"items": [{"id": 1, "x": 0}, {"id": 2, "x": 0}, {"id": 3, "x": 0}]}'::jsonb,
    'items',
    'id',
    '[{"id": 1, "x": 99}, {"id": 3, "x": 88}]'::jsonb
) -> 'items' -> 2 ->> 'x' = '88' AS test_passed;

\echo 'Test 5.3: jsonb_merge_at_path merges at specific path'
SELECT jsonb_merge_at_path(
    '{"user": {"profile": {"name": "Alice"}}}'::jsonb,
    '{"age": 30}'::jsonb,
    ARRAY['user', 'profile']
) -> 'user' -> 'profile' ->> 'age' = '30' AS test_passed;

\echo 'Test 5.4: jsonb_merge_shallow performs shallow merge'
SELECT jsonb_merge_shallow(
    '{"a": 1, "b": {"c": 2}}'::jsonb,
    '{"b": {"d": 3}}'::jsonb
) -> 'b' ->> 'c' IS NULL AS test_passed;  -- c is lost in shallow merge

\echo ''

-- -----------------------------------------------------------------------------
-- Edge Cases
-- -----------------------------------------------------------------------------

\echo '--- Edge Cases ---'
\echo ''

\echo 'Test E.1: Empty array in jsonb_array_delete_where'
SELECT jsonb_array_delete_where(
    '{"items": []}'::jsonb,
    'items',
    'id',
    '1'::jsonb
) = '{"items": []}'::jsonb AS test_passed;

\echo 'Test E.2: Delete non-existent element (no-op)'
SELECT jsonb_array_delete_where(
    '{"items": [{"id": 1}]}'::jsonb,
    'items',
    'id',
    '99'::jsonb
) = '{"items": [{"id": 1}]}'::jsonb AS test_passed;

\echo 'Test E.3: Insert into non-existent array path (should work)'
SELECT jsonb_array_insert_where(
    '{}'::jsonb,
    'new_array',
    '{"id": 1}'::jsonb,
    NULL,
    NULL
) -> 'new_array' -> 0 ->> 'id' = '1' AS test_passed;

\echo ''
\echo '==================================================================='
\echo 'Smoke Test Complete'
\echo '==================================================================='
\echo ''
\echo 'All tests should show "t" (true) for test_passed.'
\echo 'If any test shows "f" (false), investigate the corresponding function.'
\echo ''
