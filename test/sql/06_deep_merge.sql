-- Test jsonb_deep_merge function
-- Phase 3: Deep Merge & Helpers

\set ECHO all
\set ON_ERROR_STOP on

-- Clean up from previous test runs
DROP EXTENSION IF EXISTS jsonb_delta CASCADE;
CREATE EXTENSION jsonb_delta;

-- ===== BASIC DEEP MERGE TESTS =====

\echo '=== Test 1: Simple nested merge ==='
SELECT jsonb_deep_merge(
    '{"a": {"b": 1, "c": 2}}'::jsonb,
    '{"a": {"c": 3, "d": 4}}'::jsonb
);
-- Expected: {"a": {"b": 1, "c": 3, "d": 4}}

\echo '=== Test 2: Deep nested merge (3 levels) ==='
SELECT jsonb_deep_merge(
    '{"level1": {"level2": {"level3": {"a": 1, "b": 2}}}}'::jsonb,
    '{"level1": {"level2": {"level3": {"b": 99, "c": 3}}}}'::jsonb
);
-- Expected: {"level1": {"level2": {"level3": {"a": 1, "b": 99, "c": 3}}}}

\echo '=== Test 3: Array replacement (not merged) ==='
SELECT jsonb_deep_merge(
    '{"items": [1, 2, 3]}'::jsonb,
    '{"items": [4, 5]}'::jsonb
);
-- Expected: {"items": [4, 5]}

\echo '=== Test 4: Mixed types (object replaced by string) ==='
SELECT jsonb_deep_merge(
    '{"a": {"b": 1}}'::jsonb,
    '{"a": "replaced"}'::jsonb
);
-- Expected: {"a": "replaced"}

\echo '=== Test 5: Preserves sibling fields ==='
SELECT jsonb_deep_merge(
    '{"user": {"name": "Alice", "company": {"name": "ACME", "city": "NYC"}}}'::jsonb,
    '{"user": {"company": {"name": "ACME Corp"}}}'::jsonb
);
-- Expected: {"user": {"name": "Alice", "company": {"name": "ACME Corp", "city": "NYC"}}}

\echo '=== Test 6: Empty source ==='
SELECT jsonb_deep_merge(
    '{"a": {"b": 1}}'::jsonb,
    '{}'::jsonb
);
-- Expected: {"a": {"b": 1}}

\echo '=== Test 7: Empty target ==='
SELECT jsonb_deep_merge(
    '{}'::jsonb,
    '{"a": {"b": 1}}'::jsonb
);
-- Expected: {"a": {"b": 1}}

\echo '=== Test 8: Multiple root keys ==='
SELECT jsonb_deep_merge(
    '{"a": 1, "b": {"c": 2}}'::jsonb,
    '{"b": {"d": 3}, "e": 4}'::jsonb
);
-- Expected: {"a": 1, "b": {"c": 2, "d": 3}, "e": 4}

\echo '=== Test 9: Deep merge vs shallow merge comparison ==='
-- Show the difference between deep and shallow merge
SELECT
    jsonb_merge_shallow(
        '{"user": {"name": "Alice", "company": {"name": "ACME", "city": "NYC"}}}'::jsonb,
        '{"user": {"company": {"name": "ACME Corp"}}}'::jsonb
    ) AS shallow_result,
    jsonb_deep_merge(
        '{"user": {"name": "Alice", "company": {"name": "ACME", "city": "NYC"}}}'::jsonb,
        '{"user": {"company": {"name": "ACME Corp"}}}'::jsonb
    ) AS deep_result;
-- Shallow loses "name" and "city", Deep preserves them

\echo '=== All deep_merge tests passed! ==='
