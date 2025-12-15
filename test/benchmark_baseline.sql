-- Baseline: Native PostgreSQL approach (what we're trying to beat)
-- Scenario: Update single DNS server IP address, measure cascade time

\timing on
\set ON_ERROR_STOP on

\echo '========================================'
\echo 'BASELINE: Native PostgreSQL Cascade'
\echo '========================================'
\echo ''

-- Create updatable tables for testing (simulate what materialized views would be)
DROP TABLE IF EXISTS test_v_dns_server CASCADE;
DROP TABLE IF EXISTS test_tv_network_configuration CASCADE;
DROP TABLE IF EXISTS test_tv_allocation CASCADE;

CREATE TABLE test_v_dns_server AS SELECT * FROM v_dns_server;
CREATE TABLE test_tv_network_configuration AS SELECT * FROM tv_network_configuration;
CREATE TABLE test_tv_allocation AS SELECT * FROM tv_allocation;

CREATE INDEX idx_test_v_dns_server_id ON test_v_dns_server(id);
CREATE INDEX idx_test_tv_network_configuration_id ON test_tv_network_configuration(id);
CREATE INDEX idx_test_tv_allocation_id ON test_tv_allocation(id);
CREATE INDEX idx_test_tv_allocation_nc_id ON test_tv_allocation((data->'network_configuration'->>'id'));

-- Scenario: Update DNS server #42, propagate to all dependent views
\echo 'Scenario: UPDATE bench_dns_servers SET ip = ''9.9.9.9'' WHERE id = 42'
\echo ''

-- Step 1: Update leaf view (test_v_dns_server)
\echo '--- Step 1: Update test_v_dns_server ---'
BEGIN;

\echo 'Baseline approach: Full JSONB rebuild'
EXPLAIN ANALYZE
UPDATE test_v_dns_server
SET data = (
    SELECT jsonb_build_object(
        'id', id,
        'ip', ip,
        'port', port,
        'status', status
    )
    FROM bench_dns_servers
    WHERE bench_dns_servers.id = test_v_dns_server.id
)
WHERE id = 42;

ROLLBACK;

-- Step 2: Propagate to test_tv_network_configuration
\echo ''
\echo '--- Step 2: Propagate to test_tv_network_configuration ---'
BEGIN;

-- First, update the leaf
UPDATE test_v_dns_server
SET data = jsonb_set(data, '{ip}', '"9.9.9.9"')
WHERE id = 42;

-- Then propagate (WORST CASE: full rebuild)
\echo 'Baseline approach: Re-aggregate entire dns_servers array (50 elements)'
EXPLAIN ANALYZE
UPDATE test_tv_network_configuration
SET data = (
    SELECT jsonb_build_object(
        'id', nc.id,
        'name', nc.name,
        'gateway_ip', nc.gateway_ip,
        'subnet_mask', nc.subnet_mask,
        'dns_servers', (
            SELECT jsonb_agg(v.data ORDER BY m.priority)
            FROM bench_nc_dns_mapping m
            JOIN test_v_dns_server v ON v.id = m.dns_server_id
            WHERE m.network_configuration_id = nc.id
        ),
        'created_at', nc.created_at
    )
    FROM bench_network_configs nc
    WHERE nc.id = test_tv_network_configuration.id
)
WHERE id IN (
    SELECT network_configuration_id
    FROM bench_nc_dns_mapping
    WHERE dns_server_id = 42
);

ROLLBACK;

-- Step 3: Propagate to test_tv_allocation
\echo ''
\echo '--- Step 3: Propagate to test_tv_allocation ---'
BEGIN;

-- Update leaf and intermediate
UPDATE test_v_dns_server SET data = jsonb_set(data, '{ip}', '"9.9.9.9"') WHERE id = 42;

UPDATE test_tv_network_configuration
SET data = jsonb_set(
    data,
    '{dns_servers}',
    (
        SELECT jsonb_agg(v.data ORDER BY m.priority)
        FROM bench_nc_dns_mapping m
        JOIN test_v_dns_server v ON v.id = m.dns_server_id
        WHERE m.network_configuration_id = test_tv_network_configuration.id
    )
)
WHERE id IN (
    SELECT network_configuration_id
    FROM bench_nc_dns_mapping
    WHERE dns_server_id = 42
);

-- Propagate to allocation (WORST CASE: full rebuild)
\echo 'Baseline approach: Full rebuild of 50KB allocation document'
EXPLAIN ANALYZE
UPDATE test_tv_allocation
SET data = (
    SELECT jsonb_build_object(
        'id', a.id,
        'name', a.name,
        'network_configuration', nc.data,
        'machine', jsonb_build_object(
            'name', a.machine_name,
            'status', 'active'
        ),
        'storage', jsonb_build_object(
            'size_gb', a.storage_size_gb,
            'type', 'SSD'
        )
    )
    FROM bench_allocations a
    LEFT JOIN test_tv_network_configuration nc ON nc.id = a.network_configuration_id
    WHERE a.id = test_tv_allocation.id
)
WHERE (data->'network_configuration'->>'id')::int IN (
    SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42
);

ROLLBACK;

-- Step 4: END-TO-END timing (full cascade)
\echo ''
\echo '--- Step 4: END-TO-END cascade timing ---'
\echo 'Full cascade: Leaf → Intermediate → Top'
\echo ''

BEGIN;

-- Measure total time for complete propagation
\timing on
UPDATE test_v_dns_server SET data = jsonb_set(data, '{ip}', '"9.9.9.9"') WHERE id = 42;

UPDATE test_tv_network_configuration
SET data = jsonb_set(
    data,
    '{dns_servers}',
    (
        SELECT jsonb_agg(v.data ORDER BY m.priority)
        FROM bench_nc_dns_mapping m
        JOIN test_v_dns_server v ON v.id = m.dns_server_id
        WHERE m.network_configuration_id = test_tv_network_configuration.id
    )
)
WHERE id IN (SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42);

UPDATE test_tv_allocation
SET data = jsonb_set(
    data,
    '{network_configuration}',
    (SELECT nc.data FROM test_tv_network_configuration nc WHERE nc.id = (test_tv_allocation.data->'network_configuration'->>'id')::int)
)
WHERE (data->'network_configuration'->>'id')::int IN (
    SELECT network_configuration_id FROM bench_nc_dns_mapping WHERE dns_server_id = 42
);

COMMIT;

\echo ''
\echo '========================================'
\echo 'Baseline Complete'
\echo '========================================'
\echo ''
\echo 'Key metrics:'
\echo '  - Leaf update time: <measure from Step 1>'
\echo '  - Intermediate propagation: <measure from Step 2>'
\echo '  - Top-level propagation: <measure from Step 3>'
\echo '  - Total end-to-end time: <measure from Step 4>'
\echo ''
\echo 'This is the baseline to beat with custom Rust functions.'
