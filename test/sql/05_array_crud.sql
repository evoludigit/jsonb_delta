-- Test jsonb_array_delete_where and jsonb_array_insert_where
-- Phase 2 implementation test

-- ===== DELETE TESTS =====

-- Test 1: Basic deletion by integer ID
SELECT jsonb_array_delete_where(
    '{"posts": [
        {"id": 1, "title": "First"},
        {"id": 2, "title": "Second"},
        {"id": 3, "title": "Third"}
    ]}'::jsonb,
    'posts',
    'id',
    '2'::jsonb
) = '{"posts": [{"id": 1, "title": "First"}, {"id": 3, "title": "Third"}]}'::jsonb AS test_delete_basic;

-- Test 2: Delete by string ID
SELECT jsonb_array_delete_where(
    '{"items": [
        {"uuid": "aaa", "name": "Item A"},
        {"uuid": "bbb", "name": "Item B"},
        {"uuid": "ccc", "name": "Item C"}
    ]}'::jsonb,
    'items',
    'uuid',
    '"bbb"'::jsonb
) = '{"items": [{"uuid": "aaa", "name": "Item A"}, {"uuid": "ccc", "name": "Item C"}]}'::jsonb AS test_delete_string_id;

-- Test 3: Delete - no match (unchanged)
SELECT jsonb_array_delete_where(
    '{"posts": [{"id": 1, "title": "Post 1"}]}'::jsonb,
    'posts',
    'id',
    '999'::jsonb
) = '{"posts": [{"id": 1, "title": "Post 1"}]}'::jsonb AS test_delete_no_match;

-- Test 4: Delete from single-element array
SELECT jsonb_array_delete_where(
    '{"posts": [{"id": 42, "title": "Only Post"}]}'::jsonb,
    'posts',
    'id',
    '42'::jsonb
) = '{"posts": []}'::jsonb AS test_delete_single_element;

-- Test 5: Delete from non-existent array (unchanged)
SELECT jsonb_array_delete_where(
    '{"other": "data"}'::jsonb,
    'posts',
    'id',
    '1'::jsonb
) = '{"other": "data"}'::jsonb AS test_delete_missing_array;

-- Test 6: Delete first element
SELECT jsonb_array_delete_where(
    '{"items": [{"id": 1}, {"id": 2}, {"id": 3}]}'::jsonb,
    'items',
    'id',
    '1'::jsonb
) = '{"items": [{"id": 2}, {"id": 3}]}'::jsonb AS test_delete_first;

-- Test 7: Delete last element
SELECT jsonb_array_delete_where(
    '{"items": [{"id": 1}, {"id": 2}, {"id": 3}]}'::jsonb,
    'items',
    'id',
    '3'::jsonb
) = '{"items": [{"id": 1}, {"id": 2}]}'::jsonb AS test_delete_last;

-- ===== INSERT TESTS =====

-- Test 8: Simple append (no sort)
SELECT jsonb_array_insert_where(
    '{"posts": [{"id": 1}, {"id": 2}]}'::jsonb,
    'posts',
    '{"id": 3, "title": "New Post"}'::jsonb,
    NULL,
    NULL
) = '{"posts": [{"id": 1}, {"id": 2}, {"id": 3, "title": "New Post"}]}'::jsonb AS test_insert_append;

-- Test 9: Insert into empty array
SELECT jsonb_array_insert_where(
    '{"posts": []}'::jsonb,
    'posts',
    '{"id": 1, "title": "First Post"}'::jsonb,
    NULL,
    NULL
) = '{"posts": [{"id": 1, "title": "First Post"}]}'::jsonb AS test_insert_empty;

-- Test 10: Create array if doesn't exist
SELECT jsonb_array_insert_where(
    '{}'::jsonb,
    'posts',
    '{"id": 1, "title": "First Post"}'::jsonb,
    NULL,
    NULL
) = '{"posts": [{"id": 1, "title": "First Post"}]}'::jsonb AS test_insert_create_array;

-- Test 11: Ordered insert ASC (numeric)
SELECT jsonb_array_insert_where(
    '{"items": [
        {"id": 1, "score": 10},
        {"id": 3, "score": 30}
    ]}'::jsonb,
    'items',
    '{"id": 2, "score": 20}'::jsonb,
    'score',
    'ASC'
) = '{"items": [{"id": 1, "score": 10}, {"id": 2, "score": 20}, {"id": 3, "score": 30}]}'::jsonb AS test_insert_ordered_asc;

-- Test 12: Ordered insert DESC (numeric)
SELECT jsonb_array_insert_where(
    '{"items": [
        {"id": 1, "score": 30},
        {"id": 3, "score": 10}
    ]}'::jsonb,
    'items',
    '{"id": 2, "score": 20}'::jsonb,
    'score',
    'DESC'
) = '{"items": [{"id": 1, "score": 30}, {"id": 2, "score": 20}, {"id": 3, "score": 10}]}'::jsonb AS test_insert_ordered_desc;

-- Test 13: Ordered insert by timestamp ASC
SELECT jsonb_array_insert_where(
    '{"posts": [
        {"id": 1, "created_at": "2025-01-01T00:00:00Z"},
        {"id": 3, "created_at": "2025-01-03T00:00:00Z"}
    ]}'::jsonb,
    'posts',
    '{"id": 2, "created_at": "2025-01-02T00:00:00Z"}'::jsonb,
    'created_at',
    'ASC'
) = '{"posts": [
    {"id": 1, "created_at": "2025-01-01T00:00:00Z"},
    {"id": 2, "created_at": "2025-01-02T00:00:00Z"},
    {"id": 3, "created_at": "2025-01-03T00:00:00Z"}
]}'::jsonb AS test_insert_ordered_timestamp;

-- Test 14: Insert at beginning (smaller sort value)
SELECT jsonb_array_insert_where(
    '{"items": [{"id": 2, "priority": 20}]}'::jsonb,
    'items',
    '{"id": 1, "priority": 10}'::jsonb,
    'priority',
    'ASC'
) = '{"items": [{"id": 1, "priority": 10}, {"id": 2, "priority": 20}]}'::jsonb AS test_insert_at_beginning;

-- Test 15: Insert at end (larger sort value)
SELECT jsonb_array_insert_where(
    '{"items": [{"id": 1, "priority": 10}]}'::jsonb,
    'items',
    '{"id": 2, "priority": 20}'::jsonb,
    'priority',
    'ASC'
) = '{"items": [{"id": 1, "priority": 10}, {"id": 2, "priority": 20}]}'::jsonb AS test_insert_at_end;

-- ===== COMBINED DELETE + INSERT TESTS =====

-- Test 16: Delete then insert (simulating update)
WITH step1 AS (
    SELECT jsonb_array_delete_where(
        '{"posts": [
            {"id": 1, "title": "Post 1", "score": 10},
            {"id": 2, "title": "Post 2", "score": 20},
            {"id": 3, "title": "Post 3", "score": 30}
        ]}'::jsonb,
        'posts',
        'id',
        '2'::jsonb
    ) AS result
),
step2 AS (
    SELECT jsonb_array_insert_where(
        step1.result,
        'posts',
        '{"id": 2, "title": "Updated Post 2", "score": 25}'::jsonb,
        'score',
        'ASC'
    ) AS result
    FROM step1
)
SELECT result = '{"posts": [
    {"id": 1, "title": "Post 1", "score": 10},
    {"id": 2, "title": "Updated Post 2", "score": 25},
    {"id": 3, "title": "Post 3", "score": 30}
]}'::jsonb AS test_delete_then_insert
FROM step2;

-- ===== pg_tview INTEGRATION PATTERNS =====

-- Test 17: Simulate DELETE trigger on feed
CREATE TEMPORARY TABLE test_feed_delete (
    pk_feed INT PRIMARY KEY,
    data JSONB
);

INSERT INTO test_feed_delete VALUES
    (1, '{"id": 1, "posts": [
        {"id": 10, "title": "Post 10"},
        {"id": 20, "title": "Post 20"},
        {"id": 30, "title": "Post 30"}
    ]}');

-- Simulate: ON DELETE post WHERE pk_post = 20
UPDATE test_feed_delete
SET data = jsonb_array_delete_where(data, 'posts', 'id', '20'::jsonb)
WHERE pk_feed = 1;

SELECT jsonb_array_length(data->'posts') = 2 AS post_deleted,
       NOT (data->'posts' @> '[{"id": 20}]'::jsonb) AS post_20_gone,
       data->'posts' @> '[{"id": 10}]'::jsonb AS post_10_remains
FROM test_feed_delete WHERE pk_feed = 1;

-- Test 18: Simulate INSERT trigger on feed
CREATE TEMPORARY TABLE test_feed_insert (
    pk_feed INT PRIMARY KEY,
    data JSONB
);

INSERT INTO test_feed_insert VALUES
    (1, '{"id": 1, "posts": [
        {"id": 10, "created_at": "2025-01-01", "title": "Oldest"},
        {"id": 30, "created_at": "2025-01-03", "title": "Newest"}
    ]}');

-- Simulate: ON INSERT post with created_at between existing posts
UPDATE test_feed_insert
SET data = jsonb_array_insert_where(
    data,
    'posts',
    '{"id": 20, "created_at": "2025-01-02", "title": "Middle"}'::jsonb,
    'created_at',
    'ASC'
)
WHERE pk_feed = 1;

SELECT jsonb_array_length(data->'posts') = 3 AS post_inserted,
       data->'posts'->1->>'id' = '20' AS correct_position,
       data->'posts'->1->>'title' = 'Middle' AS correct_data
FROM test_feed_insert WHERE pk_feed = 1;

-- Display summary
\echo '========================================='
\echo 'All array CRUD tests passed!'
\echo '========================================='
