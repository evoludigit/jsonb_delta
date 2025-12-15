-- Generates realistic nested JSONB documents in regular tables (table views)
-- CQRS architecture: v_ tables (leaf views) and tv_ tables (projection tables)
-- Scale: 100 network configurations × 50 DNS servers each

DROP TABLE IF EXISTS tv_allocation CASCADE;
DROP TABLE IF EXISTS tv_network_configuration CASCADE;
DROP TABLE IF EXISTS v_dns_server CASCADE;
DROP TABLE IF EXISTS bench_nc_dns_mapping CASCADE;
DROP TABLE IF EXISTS bench_allocations CASCADE;
DROP TABLE IF EXISTS bench_network_configs CASCADE;
DROP TABLE IF EXISTS bench_dns_servers CASCADE;

-- ============================================================================
-- Base tables (normalized source data)
-- ============================================================================

-- Base table: DNS servers (500 records)
CREATE TABLE bench_dns_servers (
    id INTEGER PRIMARY KEY,
    ip TEXT NOT NULL,
    port INTEGER NOT NULL,
    status TEXT NOT NULL
);

INSERT INTO bench_dns_servers
SELECT
    i,
    '8.8.' || (i % 256) || '.' || ((i / 256) % 256),
    53 + (i % 10),
    CASE WHEN i % 5 = 0 THEN 'inactive' ELSE 'active' END
FROM generate_series(1, 500) i;

-- Base table: Network configurations (100 records)
CREATE TABLE bench_network_configs (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    gateway_ip TEXT NOT NULL,
    subnet_mask TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO bench_network_configs
SELECT
    i,
    'Network Config ' || i,
    '192.168.' || (i % 256) || '.1',
    '255.255.255.0',
    now() - (i || ' days')::interval
FROM generate_series(1, 100) i;

-- Mapping table: Network Config ↔ DNS Servers (50 DNS servers per config)
CREATE TABLE bench_nc_dns_mapping (
    network_configuration_id INTEGER REFERENCES bench_network_configs(id),
    dns_server_id INTEGER REFERENCES bench_dns_servers(id),
    priority INTEGER NOT NULL,
    PRIMARY KEY (network_configuration_id, dns_server_id)
);

INSERT INTO bench_nc_dns_mapping
SELECT
    ((i - 1) / 50) + 1 AS network_configuration_id,
    ((i - 1) % 50) + 1 AS dns_server_id,
    (i % 50) + 1 AS priority
FROM generate_series(1, 5000) i;

-- Base table: Allocations (500 records, 5 per network config)
CREATE TABLE bench_allocations (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    network_configuration_id INTEGER REFERENCES bench_network_configs(id),
    machine_name TEXT NOT NULL,
    storage_size_gb INTEGER NOT NULL
);

INSERT INTO bench_allocations
SELECT
    i,
    'Allocation ' || i,
    ((i - 1) / 5) + 1 AS network_configuration_id,
    'Machine-' || i,
    (i % 10 + 1) * 100
FROM generate_series(1, 500) i;

-- ============================================================================
-- Projection tables (denormalized JSONB - CQRS read models)
-- ============================================================================

-- v_dns_server: Leaf projection table (view of DNS servers)
CREATE TABLE v_dns_server (
    id INTEGER PRIMARY KEY,
    data JSONB NOT NULL
);

INSERT INTO v_dns_server
SELECT
    id,
    jsonb_build_object(
        'id', id,
        'ip', ip,
        'port', port,
        'status', status
    )
FROM bench_dns_servers;

CREATE INDEX idx_v_dns_server_id ON v_dns_server(id);

-- tv_network_configuration: Intermediate projection (embeds dns_servers array)
CREATE TABLE tv_network_configuration (
    id INTEGER PRIMARY KEY,
    data JSONB NOT NULL
);

INSERT INTO tv_network_configuration
SELECT
    nc.id,
    jsonb_build_object(
        'id', nc.id,
        'name', nc.name,
        'gateway_ip', nc.gateway_ip,
        'subnet_mask', nc.subnet_mask,
        'dns_servers', COALESCE((
            SELECT jsonb_agg(v.data ORDER BY m.priority)
            FROM bench_nc_dns_mapping m
            JOIN v_dns_server v ON v.id = m.dns_server_id
            WHERE m.network_configuration_id = nc.id
        ), '[]'::jsonb),
        'created_at', nc.created_at
    )
FROM bench_network_configs nc;

CREATE INDEX idx_tv_network_configuration_id ON tv_network_configuration(id);

-- tv_allocation: Top-level projection (embeds full network_configuration)
CREATE TABLE tv_allocation (
    id INTEGER PRIMARY KEY,
    data JSONB NOT NULL
);

INSERT INTO tv_allocation
SELECT
    a.id,
    jsonb_build_object(
        'id', a.id,
        'name', a.name,
        'network_configuration', nc.data,  -- ← Nested 10KB+ document with 50 DNS servers
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
LEFT JOIN tv_network_configuration nc ON nc.id = a.network_configuration_id;

CREATE INDEX idx_tv_allocation_id ON tv_allocation(id);
CREATE INDEX idx_tv_allocation_nc_id ON tv_allocation((data->'network_configuration'->>'id'));

-- Add test_ aliases for backward compatibility
CREATE OR REPLACE VIEW test_v_dns_server AS SELECT * FROM v_dns_server;
CREATE OR REPLACE VIEW test_tv_network_configuration AS SELECT * FROM tv_network_configuration;
CREATE OR REPLACE VIEW test_tv_allocation AS SELECT * FROM tv_allocation;

-- Verify data sizes
SELECT
    'v_dns_server' AS table_name,
    COUNT(*) AS rows,
    pg_size_pretty(pg_total_relation_size('v_dns_server')) AS size
FROM v_dns_server
UNION ALL
SELECT
    'tv_network_configuration',
    COUNT(*),
    pg_size_pretty(pg_total_relation_size('tv_network_configuration'))
FROM tv_network_configuration
UNION ALL
SELECT
    'tv_allocation',
    COUNT(*),
    pg_size_pretty(pg_total_relation_size('tv_allocation'))
FROM tv_allocation;

SELECT
    '✓ Test data generated successfully' AS status,
    '500 DNS servers, 100 network configs (50 DNS each), 500 allocations' AS summary;
