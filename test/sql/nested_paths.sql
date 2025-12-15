-- Test Suite: Nested Path Support (Phase 3)
-- Expected: All tests fail initially (functions don't exist yet)

CREATE EXTENSION IF NOT EXISTS jsonb_delta;

-- Test 1: Dot notation - update nested field in array element
SELECT jsonb_delta_array_update_where_path(
    '{"users": [{"id": 1, "profile": {"name": "Alice"}}]}'::jsonb,
    'users',
    'id', '1'::jsonb,
    'profile.name',       -- NESTED PATH (fails initially)
    '"Bob"'::jsonb
);
-- Expected: {"users": [{"id": 1, "profile": {"name": "Bob"}}]}

-- Test 2: Array indexing - update nested array element
SELECT jsonb_delta_set_path(
    '{"orders": [{"items": [{"price": 10}]}]}'::jsonb,
    'orders[0].items[0].price',  -- NESTED PATH (fails initially)
    '20'::jsonb
);
-- Expected: {"orders": [{"items": [{"price": 20}]}]}

-- Test 3: Mixed paths - complex nested navigation
SELECT jsonb_delta_array_update_where_path(
    '{"companies": [{"id": 1, "departments": [{"name": "engineering", "employees": [{"name": "Alice", "salary": 50000}]}]}]}'::jsonb,
    'companies',
    'id', '1'::jsonb,
    'departments[0].employees[0].salary',  -- NESTED PATH
    '60000'::jsonb
);
-- Expected: {"companies": [{"id": 1, "departments": [{"name": "engineering", "employees": [{"name": "Alice", "salary": 60000}]}]}]}

-- Test 4: Simple dot notation (no arrays)
SELECT jsonb_delta_set_path(
    '{"user": {"profile": {"settings": {"theme": "light"}}}}'::jsonb,
    'user.profile.settings.theme',  -- NESTED PATH
    '"dark"'::jsonb
);
-- Expected: {"user": {"profile": {"settings": {"theme": "dark"}}}}

-- Test 5: Array indexing only
SELECT jsonb_delta_set_path(
    '{"items": [{"name": "item1"}, {"name": "item2"}]}'::jsonb,
    'items[1].name',  -- NESTED PATH
    '"updated_item2"'::jsonb
);
-- Expected: {"items": [{"name": "item1"}, {"name": "updated_item2"}]}

-- Test 6: Error cases - invalid paths
SELECT jsonb_delta_set_path(
    '{"a": {"b": 1}}'::jsonb,
    'a.c',  -- Path doesn't exist
    '2'::jsonb
);
-- Expected: ERROR: Path segment 'c' not found in object

SELECT jsonb_delta_set_path(
    '{"a": [1, 2, 3]}'::jsonb,
    'a[10]',  -- Index out of bounds
    '4'::jsonb
);
-- Expected: ERROR: Array index 10 out of bounds (length 3)

SELECT jsonb_delta_set_path(
    '{"a": {"b": 1}}'::jsonb,
    'a[0]',  -- Trying to index object
    '2'::jsonb
);
-- Expected: ERROR: Cannot index into object at path 'a'

\echo 'All tests should fail initially with "function does not exist" errors'
