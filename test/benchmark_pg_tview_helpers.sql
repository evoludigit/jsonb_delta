-- ===================================================================
-- jsonb_ivm v0.3.0 - pg_tview Helpers Benchmark Suite
-- ===================================================================

\timing on

-- Setup test environment
BEGIN;

CREATE TABLE bench_company (pk INT PRIMARY KEY, id UUID, name TEXT, industry TEXT);
CREATE TABLE bench_user (pk INT PRIMARY KEY, id UUID, fk_company INT, name TEXT, email TEXT);
CREATE TABLE bench_post (pk INT PRIMARY KEY, id UUID, fk_user INT, title TEXT, content TEXT, created_at TIMESTAMPTZ);

-- Insert test data
INSERT INTO bench_company VALUES
    (1, gen_random_uuid(), 'ACME Corp', 'Tech'),
    (2, gen_random_uuid(), 'Globex Inc', 'Finance');

INSERT INTO bench_user
SELECT
    i,
    gen_random_uuid(),
    ((i-1) % 2) + 1,  -- Alternate companies
    'User ' || i,
    'user' || i || '@example.com'
FROM generate_series(1, 100) i;

INSERT INTO bench_post
SELECT
    i,
    gen_random_uuid(),
    ((i-1) % 100) + 1,  -- Distribute across users
    'Post ' || i,
    'Content for post ' || i,
    now() - (i || ' minutes')::interval
FROM generate_series(1, 1000) i;

-- Create TVIEW-style tables
CREATE TABLE tv_company (pk INT PRIMARY KEY, id UUID, data JSONB);
CREATE TABLE tv_user (pk INT PRIMARY KEY, id UUID, fk_company INT, company_id UUID, data JSONB);
CREATE TABLE tv_post (pk INT PRIMARY KEY, id UUID, fk_user INT, user_id UUID, data JSONB);
CREATE TABLE tv_feed (pk INT PRIMARY KEY, data JSONB);

-- Populate tv_company
INSERT INTO tv_company
SELECT
    pk,
    id,
    jsonb_build_object('id', id, 'name', name, 'industry', industry)
FROM bench_company;

-- Populate tv_user
INSERT INTO tv_user
SELECT
    u.pk,
    u.id,
    u.fk_company,
    c.id,
    jsonb_build_object(
        'id', u.id,
        'name', u.name,
        'email', u.email,
        'company', tc.data
    )
FROM bench_user u
JOIN bench_company c ON c.pk = u.fk_company
JOIN tv_company tc ON tc.pk = u.fk_company;

-- Populate tv_post
INSERT INTO tv_post
SELECT
    p.pk,
    p.id,
    p.fk_user,
    u.id,
    jsonb_build_object(
        'id', p.id,
        'title', p.title,
        'content', p.content,
        'created_at', p.created_at,
        'author', tu.data
    )
FROM bench_post p
JOIN bench_user u ON u.pk = p.fk_user
JOIN tv_user tu ON tu.pk = p.fk_user;

-- Populate tv_feed (aggregated posts)
INSERT INTO tv_feed
SELECT
    1,
    jsonb_build_object(
        'posts',
        jsonb_agg(data ORDER BY (data->>'created_at')::timestamptz DESC)
    )
FROM (SELECT * FROM tv_post LIMIT 100) subq;

COMMIT;

-- ===================================================================
-- BENCHMARK 1: jsonb_smart_patch() - Smart Dispatcher
-- ===================================================================

\echo ''
\echo '===== BENCHMARK 1: jsonb_smart_patch ====='

-- Test 1.1: Scalar update
\echo 'Test 1.1: Scalar update (company name change)'
BEGIN;
EXPLAIN ANALYZE
UPDATE tv_company
SET data = jsonb_smart_patch_scalar(data, '{"name": "ACME Corporation"}'::jsonb)
WHERE pk = 1;
ROLLBACK;

-- Test 1.2: Nested object update
\echo 'Test 1.2: Nested object update (company in user)'
BEGIN;
EXPLAIN ANALYZE
UPDATE tv_user
SET data = jsonb_smart_patch_nested(
    data,
    (SELECT data FROM tv_company WHERE pk = 1),
    ARRAY['company']
)
WHERE fk_company = 1;
ROLLBACK;

-- Test 1.3: Array element update
\echo 'Test 1.3: Array element update (post in feed)'
BEGIN;
EXPLAIN ANALYZE
UPDATE tv_feed
SET data = jsonb_smart_patch_array(
    data,
    '{"title": "Updated Title"}'::jsonb,
    'posts',
    'id',
    to_jsonb((SELECT data->>'id' FROM tv_post WHERE pk = 1))
)
WHERE pk = 1;
ROLLBACK;

-- ===================================================================
-- BENCHMARK 2: Array CRUD Operations
-- ===================================================================

\echo ''
\echo '===== BENCHMARK 2: Array CRUD Operations ====='

-- Test 2.1: DELETE - Baseline (re-aggregation)
\echo 'Test 2.1a: DELETE via re-aggregation (BASELINE)'
BEGIN;
EXPLAIN ANALYZE
UPDATE tv_feed
SET data = jsonb_build_object(
    'posts',
    (
        SELECT jsonb_agg(data ORDER BY (data->>'created_at')::timestamptz DESC)
        FROM tv_post
        WHERE pk != 50
        LIMIT 100
    )
)
WHERE pk = 1;
ROLLBACK;

-- Test 2.1b: DELETE - Our implementation
\echo 'Test 2.1b: DELETE via jsonb_array_delete_where (OPTIMIZED)'
BEGIN;
EXPLAIN ANALYZE
UPDATE tv_feed
SET data = jsonb_array_delete_where(
    data,
    'posts',
    'id',
    to_jsonb((SELECT data->>'id' FROM tv_post WHERE pk = 50))
)
WHERE pk = 1;
ROLLBACK;

-- Test 2.2: INSERT - Baseline (re-aggregation)
\echo 'Test 2.2a: INSERT via re-aggregation (BASELINE)'
BEGIN;
INSERT INTO bench_post VALUES (1001, gen_random_uuid(), 1, 'New Post', 'Content', now());
INSERT INTO tv_post
SELECT
    1001,
    p.id,
    p.fk_user,
    u.id,
    jsonb_build_object('id', p.id, 'title', p.title, 'created_at', p.created_at, 'content', p.content)
FROM bench_post p
JOIN bench_user u ON u.pk = p.fk_user
WHERE p.pk = 1001;

EXPLAIN ANALYZE
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
ROLLBACK;

-- Test 2.2b: INSERT - Our implementation
\echo 'Test 2.2b: INSERT via jsonb_array_insert_where (OPTIMIZED)'
BEGIN;
INSERT INTO bench_post VALUES (1001, gen_random_uuid(), 1, 'New Post', 'Content', now());
INSERT INTO tv_post
SELECT
    1001,
    p.id,
    p.fk_user,
    u.id,
    jsonb_build_object('id', p.id, 'title', p.title, 'created_at', p.created_at, 'content', p.content)
FROM bench_post p
JOIN bench_user u ON u.pk = p.fk_user
WHERE p.pk = 1001;

EXPLAIN ANALYZE
UPDATE tv_feed
SET data = jsonb_array_insert_where(
    data,
    'posts',
    (SELECT data FROM tv_post WHERE pk = 1001),
    'created_at',
    'DESC'
)
WHERE pk = 1;
ROLLBACK;

-- ===================================================================
-- BENCHMARK 3: jsonb_deep_merge
-- ===================================================================

\echo ''
\echo '===== BENCHMARK 3: jsonb_deep_merge ====='

-- Test 3.1: Shallow vs Deep merge comparison
\echo 'Test 3.1a: Shallow merge (baseline)'
BEGIN;
SELECT
    data->'company'->>'name' AS before_name,
    data->'company'->>'industry' AS before_industry
FROM tv_user
WHERE pk = 1;

UPDATE tv_user
SET data = jsonb_merge_shallow(
    data,
    '{"company": {"name": "Updated Name", "headquarters": "NYC"}}'::jsonb
)
WHERE pk = 1;

SELECT
    data->'company'->>'name' AS after_name,
    data->'company'->>'industry' AS after_industry_LOST,
    data->'company'->>'headquarters' AS after_headquarters
FROM tv_user
WHERE pk = 1;
-- Note: This will REPLACE company object, losing 'industry' field
ROLLBACK;

\echo 'Test 3.1b: Deep merge (preserves nested fields)'
BEGIN;
SELECT
    data->'company'->>'name' AS before_name,
    data->'company'->>'industry' AS before_industry
FROM tv_user
WHERE pk = 1;

UPDATE tv_user
SET data = jsonb_deep_merge(
    data,
    '{"company": {"name": "Updated Name", "headquarters": "NYC"}}'::jsonb
)
WHERE pk = 1;

SELECT
    data->'company'->>'name' AS after_name,
    data->'company'->>'industry' AS after_industry_PRESERVED,
    data->'company'->>'headquarters' AS after_headquarters
FROM tv_user
WHERE pk = 1;
-- Note: This MERGES company fields, preserving 'industry' field
ROLLBACK;

-- ===================================================================
-- BENCHMARK 4: Helper Functions
-- ===================================================================

\echo ''
\echo '===== BENCHMARK 4: Helper Functions ====='

-- Test 4.1: jsonb_extract_id
\echo 'Test 4.1: jsonb_extract_id'
EXPLAIN ANALYZE
SELECT jsonb_extract_id(data) AS id
FROM tv_user
LIMIT 1000;

-- Test 4.2: jsonb_array_contains_id
\echo 'Test 4.2: jsonb_array_contains_id (find feeds containing specific post)'
EXPLAIN ANALYZE
SELECT pk
FROM tv_feed
WHERE jsonb_array_contains_id(
    data,
    'posts',
    'id',
    to_jsonb((SELECT data->>'id' FROM tv_post WHERE pk = 1))
);

-- ===================================================================
-- STRESS TEST: Full Cascade Simulation
-- ===================================================================

\echo ''
\echo '===== STRESS TEST: Full Cascade ====='

-- Simulate company name change cascading through hierarchy
\echo 'Full cascade: Company -> Users (50) -> Posts (500)'

BEGIN;

-- Step 1: Update company
UPDATE tv_company
SET data = jsonb_smart_patch_scalar(data, '{"name": "ACME Corporation LLC"}'::jsonb)
WHERE pk = 1;

-- Step 2: Cascade to users (nested object update)
UPDATE tv_user
SET data = jsonb_smart_patch_nested(
    data,
    (SELECT data FROM tv_company WHERE pk = 1),
    ARRAY['company']
)
WHERE fk_company = 1;

-- Step 3: Cascade to posts (deep merge - 2 levels deep)
UPDATE tv_post
SET data = jsonb_deep_merge(
    data,
    jsonb_build_object(
        'author',
        (SELECT data FROM tv_user WHERE pk = tv_post.fk_user)
    )
)
WHERE fk_user IN (SELECT pk FROM tv_user WHERE fk_company = 1);

COMMIT;

-- Verify cascade completed
\echo 'Verifying cascade results:'
SELECT
    'Company updated' AS step,
    data->>'name' AS value
FROM tv_company
WHERE pk = 1;

SELECT
    'User company updated' AS step,
    data->'company'->>'name' AS value
FROM tv_user
WHERE pk = 1;

SELECT
    'Post author company updated' AS step,
    data->'author'->'company'->>'name' AS value
FROM tv_post
WHERE fk_user = 1
LIMIT 1;

-- ===================================================================
-- BENCHMARK 5: Batch Operations
-- ===================================================================

\echo ''
\echo '===== BENCHMARK 5: Batch Operations ====='

-- Test 5.1: Batch array updates
\echo 'Test 5.1: Batch update multiple array elements'
BEGIN;
EXPLAIN ANALYZE
UPDATE tv_feed
SET data = jsonb_array_update_where_batch(
    data,
    'posts',
    'id',
    jsonb_build_array(
        jsonb_build_object('id', to_jsonb((SELECT data->>'id' FROM tv_post WHERE pk = 1)), 'title', 'Updated 1'),
        jsonb_build_object('id', to_jsonb((SELECT data->>'id' FROM tv_post WHERE pk = 2)), 'title', 'Updated 2'),
        jsonb_build_object('id', to_jsonb((SELECT data->>'id' FROM tv_post WHERE pk = 3)), 'title', 'Updated 3')
    )
)
WHERE pk = 1;
ROLLBACK;

-- Test 5.2: Multi-row array updates
\echo 'Test 5.2: Update array in multiple rows'
BEGIN;
CREATE TABLE multi_feed AS SELECT * FROM tv_feed;
INSERT INTO multi_feed SELECT 2, data FROM tv_feed WHERE pk = 1;
INSERT INTO multi_feed SELECT 3, data FROM tv_feed WHERE pk = 1;

EXPLAIN ANALYZE
SELECT * FROM jsonb_array_update_multi_row(
    ARRAY(SELECT data FROM multi_feed),
    'posts',
    'id',
    to_jsonb((SELECT data->>'id' FROM tv_post WHERE pk = 1)),
    '{"title": "Bulk Updated"}'::jsonb
);

DROP TABLE multi_feed;
ROLLBACK;

-- Cleanup
DROP TABLE bench_company, bench_user, bench_post, tv_company, tv_user, tv_post, tv_feed CASCADE;

\echo ''
\echo '===== BENCHMARK COMPLETE ====='
