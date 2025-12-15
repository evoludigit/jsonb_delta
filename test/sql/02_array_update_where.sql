-- Test Suite: jsonb_array_update_where()
-- Expected: All tests pass

CREATE EXTENSION IF NOT EXISTS jsonb_ivm;

-- Test 1: Basic array update
SELECT jsonb_array_update_where(
    '{"dns_servers": [{"id": 1, "ip": "1.1.1.1"}, {"id": 2, "ip": "2.2.2.2"}]}'::jsonb,
    'dns_servers',
    'id',
    '1'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb
) = '{"dns_servers": [{"id": 1, "ip": "8.8.8.8"}, {"id": 2, "ip": "2.2.2.2"}]}'::jsonb
AS test_basic_update;

-- Test 2: No match (returns unchanged)
SELECT jsonb_array_update_where(
    '{"dns_servers": [{"id": 1, "ip": "1.1.1.1"}]}'::jsonb,
    'dns_servers',
    'id',
    '999'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb
) = '{"dns_servers": [{"id": 1, "ip": "1.1.1.1"}]}'::jsonb
AS test_no_match;

-- Test 3: Single level path (nested paths use jsonb_set)
SELECT jsonb_array_update_where(
    '{"dns_servers": [{"id": 1, "ip": "1.1.1.1"}]}'::jsonb,
    'dns_servers',
    'id',
    '1'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb
)->'dns_servers'->0->>'ip' = '8.8.8.8'
AS test_single_path;

-- Test 4: Large array (100 elements)
WITH large_array AS (
    SELECT jsonb_build_object(
        'dns_servers',
        jsonb_agg(jsonb_build_object('id', i, 'ip', '192.168.1.' || i))
    ) AS data
    FROM generate_series(1, 100) i
)
SELECT (
    jsonb_array_update_where(
        data,
        'dns_servers',
        'id',
        '99'::jsonb,
        '{"ip": "8.8.8.8", "status": "updated"}'::jsonb
    )->'dns_servers'->98->>'ip'
) = '8.8.8.8'
AS test_large_array
FROM large_array;

-- Test 5: Preserve existing fields
SELECT jsonb_array_update_where(
    '{"dns_servers": [{"id": 1, "ip": "1.1.1.1", "port": 53, "status": "active"}]}'::jsonb,
    'dns_servers',
    'id',
    '1'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb
)->'dns_servers'->0 = '{"id": 1, "ip": "8.8.8.8", "port": 53, "status": "active"}'::jsonb
AS test_preserve_fields;

-- Test 6: NULL handling
SELECT jsonb_array_update_where(
    NULL::jsonb,
    'dns_servers',
    'id',
    '1'::jsonb,
    '{"ip": "8.8.8.8"}'::jsonb
) IS NULL
AS test_null_handling;

\echo 'All tests should return TRUE'
