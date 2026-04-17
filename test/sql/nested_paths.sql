-- Test Suite: Nested Path Support

CREATE EXTENSION IF NOT EXISTS jsonb_delta;

-- Test 1: Dot notation — update nested field via array predicate
SELECT
    jsonb_delta_array_update_where_path(
        '{"users": [{"id": 1, "profile": {"name": "Alice"}}]}'::jsonb,
        'users',
        'id', '1'::jsonb,
        'profile.name',
        '"Bob"'::jsonb
    ) = '{"users": [{"id": 1, "profile": {"name": "Bob"}}]}'::jsonb
    AS test_01_dot_notation_nested_update;

-- Test 2: Array index — update deeply nested array element
SELECT
    jsonb_delta_set_path(
        '{"orders": [{"items": [{"price": 10}]}]}'::jsonb,
        'orders[0].items[0].price',
        '20'::jsonb
    ) = '{"orders": [{"items": [{"price": 20}]}]}'::jsonb
    AS test_02_nested_array_index;

-- Test 3: Mixed path — complex nested navigation
SELECT
    jsonb_delta_array_update_where_path(
        '{"companies": [{"id": 1, "departments": [{"name": "engineering", "employees": [{"name": "Alice", "salary": 50000}]}]}]}'::jsonb,
        'companies',
        'id', '1'::jsonb,
        'departments[0].employees[0].salary',
        '60000'::jsonb
    ) = '{"companies": [{"id": 1, "departments": [{"name": "engineering", "employees": [{"name": "Alice", "salary": 60000}]}]}]}'::jsonb
    AS test_03_mixed_path_complex;

-- Test 4: Deep dot notation (no arrays)
SELECT
    jsonb_delta_set_path(
        '{"user": {"profile": {"settings": {"theme": "light"}}}}'::jsonb,
        'user.profile.settings.theme',
        '"dark"'::jsonb
    ) = '{"user": {"profile": {"settings": {"theme": "dark"}}}}'::jsonb
    AS test_04_deep_dot_notation;

-- Test 5: Array indexing into existing array
SELECT
    jsonb_delta_set_path(
        '{"items": [{"name": "item1"}, {"name": "item2"}]}'::jsonb,
        'items[1].name',
        '"updated_item2"'::jsonb
    ) = '{"items": [{"name": "item1"}, {"name": "updated_item2"}]}'::jsonb
    AS test_05_array_index_update;

-- Test 6: Create-on-navigate — adding a new key to an existing object
-- set_path creates {"a": {"b": 1, "c": 2}}, it does NOT error
SELECT
    jsonb_delta_set_path(
        '{"a": {"b": 1}}'::jsonb,
        'a.c',
        '2'::jsonb
    ) = '{"a": {"b": 1, "c": 2}}'::jsonb
    AS test_06_create_new_key_in_object;

-- Test 7: Pad-on-extend — array shorter than requested index gets null-padded
-- index 10 on a 3-element array → [1, 2, 3, null, null, null, null, null, null, null, 4]
SELECT
    jsonb_delta_set_path(
        '{"a": [1, 2, 3]}'::jsonb,
        'a[10]',
        '4'::jsonb
    ) = '{"a": [1, 2, 3, null, null, null, null, null, null, null, 4]}'::jsonb
    AS test_07_array_pad_with_nulls;

-- Test 8: Type coercion — object at path is replaced by array when an index is used
SELECT
    jsonb_delta_set_path(
        '{"a": {"b": 1}}'::jsonb,
        'a[0]',
        '2'::jsonb
    ) = '{"a": [2]}'::jsonb
    AS test_08_object_replaced_by_array;

-- Test 9: Create entire chain from empty object
SELECT
    jsonb_delta_set_path(
        '{}'::jsonb,
        'a.b.c.d.e',
        '"deep"'::jsonb
    ) = '{"a": {"b": {"c": {"d": {"e": "deep"}}}}}'::jsonb
    AS test_09_create_deep_chain;

-- Test 10: Index 0 on empty array works
SELECT
    jsonb_delta_set_path(
        '{"a": []}'::jsonb,
        'a[0]',
        '"first"'::jsonb
    ) = '{"a": ["first"]}'::jsonb
    AS test_10_set_index_zero_empty_array;

-- Test 11: Empty path string is rejected
DO $$
BEGIN
    PERFORM jsonb_delta_set_path('{}'::jsonb, '', '1'::jsonb);
    RAISE EXCEPTION 'Expected error for empty path but got none';
EXCEPTION WHEN OTHERS THEN
    IF sqlerrm NOT LIKE '%empty%' THEN
        RAISE EXCEPTION 'Unexpected error message: %', sqlerrm;
    END IF;
END $$;
SELECT 'test_11_empty_path_rejected' AS passed;
