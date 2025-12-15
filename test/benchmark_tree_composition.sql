-- Benchmark: Deep JSONB tree updates (non-array composition)
-- Compare jsonb_merge_at_path vs native jsonb_set for nested object updates

\timing on
\set ON_ERROR_STOP on

CREATE EXTENSION IF NOT EXISTS jsonb_ivm;

\echo '========================================'
\echo 'BENCHMARK: Deep JSONB Tree Composition'
\echo '========================================'
\echo ''

-- Ensure test data exists
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_tables WHERE tablename = 'v_tree_user_profile') THEN
        RAISE EXCEPTION 'Tree composition test data not found. Run generate_tree_composition_data.sql first.';
    END IF;
END $$;

\echo 'Sample profile structure:'
SELECT jsonb_pretty(data) FROM v_tree_user_profile WHERE id = 1 LIMIT 1;

\echo ''
\echo '========================================'
\echo ''

-- ============================================================================
-- Benchmark 1: Single-level nested update (address.city)
-- ============================================================================

\echo '=== Benchmark 1: Update nested field (address.city) ==='
\echo ''

-- Native jsonb_set approach
\echo '--- Native jsonb_set (3 operations: extract, update, merge) ---'
BEGIN;
EXPLAIN ANALYZE
UPDATE v_tree_user_profile
SET data = jsonb_set(
    data,
    '{address,city}',
    '"San Francisco"'::jsonb
)
WHERE id = 42;
ROLLBACK;

\echo ''

-- jsonb_merge_at_path approach
\echo '--- jsonb_merge_at_path (single operation) ---'
BEGIN;
EXPLAIN ANALYZE
UPDATE v_tree_user_profile
SET data = jsonb_merge_at_path(
    data,
    '{"city": "San Francisco"}'::jsonb,
    ARRAY['address']
)
WHERE id = 42;
ROLLBACK;

-- ============================================================================
-- Benchmark 2: Deep nested update (billing.subscription.tier)
-- ============================================================================

\echo ''
\echo '=== Benchmark 2: Update deep nested field (billing.subscription.tier) ==='
\echo ''

-- Native jsonb_set approach
\echo '--- Native jsonb_set (deep path) ---'
BEGIN;
EXPLAIN ANALYZE
UPDATE v_tree_user_profile
SET data = jsonb_set(
    data,
    '{billing,subscription,tier}',
    '"enterprise"'::jsonb
)
WHERE id = 42;
ROLLBACK;

\echo ''

-- jsonb_merge_at_path approach
\echo '--- jsonb_merge_at_path (deep path) ---'
BEGIN;
EXPLAIN ANALYZE
UPDATE v_tree_user_profile
SET data = jsonb_merge_at_path(
    data,
    '{"tier": "enterprise"}'::jsonb,
    ARRAY['billing', 'subscription']
)
WHERE id = 42;
ROLLBACK;

-- ============================================================================
-- Benchmark 3: Multi-field update at same nesting level
-- ============================================================================

\echo ''
\echo '=== Benchmark 3: Update multiple fields in nested object (preferences.ui.*) ==='
\echo ''

-- Native approach (multiple jsonb_set calls nested)
\echo '--- Native jsonb_set (nested operations) ---'
BEGIN;
EXPLAIN ANALYZE
UPDATE v_tree_user_profile
SET data = jsonb_set(
    jsonb_set(
        data,
        '{preferences,ui,theme}',
        '"dark"'::jsonb
    ),
    '{preferences,ui,language}',
    '"fr"'::jsonb
)
WHERE id = 42;
ROLLBACK;

\echo ''

-- jsonb_merge_at_path approach (single merge)
\echo '--- jsonb_merge_at_path (single merge) ---'
BEGIN;
EXPLAIN ANALYZE
UPDATE v_tree_user_profile
SET data = jsonb_merge_at_path(
    data,
    '{"theme": "dark", "language": "fr"}'::jsonb,
    ARRAY['preferences', 'ui']
)
WHERE id = 42;
ROLLBACK;

-- ============================================================================
-- Benchmark 4: Composition pattern - Update child data payload
-- ============================================================================

\echo ''
\echo '=== Benchmark 4: CQRS Composition - Propagate child table changes ==='
\echo 'Scenario: Update billing info in 100 user profiles when billing data changes'
\echo ''

-- Native approach: Full object replacement
\echo '--- Native SQL: Full billing object replacement ---'
BEGIN;
\timing on
UPDATE v_tree_user_profile
SET data = jsonb_set(
    data,
    '{billing}',
    (
        SELECT jsonb_build_object(
            'card_last4', b.card_last4,
            'card_type', b.card_type,
            'billing_cycle', b.billing_cycle,
            'subscription', jsonb_build_object(
                'tier', b.subscription_tier,
                'monthly_cost', b.monthly_cost,
                'active', true
            )
        )
        FROM bench_tree_billing b
        WHERE b.user_id = v_tree_user_profile.id
    )
)
WHERE id <= 100;
ROLLBACK;

\echo ''

-- jsonb_merge_at_path approach: Partial update
\echo '--- jsonb_merge_at_path: Partial billing update (only changed fields) ---'
BEGIN;
\timing on
UPDATE v_tree_user_profile
SET data = jsonb_merge_at_path(
    data,
    (
        SELECT jsonb_build_object(
            'subscription', jsonb_build_object(
                'tier', b.subscription_tier,
                'monthly_cost', b.monthly_cost
            )
        )
        FROM bench_tree_billing b
        WHERE b.user_id = v_tree_user_profile.id
    ),
    ARRAY['billing']
)
WHERE id <= 100;
ROLLBACK;

-- ============================================================================
-- Benchmark 5: Cascade propagation (3-level tree)
-- ============================================================================

\echo ''
\echo '=== Benchmark 5: Multi-level cascade (Address → Profile → Aggregated Report) ==='
\echo ''

-- Create aggregated report view (top-level projection)
DROP MATERIALIZED VIEW IF EXISTS v_tree_user_report CASCADE;
CREATE MATERIALIZED VIEW v_tree_user_report AS
SELECT
    id,
    jsonb_build_object(
        'user_id', id,
        'report_date', now(),
        'profile', (SELECT data FROM v_tree_user_profile WHERE v_tree_user_profile.id = u.id),
        'summary', jsonb_build_object(
            'status', 'active',
            'risk_score', (id % 100)
        )
    ) AS data
FROM bench_tree_users u
LIMIT 100;

CREATE INDEX idx_v_tree_user_report_id ON v_tree_user_report(id);

\echo 'Cascade scenario: Update address in source table → propagate through 2 levels'
\echo ''

-- Full cascade with native SQL
\echo '--- Native SQL cascade (full object replacement at each level) ---'
BEGIN;

-- Level 1: Update source table
UPDATE bench_tree_addresses
SET city = 'Seattle', state = 'WA'
WHERE user_id = 42;

-- Level 2: Propagate to v_tree_user_profile (full address rebuild)
UPDATE v_tree_user_profile
SET data = jsonb_set(
    data,
    '{address}',
    (
        SELECT jsonb_build_object(
            'street', a.street,
            'city', a.city,
            'state', a.state,
            'zip', a.zip,
            'country', a.country
        )
        FROM bench_tree_addresses a
        WHERE a.user_id = v_tree_user_profile.id
    )
)
WHERE id = 42;

-- Level 3: Propagate to v_tree_user_report (full profile rebuild)
UPDATE v_tree_user_report
SET data = jsonb_set(
    data,
    '{profile}',
    (SELECT data FROM v_tree_user_profile WHERE v_tree_user_profile.id = 42)
)
WHERE id = 42;

ROLLBACK;

\echo ''

-- Cascade with jsonb_merge_at_path (partial updates)
\echo '--- jsonb_merge_at_path cascade (partial updates at each level) ---'
BEGIN;

-- Level 1: Update source table
UPDATE bench_tree_addresses
SET city = 'Seattle', state = 'WA'
WHERE user_id = 42;

-- Level 2: Propagate to v_tree_user_profile (partial address merge)
UPDATE v_tree_user_profile
SET data = jsonb_merge_at_path(
    data,
    (
        SELECT jsonb_build_object(
            'city', a.city,
            'state', a.state
        )
        FROM bench_tree_addresses a
        WHERE a.user_id = v_tree_user_profile.id
    ),
    ARRAY['address']
)
WHERE id = 42;

-- Level 3: Propagate to v_tree_user_report (partial profile merge)
UPDATE v_tree_user_report
SET data = jsonb_merge_at_path(
    data,
    (SELECT jsonb_build_object(
        'address', data->'address'
    ) FROM v_tree_user_profile WHERE v_tree_user_profile.id = 42),
    ARRAY['profile']
)
WHERE id = 42;

ROLLBACK;

-- ============================================================================
-- Benchmark 6: Batch composition (100 profiles)
-- ============================================================================

\echo ''
\echo '=== Benchmark 6: Batch update - Propagate billing changes to 100 profiles ==='
\echo 'Scenario: Subscription tier update affects 100 users'
\echo ''

-- Update source data
UPDATE bench_tree_billing
SET subscription_tier = 'premium', monthly_cost = 49.99
WHERE user_id <= 100;

-- Native approach
\echo '--- Native SQL: Full billing replacement (100 rows) ---'
BEGIN;
\timing on
UPDATE v_tree_user_profile
SET data = jsonb_set(
    data,
    '{billing}',
    (
        SELECT jsonb_build_object(
            'card_last4', b.card_last4,
            'card_type', b.card_type,
            'billing_cycle', b.billing_cycle,
            'subscription', jsonb_build_object(
                'tier', b.subscription_tier,
                'monthly_cost', b.monthly_cost,
                'active', true
            )
        )
        FROM bench_tree_billing b
        WHERE b.user_id = v_tree_user_profile.id
    )
)
WHERE id <= 100;
ROLLBACK;

\echo ''

-- jsonb_merge_at_path approach
\echo '--- jsonb_merge_at_path: Partial subscription update (100 rows) ---'
BEGIN;
\timing on
UPDATE v_tree_user_profile
SET data = jsonb_merge_at_path(
    data,
    (
        SELECT jsonb_build_object(
            'subscription', jsonb_build_object(
                'tier', b.subscription_tier,
                'monthly_cost', b.monthly_cost
            )
        )
        FROM bench_tree_billing b
        WHERE b.user_id = v_tree_user_profile.id
    ),
    ARRAY['billing']
)
WHERE id <= 100;
ROLLBACK;

-- Reset billing data
UPDATE bench_tree_billing
SET subscription_tier = CASE (user_id % 5)
        WHEN 0 THEN 'free'
        WHEN 1 THEN 'basic'
        WHEN 2 THEN 'pro'
        WHEN 3 THEN 'enterprise'
        ELSE 'premium'
    END,
    monthly_cost = CASE (user_id % 5)
        WHEN 0 THEN 0.00
        WHEN 1 THEN 9.99
        WHEN 2 THEN 29.99
        WHEN 3 THEN 99.99
        ELSE 49.99
    END
WHERE user_id <= 100;

\echo ''
\echo '========================================'
\echo 'SUMMARY: Tree Composition Performance'
\echo '========================================'
\echo ''
\echo 'Expected findings:'
\echo '  1. Single field update: jsonb_merge_at_path ~1-1.5× (similar performance)'
\echo '  2. Multi-field update: jsonb_merge_at_path 1.5-2× faster (single pass)'
\echo '  3. Deep path update: Both similar (path traversal dominates)'
\echo '  4. Partial vs full replacement: jsonb_merge_at_path 1.5-2× faster'
\echo '  5. Cascade propagation: jsonb_merge_at_path reduces serialization overhead'
\echo ''
\echo 'Key insight: jsonb_merge_at_path shines when:'
\echo '  - Updating multiple fields at same level'
\echo '  - Partial updates (not full object replacement)'
\echo '  - CQRS cascade with incremental changes'
\echo ''
