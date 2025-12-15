-- End-to-end benchmark: Complete CQRS cascade with custom operations
-- Compare: Native SQL vs Custom Rust cascade

\timing on

\echo '========================================'
\echo 'END-TO-END CASCADE BENCHMARK'
\echo '========================================'
\echo ''
\echo 'Scenario: Update DNS server #42, propagate through all levels'
\echo '  - Affected: 1 DNS server → 2 network configs → 10 allocations'
\echo ''

-- ============================================================================
-- NATIVE SQL CASCADE (Baseline)
-- ============================================================================

\echo '=== NATIVE SQL CASCADE ==='
\echo ''

BEGIN;

-- Step 1: Update leaf (test_v_dns_server)
\echo '--- Step 1: Update test_v_dns_server ---'
UPDATE test_v_dns_server
SET data = jsonb_set(data, '{ip}', '"9.9.9.9"')
WHERE id = 42;

-- Step 2: Propagate to test_tv_network_configuration (re-aggregate entire array)
\echo '--- Step 2: Propagate to test_tv_network_configuration (FULL RE-AGGREGATE) ---'
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
    JOIN test_tv_network_configuration nc ON nc.id = ac.id
)
UPDATE test_tv_network_configuration
SET data = jsonb_set(data, '{dns_servers}', updated_dns_servers)
FROM updated_configs uc
WHERE test_tv_network_configuration.id = uc.id;

-- Step 3: Propagate to test_tv_allocation (replace entire network_configuration)
\echo '--- Step 3: Propagate to test_tv_allocation (REPLACE OBJECT) ---'
UPDATE test_tv_allocation
SET data = jsonb_set(
    data,
    '{network_configuration}',
    (SELECT nc.data FROM test_tv_network_configuration nc WHERE nc.id = (test_tv_allocation.data->'network_configuration'->>'id')::int)
)
WHERE (data->'network_configuration'->>'id')::int IN (
    SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42
);

ROLLBACK;

\echo ''

-- ============================================================================
-- CUSTOM RUST CASCADE (Optimized)
-- ============================================================================

\echo '=== CUSTOM RUST CASCADE ==='
\echo ''

BEGIN;

-- Step 1: Update leaf (test_v_dns_server)
\echo '--- Step 1: Update test_v_dns_server ---'
UPDATE test_v_dns_server
SET data = jsonb_set(data, '{ip}', '"9.9.9.9"')
WHERE id = 42;

-- Step 2: Propagate to test_tv_network_configuration (SURGICAL array update)
\echo '--- Step 2: Propagate to test_tv_network_configuration (SURGICAL UPDATE) ---'
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

-- Step 3: Propagate to test_tv_allocation (SURGICAL nested merge)
\echo '--- Step 3: Propagate to test_tv_allocation (SURGICAL MERGE) ---'
UPDATE test_tv_allocation a
SET data = jsonb_merge_at_path(
    data,
    nc.data,
    ARRAY['network_configuration']
)
FROM test_tv_network_configuration nc
WHERE nc.id = (a.data->'network_configuration'->>'id')::int
  AND nc.id IN (
    SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42
  );

ROLLBACK;

\echo ''
\echo '========================================'
\echo 'Expected Results'
\echo '========================================'
\echo ''
\echo 'Native SQL:'
\echo '  - Step 2: 50-100ms (re-aggregate 50-element array × 2 configs)'
\echo '  - Step 3: 100-200ms (replace 50KB object × 10 allocations)'
\echo '  - Total: 150-300ms'
\echo ''
\echo 'Custom Rust:'
\echo '  - Step 2: 10-20ms (surgical update 1 element × 2 configs)'
\echo '  - Step 3: 20-40ms (surgical merge × 10 allocations)'
\echo '  - Total: 30-60ms'
\echo ''
\echo 'Target: 5x improvement (300ms → 60ms)'
\echo ''
