-- Generates UUID-based CQRS test data to compare performance
-- UUID array element IDs vs Integer array element IDs
-- Scale: 100 network configurations × 50 DNS servers each (UUID-based)

CREATE OR REPLACE FUNCTION generate_uuid_test_data()
RETURNS void AS $$
BEGIN
    -- Drop existing test tables
    DROP TABLE IF EXISTS bench_uuid_dns_servers CASCADE;
    DROP TABLE IF EXISTS bench_uuid_network_configs CASCADE;
    DROP TABLE IF EXISTS bench_uuid_allocations CASCADE;
    DROP TABLE IF EXISTS bench_uuid_nc_dns_mapping CASCADE;

    -- Base table: DNS servers with UUIDs (500 records)
    CREATE TABLE bench_uuid_dns_servers (
        id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
        int_id INTEGER UNIQUE NOT NULL,  -- for deterministic lookups
        ip TEXT NOT NULL,
        port INTEGER NOT NULL,
        status TEXT NOT NULL
    );

    INSERT INTO bench_uuid_dns_servers (int_id, ip, port, status)
    SELECT
        i,
        '8.8.' || (i % 256) || '.' || ((i / 256) % 256),
        53 + (i % 10),
        CASE WHEN i % 5 = 0 THEN 'inactive' ELSE 'active' END
    FROM generate_series(1, 500) i;

    -- Table view: v_uuid_dns_server (leaf view)
    CREATE TABLE v_uuid_dns_server (
        id UUID PRIMARY KEY,
        int_id INTEGER UNIQUE NOT NULL,
        data JSONB NOT NULL
    );

    INSERT INTO v_uuid_dns_server
    SELECT
        id,
        int_id,
        jsonb_build_object(
            'id', id::text,  -- UUID as string in JSONB
            'ip', ip,
            'port', port,
            'status', status
        ) AS data
    FROM bench_uuid_dns_servers;

    -- Base table: Network configurations (100 records)
    CREATE TABLE bench_uuid_network_configs (
        id INTEGER PRIMARY KEY,  -- Keep integer PK for simplicity
        name TEXT NOT NULL,
        gateway_ip TEXT NOT NULL,
        subnet_mask TEXT NOT NULL,
        created_at TIMESTAMPTZ NOT NULL DEFAULT now()
    );

    INSERT INTO bench_uuid_network_configs
    SELECT
        i,
        'Network Config ' || i,
        '192.168.' || (i % 256) || '.1',
        '255.255.255.0',
        now() - (i || ' days')::interval
    FROM generate_series(1, 100) i;

    -- Mapping table: Network Config ↔ UUID DNS Servers (50 DNS servers per config)
    CREATE TABLE bench_uuid_nc_dns_mapping (
        network_configuration_id INTEGER REFERENCES bench_uuid_network_configs(id),
        dns_server_id UUID REFERENCES bench_uuid_dns_servers(id),
        dns_server_int_id INTEGER NOT NULL,  -- for lookups
        priority INTEGER NOT NULL,
        PRIMARY KEY (network_configuration_id, dns_server_id)
    );

    INSERT INTO bench_uuid_nc_dns_mapping
    SELECT
        ((i - 1) / 50) + 1 AS network_configuration_id,
        (SELECT id FROM bench_uuid_dns_servers WHERE int_id = ((i - 1) % 50) + 1) AS dns_server_id,
        ((i - 1) % 50) + 1 AS dns_server_int_id,
        (i % 50) + 1 AS priority
    FROM generate_series(1, 5000) i;

    -- Table view: tv_uuid_network_configuration (projection with UUID array elements)
    CREATE TABLE tv_uuid_network_configuration (
        id INTEGER PRIMARY KEY,
        data JSONB NOT NULL
    );

    INSERT INTO tv_uuid_network_configuration
    SELECT
        nc.id,
        jsonb_build_object(
            'id', nc.id,
            'name', nc.name,
            'gateway_ip', nc.gateway_ip,
            'subnet_mask', nc.subnet_mask,
            'dns_servers', COALESCE((
                SELECT jsonb_agg(v.data ORDER BY m.priority)
                FROM bench_uuid_nc_dns_mapping m
                JOIN v_uuid_dns_server v ON v.id = m.dns_server_id
                WHERE m.network_configuration_id = nc.id
            ), '[]'::jsonb),
            'created_at', nc.created_at
        ) AS data
    FROM bench_uuid_network_configs nc;

    RAISE NOTICE 'UUID test data generated successfully:';
    RAISE NOTICE '  - 500 DNS servers (UUID IDs)';
    RAISE NOTICE '  - 100 network configurations (50 UUID DNS servers each)';
    RAISE NOTICE '  - Array elements use UUID strings as IDs';
END;
$$ LANGUAGE plpgsql;

-- Execute generator
SELECT generate_uuid_test_data();

-- Verify data structure
\echo 'Sample UUID-based network configuration:'
SELECT jsonb_pretty(data)
FROM tv_uuid_network_configuration
WHERE id = 1
LIMIT 1;

\echo ''
\echo 'Verify UUID format in array elements:'
SELECT
    data->'dns_servers'->0->>'id' AS first_dns_id,
    length(data->'dns_servers'->0->>'id') AS uuid_string_length,
    jsonb_array_length(data->'dns_servers') AS total_dns_servers
FROM tv_uuid_network_configuration
WHERE id = 1;
