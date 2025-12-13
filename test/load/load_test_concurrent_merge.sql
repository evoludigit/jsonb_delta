-- Load test for concurrent merge operations
-- This script tests the performance and correctness of jsonb_ivm_deep_merge
-- under concurrent load with 100 clients

\set id random(1, 1000)
\set random_val random(100, 999)

UPDATE test_jsonb
SET data = jsonb_deep_merge(
    data,
    jsonb_build_object('updated_at', now()::text, 'random_field', :random_val)
)
WHERE id = :id;
