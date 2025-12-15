-- Simple performance benchmarks for jsonb_merge_shallow()

CREATE EXTENSION IF NOT EXISTS jsonb_ivm;

\timing on

-- Benchmark 1: Small objects (10 keys) - 10,000 merges
\echo '\n=== Benchmark 1: Small objects (10 keys) - 10,000 merges ==='
SELECT count(*) FROM (
    SELECT jsonb_merge_shallow(
        jsonb_build_object('a', i, 'b', i+1, 'c', i+2, 'd', i+3, 'e', i+4),
        jsonb_build_object('f', i*10, 'g', i*20, 'h', i*30, 'i', i*40, 'j', i*50)
    )
    FROM generate_series(1, 10000) i
) sub;

-- Benchmark 2: Medium objects (50 keys) - 1,000 merges
\echo '\n=== Benchmark 2: Medium objects (50 keys) - 1,000 merges ==='
WITH target AS (
    SELECT jsonb_object_agg('key' || i, i) AS obj
    FROM generate_series(1, 50) i
),
source AS (
    SELECT jsonb_object_agg('key' || i, i * 10) AS obj
    FROM generate_series(26, 75) i
)
SELECT count(*) FROM (
    SELECT jsonb_merge_shallow(target.obj, source.obj)
    FROM target, source, generate_series(1, 1000)
) sub;

-- Benchmark 3: Large objects (150 keys) - 100 merges
\echo '\n=== Benchmark 3: Large objects (150 keys) - 100 merges ==='
WITH target AS (
    SELECT jsonb_object_agg('key' || i, i) AS obj
    FROM generate_series(1, 100) i
),
source AS (
    SELECT jsonb_object_agg('key' || i, i * 10) AS obj
    FROM generate_series(51, 150) i
)
SELECT count(*) FROM (
    SELECT jsonb_merge_shallow(target.obj, source.obj)
    FROM target, source, generate_series(1, 100)
) sub;

\timing off

\echo '\n=== Benchmark complete ==='
\echo 'Expected performance thresholds:'
\echo '  Small (10 keys, 10k merges):  < 100ms'
\echo '  Medium (50 keys, 1k merges):  < 500ms'
\echo '  Large (150 keys, 100 merges): < 200ms'
