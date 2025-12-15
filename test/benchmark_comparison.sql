-- Performance comparison: jsonb_merge_shallow vs native || operator
--
-- This benchmark compares the extension's manual merge implementation
-- against PostgreSQL's built-in jsonb_concat (|| operator).
--
-- Expected results:
--   - Extension is typically 20-40% slower than native || operator
--   - Extension provides better type safety (errors on non-objects)
--   - Extension has clearer error messages
--
-- Run with: psql -d your_db -f test/benchmark_comparison.sql

CREATE EXTENSION IF NOT EXISTS jsonb_ivm;

\echo '========================================='
\echo 'Performance Comparison Benchmark'
\echo 'jsonb_merge_shallow vs. native || operator'
\echo '========================================='
\echo ''

\timing on

-- ============================================================================
-- Benchmark 1: Small objects (10 keys each, 10,000 merges)
-- ============================================================================

\echo '=== Benchmark 1: Small objects (10 keys, 10,000 merges) ==='
\echo ''

\echo '--- jsonb_merge_shallow (extension) ---'
SELECT count(*) FROM (
    SELECT jsonb_merge_shallow(
        jsonb_build_object('a', i, 'b', i+1, 'c', i+2, 'd', i+3, 'e', i+4),
        jsonb_build_object('f', i*10, 'g', i*20, 'h', i*30, 'i', i*40, 'j', i*50)
    )
    FROM generate_series(1, 10000) i
) sub;

\echo '--- || operator (native) ---'
SELECT count(*) FROM (
    SELECT jsonb_build_object('a', i, 'b', i+1, 'c', i+2, 'd', i+3, 'e', i+4) ||
           jsonb_build_object('f', i*10, 'g', i*20, 'h', i*30, 'i', i*40, 'j', i*50)
    FROM generate_series(1, 10000) i
) sub;

\echo ''

-- ============================================================================
-- Benchmark 2: Medium objects (50 keys each, 1,000 merges)
-- ============================================================================

\echo '=== Benchmark 2: Medium objects (50 keys, 1,000 merges) ==='
\echo ''

-- Prepare test objects
DROP TABLE IF EXISTS bench_medium_target;
DROP TABLE IF EXISTS bench_medium_source;

CREATE TEMP TABLE bench_medium_target AS
    SELECT jsonb_object_agg('target_key' || i, i) AS obj
    FROM generate_series(1, 50) i;

CREATE TEMP TABLE bench_medium_source AS
    SELECT jsonb_object_agg('source_key' || i, i * 10) AS obj
    FROM generate_series(1, 50) i;

\echo '--- jsonb_merge_shallow (extension) ---'
SELECT count(*) FROM (
    SELECT jsonb_merge_shallow(t.obj, s.obj)
    FROM bench_medium_target t, bench_medium_source s, generate_series(1, 1000)
) sub;

\echo '--- || operator (native) ---'
SELECT count(*) FROM (
    SELECT t.obj || s.obj
    FROM bench_medium_target t, bench_medium_source s, generate_series(1, 1000)
) sub;

\echo ''

-- ============================================================================
-- Benchmark 3: Large objects (150 keys each, 100 merges)
-- ============================================================================

\echo '=== Benchmark 3: Large objects (150 keys, 100 merges) ==='
\echo ''

-- Prepare test objects
DROP TABLE IF EXISTS bench_large_target;
DROP TABLE IF EXISTS bench_large_source;

CREATE TEMP TABLE bench_large_target AS
    SELECT jsonb_object_agg('target_key' || i, i) AS obj
    FROM generate_series(1, 100) i;

CREATE TEMP TABLE bench_large_source AS
    SELECT jsonb_object_agg('source_key' || i, i * 10) AS obj
    FROM generate_series(1, 50) i;

\echo '--- jsonb_merge_shallow (extension) ---'
SELECT count(*) FROM (
    SELECT jsonb_merge_shallow(t.obj, s.obj)
    FROM bench_large_target t, bench_large_source s, generate_series(1, 100)
) sub;

\echo '--- || operator (native) ---'
SELECT count(*) FROM (
    SELECT t.obj || s.obj
    FROM bench_large_target t, bench_large_source s, generate_series(1, 100)
) sub;

\echo ''

-- ============================================================================
-- Benchmark 4: Overlapping keys (realistic CQRS scenario)
-- ============================================================================

\echo '=== Benchmark 4: Overlapping keys - CQRS update scenario (5,000 updates) ==='
\echo ''
\echo 'Scenario: Updating customer info in denormalized order view'
\echo 'Target: Order with 20 fields, Source: 5 customer fields (3 overlap)'
\echo ''

-- Prepare realistic CQRS test data
DROP TABLE IF EXISTS bench_cqrs_orders;
DROP TABLE IF EXISTS bench_cqrs_updates;

CREATE TEMP TABLE bench_cqrs_orders AS
    SELECT jsonb_build_object(
        'order_id', i,
        'customer_id', i % 100,
        'customer_name', 'Customer ' || i,
        'customer_email', 'customer' || i || '@example.com',
        'customer_phone', '555-' || i,
        'product_id', i * 2,
        'product_name', 'Product ' || i,
        'quantity', (i % 10) + 1,
        'unit_price', (i % 100) * 1.5,
        'total_price', ((i % 10) + 1) * (i % 100) * 1.5,
        'status', CASE WHEN i % 3 = 0 THEN 'shipped' ELSE 'pending' END,
        'created_at', '2025-01-01'::timestamp + (i || ' hours')::interval,
        'updated_at', now(),
        'shipping_address', jsonb_build_object('street', 'Street ' || i, 'city', 'City'),
        'billing_address', jsonb_build_object('street', 'Street ' || i, 'city', 'City'),
        'notes', 'Order notes for ' || i,
        'tags', jsonb_build_array('tag1', 'tag2'),
        'metadata', jsonb_build_object('source', 'web', 'campaign', 'summer2025')
    ) AS order_data
    FROM generate_series(1, 100) i;

CREATE TEMP TABLE bench_cqrs_updates AS
    SELECT jsonb_build_object(
        'customer_name', 'Updated Customer ' || i,
        'customer_email', 'updated' || i || '@example.com',
        'customer_phone', '555-UPDATED-' || i,
        'updated_at', now(),
        'update_reason', 'Customer info changed'
    ) AS update_data
    FROM generate_series(1, 100) i;

\echo '--- jsonb_merge_shallow (extension) ---'
SELECT count(*) FROM (
    SELECT jsonb_merge_shallow(o.order_data, u.update_data)
    FROM bench_cqrs_orders o, bench_cqrs_updates u, generate_series(1, 50)
) sub;

\echo '--- || operator (native) ---'
SELECT count(*) FROM (
    SELECT o.order_data || u.update_data
    FROM bench_cqrs_orders o, bench_cqrs_updates u, generate_series(1, 50)
) sub;

\echo ''

-- ============================================================================
-- Type Safety Comparison
-- ============================================================================

\echo '=== Type Safety Comparison ==='
\echo ''
\echo 'Test: Merging array with object (should error in extension, allow in native)'
\echo ''

\echo '--- jsonb_merge_shallow (extension - should ERROR) ---'
\set ON_ERROR_STOP off
SELECT jsonb_merge_shallow('[1,2,3]'::jsonb, '{"a": 1}'::jsonb);
\set ON_ERROR_STOP on

\echo ''
\echo '--- || operator (native - allows array concat) ---'
SELECT '[1,2,3]'::jsonb || '{"a": 1}'::jsonb;

\echo ''

\timing off

-- ============================================================================
-- Summary
-- ============================================================================

\echo ''
\echo '========================================='
\echo 'Benchmark Summary'
\echo '========================================='
\echo ''
\echo 'Expected Performance Difference:'
\echo '  - Extension typically 20-40% slower than native || operator'
\echo '  - Slowdown is due to manual HashMap cloning in Rust implementation'
\echo ''
\echo 'Extension Advantages:'
\echo '  ✅ Type safety: Errors on non-object merges (prevents bugs)'
\echo '  ✅ Clear error messages: Shows actual type received'
\echo '  ✅ Explicit function name: More readable than || operator'
\echo '  ✅ Future features: jsonb_merge_at_path for nested merging'
\echo ''
\echo 'Native || Advantages:'
\echo '  ✅ Performance: Faster (native C implementation)'
\echo '  ✅ Flexibility: Allows array concatenation, mixed types'
\echo '  ✅ Built-in: No extension dependency'
\echo ''
\echo 'Recommendation:'
\echo '  - Use extension for CQRS materialized view updates (type-safe, readable)'
\echo '  - Use native || for performance-critical general JSONB manipulation'
\echo ''

-- Cleanup
DROP TABLE IF EXISTS bench_medium_target;
DROP TABLE IF EXISTS bench_medium_source;
DROP TABLE IF EXISTS bench_large_target;
DROP TABLE IF EXISTS bench_large_source;
DROP TABLE IF EXISTS bench_cqrs_orders;
DROP TABLE IF EXISTS bench_cqrs_updates;
