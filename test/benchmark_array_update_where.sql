-- Benchmark: jsonb_array_update_where vs native SQL equivalent
-- Goal: Demonstrate >3x improvement on 50-element arrays

\i test/fixtures/preamble.sql

\echo '========================================'
\echo 'BENCHMARK: jsonb_array_update_where'
\echo '========================================'
\echo ''

-- Ensure test data exists
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_tables WHERE tablename = 'test_tv_network_configuration') THEN
        RAISE EXCEPTION 'Benchmark fixtures not found. Run: psql -f test/fixtures/setup_benchmark_env.sql (or just bench)';
    END IF;
END $$;

-- ============================================================================
-- Benchmark 1: Single element update in a 50-element array
--
-- Driven by test/bench/harness.sql: warm-up, then N measured trials each in a
-- rolled-back subtransaction, reported as median and p95. The two arms are
-- checked for byte-identical output first, and no ratio is reported if they
-- disagree.
--
-- This previously ran a single `EXPLAIN ANALYZE` per arm under a header reading
-- "1000 iterations". One sample of a noisy variable is not a measurement, and
-- the numbers published from it should not be trusted (issue #15).
-- ============================================================================

\i test/bench/harness.sql

\echo '=== Benchmark 1: Update 1 element in 50-element array ==='
\echo ''

SELECT bench.define(
    name        => 'array_update_where_50',
    description => 'Update one element of a 50-element dns_servers array, stored table',
    setup_sql   => $$UPDATE test_tv_network_configuration t
                     SET data = s.data FROM tv_network_configuration s
                     WHERE s.id = t.id AND t.id = 1$$,
    native_sql  => $$UPDATE test_tv_network_configuration
                     SET data = jsonb_set(data, '{dns_servers}', (
                         SELECT jsonb_agg(CASE WHEN elem->>'id' = '42'
                                               THEN elem || '{"ip": "8.8.8.8"}'::jsonb
                                               ELSE elem END)
                         FROM jsonb_array_elements(data->'dns_servers') AS elem))
                     WHERE id = 1$$,
    delta_sql   => $$UPDATE test_tv_network_configuration
                     SET data = jsonb_array_update_where(
                         data, 'dns_servers', 'id', '42'::jsonb,
                         '{"ip": "8.8.8.8"}'::jsonb)
                     WHERE id = 1$$,
    verify_sql  => $$SELECT md5(data::text) FROM test_tv_network_configuration WHERE id = 1$$,
    n_trials    => 25,
    n_warmup    => 5
);

DO $run$ BEGIN PERFORM bench.run('array_update_where_50'); END $run$;

SELECT scenario, n, native_median_ms, native_p95_ms,
       delta_median_ms, delta_p95_ms, speedup, verdict
FROM bench.report
WHERE scenario = 'array_update_where_50';

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
\echo 'Benchmark 1 is harness-driven: median/p95 over 25 verified trials.'
\echo 'Benchmarks 2 and 3 are still single-shot wall-clock timings and are'
\echo 'indicative only -- do not publish numbers from them (issue #15).'
\echo ''
\echo 'Timings taken on a developer machine are not publishable regardless:'
\echo 'only ratios measured on the recorded machine profile are reportable.'
