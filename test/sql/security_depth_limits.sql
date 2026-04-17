-- test/sql/security_depth_limits.sql
-- Security tests for JSONB depth limits to prevent DoS attacks
-- Note: PostgreSQL itself has JSON parsing limits, so deep nesting tests
-- are handled by Rust unit tests. These SQL tests verify basic functionality.

-- Test 1: Basic deep merge should work (shallow structure)
SELECT jsonb_deep_merge(
    '{"a": 1}'::jsonb,
    '{"b": 2}'::jsonb
);

-- Test 2: Basic shallow merge should work
SELECT jsonb_merge_shallow(
    '{"a": 1}'::jsonb,
    '{"b": 2}'::jsonb
);

-- Test 3: Array update should work
SELECT jsonb_array_update_where(
    '{"items": [{"id": 1, "name": "Alice"}]}'::jsonb,
    'items',  -- array_path (key containing the array)
    'id',     -- match_key (key to match on)
    '1'::jsonb,  -- match_value (value to match)
    '{"name": "Bob"}'::jsonb  -- updates (object to merge)
);

-- Test 4: Array delete should work
SELECT jsonb_array_delete_where(
    '{"items": [{"id": 1}, {"id": 2}]}'::jsonb,
    'items',  -- array_path
    'id',     -- match_key
    '1'::jsonb  -- match_value
);

-- Test 5: Nested structures within limits should work
SELECT jsonb_deep_merge(
    '{"user": {"profile": {"name": "Alice"}}}'::jsonb,
    '{"user": {"profile": {"age": 30}}}'::jsonb
);

-- Test 6: Array index bound enforcement
SELECT jsonb_delta_set_path(
    '{"a": []}'::jsonb,
    'a[200000]',
    '1'::jsonb
);
-- Expected: ERROR containing "Array index"

-- Test 7: Empty match_key rejected
DO $$
BEGIN
    PERFORM jsonb_array_update_where(
        '{"items": [{"id": 1}]}'::jsonb,
        'items',
        '',                  -- empty match_key
        '1'::jsonb,
        '{"updated": true}'::jsonb
    );
    RAISE EXCEPTION 'Expected error but got none';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%empty%' THEN
        RAISE EXCEPTION 'Unexpected error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_empty_match_key_rejected' AS passed;

-- Test 8: Long key segment rejected
DO $$
DECLARE
    long_path text := repeat('a', 257) || '.b';
BEGIN
    PERFORM jsonb_delta_set_path('{}'::jsonb, long_path, '1'::jsonb);
    RAISE EXCEPTION 'Expected error but got none';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%exceeds%' AND sqlerrm NOT LIKE '%256%' THEN
        RAISE EXCEPTION 'Unexpected error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_long_key_segment_rejected' AS passed;
