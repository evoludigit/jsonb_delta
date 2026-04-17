CREATE EXTENSION IF NOT EXISTS jsonb_delta;

-- 1. jsonb_merge_shallow with array argument
DO $$
BEGIN
    PERFORM jsonb_merge_shallow('[1,2]'::jsonb, '{"b":2}'::jsonb);
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%object%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_01_merge_shallow_rejects_array' AS passed;

-- 2. jsonb_merge_shallow with scalar argument
DO $$
BEGIN
    PERFORM jsonb_merge_shallow('42'::jsonb, '{"b":2}'::jsonb);
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%object%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_02_merge_shallow_rejects_scalar' AS passed;

-- 3. jsonb_array_update_where with non-existent path
DO $$
BEGIN
    PERFORM jsonb_array_update_where(
        '{"items": [{"id":1}]}'::jsonb,
        'missing_path',
        'id', '1'::jsonb,
        '{"x":1}'::jsonb
    );
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%does not exist%' AND sqlerrm NOT LIKE '%missing_path%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_03_array_update_nonexistent_path' AS passed;

-- 4. jsonb_array_update_where with path pointing to object (not array)
DO $$
BEGIN
    PERFORM jsonb_array_update_where(
        '{"user": {"id":1}}'::jsonb,
        'user',
        'id', '1'::jsonb,
        '{"x":1}'::jsonb
    );
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%array%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_04_array_update_path_not_array' AS passed;

-- 5. jsonb_array_update_where with non-object updates
DO $$
BEGIN
    PERFORM jsonb_array_update_where(
        '{"items":[{"id":1}]}'::jsonb,
        'items',
        'id', '1'::jsonb,
        '[1,2,3]'::jsonb      -- updates must be an object
    );
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%object%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_05_array_update_non_object_updates' AS passed;

-- 6. jsonb_merge_at_path with non-object source
DO $$
BEGIN
    PERFORM jsonb_merge_at_path(
        '{"a": {"b":1}}'::jsonb,
        '[1,2,3]'::jsonb,      -- patch must be an object
        ARRAY['a']
    );
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%object%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_06_merge_at_path_non_object_patch' AS passed;

-- 7. jsonb_merge_at_path with non-object at path
DO $$
BEGIN
    PERFORM jsonb_merge_at_path(
        '{"a": [1,2,3]}'::jsonb,   -- 'a' is an array, not an object
        '{"x":1}'::jsonb,
        ARRAY['a']
    );
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%object%' AND sqlerrm NOT LIKE '%array%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_07_merge_at_path_target_not_object' AS passed;

-- 8. jsonb_deep_merge with non-object replaces (not errors)
-- deep_merge source-replaces non-objects — this should NOT error
SELECT
    jsonb_deep_merge('{"a": 1}'::jsonb, '{"a": {"b": 2}}'::jsonb)
    = '{"a": {"b": 2}}'::jsonb
    AS test_08_deep_merge_replaces_scalar;

-- 9. jsonb_array_insert_where on non-object target
DO $$
BEGIN
    PERFORM jsonb_array_insert_where('[1,2,3]'::jsonb, 'items', '{"id":1}'::jsonb, NULL, NULL);
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%object%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_09_array_insert_non_object_target' AS passed;

-- 10. jsonb_array_insert_where on path pointing to non-array
DO $$
BEGIN
    PERFORM jsonb_array_insert_where(
        '{"items": "not-an-array"}'::jsonb,
        'items',
        '{"id":1}'::jsonb,
        NULL, NULL
    );
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%array%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_10_array_insert_path_not_array' AS passed;

-- 11. jsonb_delta_set_path with empty path string
DO $$
BEGIN
    PERFORM jsonb_delta_set_path('{}'::jsonb, '', '1'::jsonb);
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%empty%' AND sqlerrm NOT LIKE '%path%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_11_set_path_empty_path' AS passed;

-- 12. jsonb_delta_set_path with invalid path syntax (consecutive dots)
DO $$
BEGIN
    PERFORM jsonb_delta_set_path('{}'::jsonb, 'a..b', '1'::jsonb);
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%path%' AND sqlerrm NOT LIKE '%dot%' AND sqlerrm NOT LIKE '%Invalid%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_12_set_path_invalid_syntax' AS passed;

-- 13. jsonb_delta_array_update_where_path with non-existent array
DO $$
BEGIN
    PERFORM jsonb_delta_array_update_where_path(
        '{"users": [{"id":1}]}'::jsonb,
        'missing',
        'id', '1'::jsonb,
        'name',
        '"Alice"'::jsonb
    );
    RAISE EXCEPTION 'Expected error';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%missing%' AND sqlerrm NOT LIKE '%exist%' THEN
        RAISE EXCEPTION 'Wrong error: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_13_array_update_where_path_missing_array' AS passed;

-- 14. jsonb_extract_id with non-object returns NULL
SELECT
    jsonb_extract_id('[1,2,3]'::jsonb, 'id') IS NULL
    AS test_14_extract_id_non_object_returns_null;

-- 15. jsonb_array_contains_id with non-object element
-- The function should handle gracefully (return false or similar)
SELECT
    jsonb_array_contains_id(
        '{"items": [1, 2, 3]}'::jsonb,    -- array of scalars, not objects
        'items',
        'id',
        '1'::jsonb
    ) = false
    AS test_15_contains_id_non_object_elements;