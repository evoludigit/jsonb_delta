-- Load test for concurrent array operations
-- This script tests the performance and correctness of array update operations
-- under concurrent load with 100 clients

\set id random(1, 1000)
\set random_val random(100, 999)

-- Update first array element in test data
-- Since items have random UUIDs, we update based on array position
UPDATE test_jsonb
SET data = jsonb_set(
    data,
    '{items,0,value}',
    to_jsonb(:random_val)
)
WHERE id = :id;
