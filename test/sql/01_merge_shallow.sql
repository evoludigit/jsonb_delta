-- Test Suite: jsonb_merge_shallow()
-- Expected: All tests pass

CREATE EXTENSION jsonb_delta;

-- Test 1: Basic merge
SELECT jsonb_merge_shallow(
    '{"a": 1, "b": 2}'::jsonb,
    '{"c": 3}'::jsonb
);

-- Test 2: Overlapping keys (source overwrites target)
SELECT jsonb_merge_shallow(
    '{"a": 1, "b": 2}'::jsonb,
    '{"b": 99, "c": 3}'::jsonb
);

-- Test 3: Empty source
SELECT jsonb_merge_shallow(
    '{"a": 1}'::jsonb,
    '{}'::jsonb
);

-- Test 4: Empty target
SELECT jsonb_merge_shallow(
    '{}'::jsonb,
    '{"a": 1}'::jsonb
);

-- Test 5: Both empty
SELECT jsonb_merge_shallow(
    '{}'::jsonb,
    '{}'::jsonb
);

-- Test 6: NULL target
SELECT jsonb_merge_shallow(
    NULL::jsonb,
    '{"a": 1}'::jsonb
);

-- Test 7: NULL source
SELECT jsonb_merge_shallow(
    '{"a": 1}'::jsonb,
    NULL::jsonb
);

-- Test 8: Nested objects (shallow merge - overwrites nested object entirely)
SELECT jsonb_merge_shallow(
    '{"a": {"x": 1, "y": 2}, "b": 3}'::jsonb,
    '{"a": {"z": 3}}'::jsonb
);

-- Test 9: Different value types
SELECT jsonb_merge_shallow(
    '{"a": 1, "b": "text", "c": true}'::jsonb,
    '{"d": [1,2,3], "e": {"nested": "object"}}'::jsonb
);

-- Test 10: Large object (150 keys total)
WITH large_target AS (
    SELECT jsonb_object_agg('key' || i, i) AS obj
    FROM generate_series(1, 100) i
),
large_source AS (
    SELECT jsonb_object_agg('key' || i, i * 10) AS obj
    FROM generate_series(51, 150) i
)
SELECT count(*)
FROM (
    SELECT jsonb_object_keys(jsonb_merge_shallow(t.obj, s.obj)) AS key
    FROM large_target t, large_source s
) keys;

-- Test 11: Unicode support
SELECT jsonb_merge_shallow(
    '{"名前": "太郎"}'::jsonb,
    '{"ville": "Montréal", "emoji": "🚀"}'::jsonb
);

-- Test 12: Type validation - array should error
SELECT jsonb_merge_shallow(
    '[1,2,3]'::jsonb,
    '{"a": 1}'::jsonb
);
