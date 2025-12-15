-- Test Suite: jsonb_merge_at_path()
-- Expected: All tests pass

CREATE EXTENSION IF NOT EXISTS jsonb_delta;

-- Test 1: Root level merge
SELECT jsonb_merge_at_path(
    '{"a": 1, "b": 2}'::jsonb,
    '{"b": 99, "c": 3}'::jsonb,
    ARRAY[]::text[]
) = '{"a": 1, "b": 99, "c": 3}'::jsonb AS test_root_merge;

-- Test 2: Nested merge
SELECT jsonb_merge_at_path(
    '{"id": 1, "network_configuration": {"id": 17, "name": "old"}}'::jsonb,
    '{"name": "updated"}'::jsonb,
    ARRAY['network_configuration']
) = '{"id": 1, "network_configuration": {"id": 17, "name": "updated"}}'::jsonb AS test_nested_merge;

-- Test 3: Use in allocation update
WITH updated_nc AS (
    SELECT jsonb_build_object('name', 'Updated Network', 'dns_count', 50) AS new_data
)
SELECT jsonb_merge_at_path(
    '{"id": 100, "name": "Allocation 1", "network_configuration": {"id": 17, "name": "old"}}'::jsonb,
    new_data,
    ARRAY['network_configuration']
)->'network_configuration'->>'name' = 'Updated Network' AS test_allocation_update
FROM updated_nc;

\echo 'All tests should return TRUE'
