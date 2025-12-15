-- Benchmark: UUID vs Integer ID matching performance
-- Compare jsonb_array_update_where with UUID string IDs vs Integer IDs

\timing on
\set ON_ERROR_STOP on

CREATE EXTENSION IF NOT EXISTS jsonb_ivm;

\echo '========================================'
\echo 'BENCHMARK: UUID vs Integer ID Performance'
\echo '========================================'
\echo ''

-- Ensure test data exists
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_tables WHERE tablename = 'tv_uuid_network_configuration') THEN
        RAISE EXCEPTION 'UUID test data not found. Run generate_uuid_test_data.sql first.';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pg_tables WHERE tablename = 'tv_network_configuration') THEN
        RAISE EXCEPTION 'Integer test data not found. Run generate_cqrs_data.sql first.';
    END IF;
END $$;

-- Get a sample UUID for testing
\set TEST_UUID `psql -d postgres -tAc "SELECT data->'dns_servers'->0->>'id' FROM tv_uuid_network_configuration WHERE id = 1"`

\echo 'Test UUID:' :TEST_UUID
\echo ''

-- ============================================================================
-- Benchmark 1: Single UUID-based array update (50 elements)
-- ============================================================================

\echo '=== Benchmark 1: Update 1 UUID element in 50-element array ==='
\echo ''

-- UUID approach with jsonb_array_update_where
\echo '--- Rust Extension (UUID string matching) ---'
BEGIN;
EXPLAIN ANALYZE
UPDATE tv_uuid_network_configuration
SET data = jsonb_array_update_where(
    data,
    'dns_servers',
    'id',
    format('"%s"', (data->'dns_servers'->0->>'id'))::jsonb,  -- UUID as string
    '{"status": "maintenance"}'::jsonb
)
WHERE id = 1;
ROLLBACK;

\echo ''

-- Integer approach with jsonb_array_update_where (baseline)
\echo '--- Rust Extension (Integer matching - baseline) ---'
BEGIN;
EXPLAIN ANALYZE
UPDATE tv_network_configuration
SET data = jsonb_array_update_where(
    data,
    'dns_servers',
    'id',
    '42'::jsonb,  -- Integer
    '{"status": "maintenance"}'::jsonb
)
WHERE id = 1;
ROLLBACK;

-- ============================================================================
-- Benchmark 2: UUID vs Integer - 100 row cascade
-- ============================================================================

\echo ''
\echo '=== Benchmark 2: Cascade - Update 100 network configs ==='
\echo ''

-- UUID approach
\echo '--- UUID-based cascade (100 rows) ---'
BEGIN;
\timing on
UPDATE tv_uuid_network_configuration
SET data = jsonb_array_update_where(
    data,
    'dns_servers',
    'id',
    format('"%s"', (
        SELECT id::text
        FROM bench_uuid_dns_servers
        WHERE int_id = 42
    ))::jsonb,
    '{"status": "updated"}'::jsonb
)
WHERE id <= 100;
ROLLBACK;

\echo ''

-- Integer approach
\echo '--- Integer-based cascade (100 rows) ---'
BEGIN;
\timing on
UPDATE tv_network_configuration
SET data = jsonb_array_update_where(
    data,
    'dns_servers',
    'id',
    '42'::jsonb,
    '{"status": "updated"}'::jsonb
)
WHERE id <= 100;
ROLLBACK;

-- ============================================================================
-- Benchmark 3: String matching performance at different array sizes
-- ============================================================================

\echo ''
\echo '=== Benchmark 3: String vs Integer comparison overhead ==='
\echo ''

-- Create test arrays with different sizes
DO $$
DECLARE
    test_uuid TEXT;
    test_array_10 JSONB;
    test_array_50 JSONB;
    test_array_100 JSONB;
    start_time TIMESTAMPTZ;
    end_time TIMESTAMPTZ;
    duration_ms NUMERIC;
BEGIN
    -- Get a test UUID
    SELECT data->'dns_servers'->0->>'id' INTO test_uuid
    FROM tv_uuid_network_configuration
    WHERE id = 1;

    -- Build test arrays
    SELECT jsonb_agg(
        jsonb_build_object(
            'id', gen_random_uuid()::text,
            'value', i
        )
    ) INTO test_array_10
    FROM generate_series(1, 10) i;

    SELECT jsonb_agg(
        jsonb_build_object(
            'id', gen_random_uuid()::text,
            'value', i
        )
    ) INTO test_array_50
    FROM generate_series(1, 50) i;

    SELECT jsonb_agg(
        jsonb_build_object(
            'id', gen_random_uuid()::text,
            'value', i
        )
    ) INTO test_array_100
    FROM generate_series(1, 100) i;

    -- Insert target UUID at position 25 for 50-element array
    test_array_50 = jsonb_set(
        test_array_50,
        '{24,id}',
        to_jsonb(test_uuid)
    );

    RAISE NOTICE '10-element UUID array:';
    start_time := clock_timestamp();
    FOR i IN 1..1000 LOOP
        PERFORM jsonb_array_update_where(
            jsonb_build_object('items', test_array_10),
            'items',
            'id',
            format('"%s"', test_uuid)::jsonb,
            '{"updated": true}'::jsonb
        );
    END LOOP;
    end_time := clock_timestamp();
    duration_ms := EXTRACT(EPOCH FROM (end_time - start_time)) * 1000;
    RAISE NOTICE 'Time for 1000 iterations: % ms (avg: % ms)',
        ROUND(duration_ms, 2),
        ROUND(duration_ms / 1000, 3);

    RAISE NOTICE '';
    RAISE NOTICE '50-element UUID array:';
    start_time := clock_timestamp();
    FOR i IN 1..1000 LOOP
        PERFORM jsonb_array_update_where(
            jsonb_build_object('items', test_array_50),
            'items',
            'id',
            format('"%s"', test_uuid)::jsonb,
            '{"updated": true}'::jsonb
        );
    END LOOP;
    end_time := clock_timestamp();
    duration_ms := EXTRACT(EPOCH FROM (end_time - start_time)) * 1000;
    RAISE NOTICE 'Time for 1000 iterations: % ms (avg: % ms)',
        ROUND(duration_ms, 2),
        ROUND(duration_ms / 1000, 3);

    RAISE NOTICE '';
    RAISE NOTICE '100-element UUID array:';
    start_time := clock_timestamp();
    FOR i IN 1..100 LOOP
        PERFORM jsonb_array_update_where(
            jsonb_build_object('items', test_array_100),
            'items',
            'id',
            format('"%s"', test_uuid)::jsonb,
            '{"updated": true}'::jsonb
        );
    END LOOP;
    end_time := clock_timestamp();
    duration_ms := EXTRACT(EPOCH FROM (end_time - start_time)) * 1000;
    RAISE NOTICE 'Time for 100 iterations: % ms (avg: % ms)',
        ROUND(duration_ms, 2),
        ROUND(duration_ms / 1000, 3);
END $$;

\echo ''
\echo '========================================'
\echo 'SUMMARY: UUID vs Integer Performance'
\echo '========================================'
\echo ''
\echo 'Expected findings:'
\echo '  - Integer matching: Optimized with 8-way loop unrolling'
\echo '  - UUID string matching: No loop unrolling optimization (uses find_by_jsonb_value)'
\echo '  - Performance delta will show optimization impact'
\echo ''
