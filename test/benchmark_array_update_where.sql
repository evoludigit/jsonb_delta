-- Benchmark: jsonb_array_update_where vs native SQL equivalent
-- Goal: Demonstrate >3x improvement on 50-element arrays

\timing on
\set ON_ERROR_STOP on

CREATE EXTENSION IF NOT EXISTS jsonb_ivm;

\echo '========================================'
\echo 'BENCHMARK: jsonb_array_update_where'
\echo '========================================'
\echo ''

-- Ensure test data exists
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_tables WHERE tablename = 'test_tv_network_configuration') THEN
        RAISE EXCEPTION 'Test data not found. Run generate_cqrs_data.sql first.';
    END IF;
END $$;

-- ============================================================================
-- Benchmark 1: Single element update in 50-element array
-- ============================================================================

\echo '=== Benchmark 1: Update 1 element in 50-element array (1000 iterations) ==='
\echo ''

-- NATIVE APPROACH (baseline)
\echo '--- Native SQL (re-aggregate array with CASE) ---'
BEGIN;
EXPLAIN ANALYZE
WITH updated AS (
    SELECT
        id,
        (
            SELECT jsonb_agg(
                CASE
                    WHEN elem->>'id' = '42'
                    THEN elem || '{"ip": "8.8.8.8"}'::jsonb
                    ELSE elem
                END
            )
            FROM jsonb_array_elements(data->'dns_servers') AS elem
        ) AS updated_array
            FROM test_tv_network_configuration
            WHERE id = 1
)
UPDATE test_tv_network_configuration
SET data = jsonb_set(data, '{dns_servers}', updated_array)
FROM updated
WHERE test_tv_network_configuration.id = updated.id;
ROLLBACK;

\echo ''
\echo '--- Custom Rust function ---'
BEGIN;
EXPLAIN ANALYZE
UPDATE test_tv_network_configuration
SET data = jsonb_array_update_where(
    data,
    'dns_servers',
    'id',
    '42'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb
)
WHERE id = 1;
ROLLBACK;

-- ============================================================================
-- Benchmark 2: Update propagation in CQRS cascade
-- ============================================================================

\echo ''
\echo '=== Benchmark 2: CQRS Cascade - Update DNS Server #42 ==='
\echo 'Propagate through: v_dns_server → tv_network_configuration → tv_allocation'
\echo ''

-- NATIVE APPROACH
\echo '--- Native SQL Cascade ---'
BEGIN;

UPDATE test_v_dns_server
SET data = jsonb_set(data, '{ip}', '"9.9.9.9"')
WHERE id = 42;

-- Propagate to tv_network_configuration (re-aggregate full array)
WITH affected_configs AS (
    SELECT DISTINCT network_configuration_id AS id
    FROM bench_nc_dns_mapping
    WHERE dns_server_id = 42
),
updated_configs AS (
    SELECT
        nc.id,
        (
            SELECT jsonb_agg(v.data ORDER BY m.priority)
            FROM bench_nc_dns_mapping m
            JOIN test_v_dns_server v ON v.id = m.dns_server_id
            WHERE m.network_configuration_id = nc.id
        ) AS updated_dns_servers
    FROM affected_configs ac
    JOIN tv_network_configuration nc ON nc.id = ac.id
)
UPDATE test_tv_network_configuration
SET data = jsonb_set(data, '{dns_servers}', updated_dns_servers)
FROM updated_configs uc
WHERE test_tv_network_configuration.id = uc.id;

-- Propagate to tv_allocation (replace network_configuration object)
UPDATE tv_allocation
SET data = jsonb_set(
    data,
    '{network_configuration}',
    (SELECT nc.data FROM tv_network_configuration nc WHERE nc.id = (tv_allocation.data->'network_configuration'->>'id')::int)
)
WHERE (data->'network_configuration'->>'id')::int IN (
    SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42
);

ROLLBACK;

\echo ''
\echo '--- Custom Rust Cascade ---'
BEGIN;

UPDATE test_v_dns_server
SET data = jsonb_set(data, '{ip}', '"9.9.9.9"')
WHERE id = 42;

-- Propagate to test_tv_network_configuration (surgical array update)
UPDATE test_tv_network_configuration
SET data = jsonb_array_update_where(
    data,
    'dns_servers',
    'id',
    '42'::jsonb,
    (SELECT data FROM test_v_dns_server WHERE id = 42)
)
WHERE id IN (
    SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42
);

-- Propagate to tv_allocation (replace network_configuration object)
UPDATE tv_allocation
SET data = jsonb_set(
    data,
    '{network_configuration}',
    (SELECT nc.data FROM tv_network_configuration nc WHERE nc.id = (tv_allocation.data->'network_configuration'->>'id')::int)
)
WHERE (data->'network_configuration'->>'id')::int IN (
    SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42
);

ROLLBACK;

-- ============================================================================
-- Benchmark 3: Stress test - Update 100 different DNS servers
-- ============================================================================

\echo ''
\echo '=== Benchmark 3: Stress Test - Update 100 DNS servers sequentially ==='
\echo ''

\echo '--- Native SQL (100 cascades) ---'
\timing on
DO $$
DECLARE
    dns_id INTEGER;
BEGIN
    FOR dns_id IN 1..100 LOOP
        -- Update leaf
        UPDATE test_v_dns_server
        SET data = jsonb_set(data, '{ip}', to_jsonb('10.0.0.' || dns_id))
        WHERE id = dns_id;

        -- Propagate to configs (native re-aggregate)
        WITH affected_configs AS (
            SELECT DISTINCT network_configuration_id AS id
            FROM bench_nc_dns_mapping
            WHERE dns_server_id = dns_id
        ),
        updated_configs AS (
            SELECT
                nc.id,
                (
                    SELECT jsonb_agg(v.data ORDER BY m.priority)
                    FROM bench_nc_dns_mapping m
                    JOIN test_v_dns_server v ON v.id = m.dns_server_id
                    WHERE m.network_configuration_id = nc.id
                ) AS updated_dns_servers
            FROM affected_configs ac
            JOIN test_tv_network_configuration nc ON nc.id = ac.id
        )
        UPDATE test_tv_network_configuration
        SET data = jsonb_set(data, '{dns_servers}', updated_dns_servers)
        FROM updated_configs uc
        WHERE test_tv_network_configuration.id = uc.id;
    END LOOP;
END $$;
\timing off

\echo ''
\echo '--- Custom Rust (100 cascades) ---'
\timing on
DO $$
DECLARE
    dns_id INTEGER;
BEGIN
    FOR dns_id IN 1..100 LOOP
        -- Update leaf
        UPDATE test_v_dns_server
        SET data = jsonb_set(data, '{ip}', to_jsonb('10.0.0.' || dns_id))
        WHERE id = dns_id;

        -- Propagate to configs (surgical array update)
        UPDATE test_tv_network_configuration
        SET data = jsonb_array_update_where(
            data,
            'dns_servers',
            'id',
            to_jsonb(dns_id),
            (SELECT data FROM test_v_dns_server WHERE id = dns_id)
        )
        WHERE id IN (
            SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = dns_id
        );
    END LOOP;
END $$;
\timing off

\echo ''
\echo '========================================'
\echo 'Benchmark Complete'
\echo '========================================'
\echo ''
\echo 'Expected Results:'
\echo '  - Benchmark 1 (single update): Rust 2-3x faster'
\echo '  - Benchmark 2 (cascade): Rust 3-5x faster'
\echo '  - Benchmark 3 (stress): Rust 5-10x faster'
\echo ''
\echo 'If Rust is <1.5x faster, reconsider approach.'
