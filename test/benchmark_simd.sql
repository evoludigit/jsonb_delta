\timing on

-- Test SIMD performance on large arrays
CREATE EXTENSION IF NOT EXISTS jsonb_ivm;

\echo '========================================'
\echo 'BENCHMARK: SIMD Array Scanning'
\echo '========================================'

-- Generate test data: 1000-element arrays
DO $$
DECLARE
    large_array jsonb;
BEGIN
    SELECT jsonb_build_object(
        'items',
        jsonb_agg(
            jsonb_build_object(
                'id', i,
                'data', 'value_' || i::text,
                'timestamp', now()
            )
        )
    ) INTO large_array
    FROM generate_series(1, 1000) i;

    CREATE TEMP TABLE test_large_arrays (
        id int PRIMARY KEY,
        data jsonb
    );

    INSERT INTO test_large_arrays
    SELECT i, large_array
    FROM generate_series(1, 100) i;
END $$;

\echo '=== Benchmark 1: Find element at position 900 (near end) ==='

-- Scalar search (current POC)
\timing on
BEGIN;
EXPLAIN ANALYZE
UPDATE test_large_arrays
SET data = jsonb_array_update_where(
    data,
    'items',
    'id',
    '900'::jsonb,
    '{"status": "updated"}'::jsonb
)
WHERE id = 1;
ROLLBACK;

-- Expected: ~0.5-1ms for 1000-element scan

\echo '=== Benchmark 2: Batch update 10 elements ==='

BEGIN;
EXPLAIN ANALYZE
SELECT jsonb_array_update_where_batch(
    data,
    'items',
    'id',
    '[
        {"match_value": 100, "updates": {"status": "ok"}},
        {"match_value": 200, "updates": {"status": "ok"}},
        {"match_value": 300, "updates": {"status": "ok"}},
        {"match_value": 400, "updates": {"status": "ok"}},
        {"match_value": 500, "updates": {"status": "ok"}},
        {"match_value": 600, "updates": {"status": "ok"}},
        {"match_value": 700, "updates": {"status": "ok"}},
        {"match_value": 800, "updates": {"status": "ok"}},
        {"match_value": 900, "updates": {"status": "ok"}},
        {"match_value": 1000, "updates": {"status": "ok"}}
    ]'::jsonb
)
FROM test_large_arrays
WHERE id = 1;
ROLLBACK;

-- Expected: 1 batch call ~2× faster than 10 separate calls

\echo '=== Benchmark 3: Multi-row batch (100 rows) ==='

BEGIN;
EXPLAIN ANALYZE
SELECT * FROM jsonb_array_update_multi_row(
    (SELECT array_agg(data ORDER BY id) FROM test_large_arrays),
    'items',
    'id',
    '42'::jsonb,
    '{"status": "batch_updated"}'::jsonb
);
ROLLBACK;

-- Expected: ~10-20ms for 100 docs (vs 50-100ms with 100 separate calls)

\echo '========================================'
\echo 'Benchmark Complete'
\echo '========================================'
