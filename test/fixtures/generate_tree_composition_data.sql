-- Generates deep JSONB tree test data for non-array composition benchmarks
-- Focus: Updating nested objects (child data payload composition)
-- Example: User profile with nested address, preferences, billing, etc.

CREATE OR REPLACE FUNCTION generate_tree_composition_data()
RETURNS void AS $$
BEGIN
    -- Drop existing test tables
    DROP TABLE IF EXISTS bench_tree_users CASCADE;
    DROP TABLE IF EXISTS bench_tree_addresses CASCADE;
    DROP TABLE IF EXISTS bench_tree_billing CASCADE;
    DROP TABLE IF EXISTS bench_tree_preferences CASCADE;
    DROP MATERIALIZED VIEW IF EXISTS v_tree_user_profile CASCADE;

    -- Base table: Users (1000 records)
    CREATE TABLE bench_tree_users (
        id INTEGER PRIMARY KEY,
        username TEXT NOT NULL,
        email TEXT NOT NULL,
        created_at TIMESTAMPTZ NOT NULL DEFAULT now()
    );

    INSERT INTO bench_tree_users
    SELECT
        i,
        'user' || i,
        'user' || i || '@example.com',
        now() - (i || ' days')::interval
    FROM generate_series(1, 1000) i;

    -- Child table: Addresses (1:1 with users)
    CREATE TABLE bench_tree_addresses (
        user_id INTEGER PRIMARY KEY REFERENCES bench_tree_users(id),
        street TEXT NOT NULL,
        city TEXT NOT NULL,
        state TEXT NOT NULL,
        zip TEXT NOT NULL,
        country TEXT NOT NULL DEFAULT 'USA'
    );

    INSERT INTO bench_tree_addresses
    SELECT
        i,
        (i * 100) || ' Main St',
        CASE (i % 10)
            WHEN 0 THEN 'New York'
            WHEN 1 THEN 'Los Angeles'
            WHEN 2 THEN 'Chicago'
            WHEN 3 THEN 'Houston'
            WHEN 4 THEN 'Phoenix'
            WHEN 5 THEN 'Philadelphia'
            WHEN 6 THEN 'San Antonio'
            WHEN 7 THEN 'San Diego'
            WHEN 8 THEN 'Dallas'
            ELSE 'San Jose'
        END,
        CASE (i % 5)
            WHEN 0 THEN 'NY'
            WHEN 1 THEN 'CA'
            WHEN 2 THEN 'TX'
            WHEN 3 THEN 'IL'
            ELSE 'AZ'
        END,
        LPAD((i % 100000)::text, 5, '0'),
        'USA'
    FROM generate_series(1, 1000) i;

    -- Child table: Billing info (1:1 with users)
    CREATE TABLE bench_tree_billing (
        user_id INTEGER PRIMARY KEY REFERENCES bench_tree_users(id),
        card_last4 TEXT NOT NULL,
        card_type TEXT NOT NULL,
        billing_cycle TEXT NOT NULL,
        subscription_tier TEXT NOT NULL,
        monthly_cost NUMERIC(10,2) NOT NULL
    );

    INSERT INTO bench_tree_billing
    SELECT
        i,
        LPAD((i % 10000)::text, 4, '0'),
        CASE (i % 4)
            WHEN 0 THEN 'Visa'
            WHEN 1 THEN 'Mastercard'
            WHEN 2 THEN 'Amex'
            ELSE 'Discover'
        END,
        CASE (i % 3)
            WHEN 0 THEN 'monthly'
            WHEN 1 THEN 'quarterly'
            ELSE 'annual'
        END,
        CASE (i % 5)
            WHEN 0 THEN 'free'
            WHEN 1 THEN 'basic'
            WHEN 2 THEN 'pro'
            WHEN 3 THEN 'enterprise'
            ELSE 'premium'
        END,
        CASE (i % 5)
            WHEN 0 THEN 0.00
            WHEN 1 THEN 9.99
            WHEN 2 THEN 29.99
            WHEN 3 THEN 99.99
            ELSE 49.99
        END
    FROM generate_series(1, 1000) i;

    -- Child table: User preferences (1:1 with users)
    CREATE TABLE bench_tree_preferences (
        user_id INTEGER PRIMARY KEY REFERENCES bench_tree_users(id),
        theme TEXT NOT NULL DEFAULT 'light',
        language TEXT NOT NULL DEFAULT 'en',
        timezone TEXT NOT NULL DEFAULT 'UTC',
        notifications_enabled BOOLEAN NOT NULL DEFAULT true,
        email_frequency TEXT NOT NULL DEFAULT 'daily'
    );

    INSERT INTO bench_tree_preferences
    SELECT
        i,
        CASE (i % 3)
            WHEN 0 THEN 'light'
            WHEN 1 THEN 'dark'
            ELSE 'auto'
        END,
        CASE (i % 5)
            WHEN 0 THEN 'en'
            WHEN 1 THEN 'es'
            WHEN 2 THEN 'fr'
            WHEN 3 THEN 'de'
            ELSE 'ja'
        END,
        'America/New_York',
        (i % 2 = 0),
        CASE (i % 4)
            WHEN 0 THEN 'realtime'
            WHEN 1 THEN 'hourly'
            WHEN 2 THEN 'daily'
            ELSE 'weekly'
        END
    FROM generate_series(1, 1000) i;

    -- Table view: Deep JSONB composition (4-level tree)
    CREATE TABLE v_tree_user_profile (
        id INTEGER PRIMARY KEY,
        data JSONB NOT NULL
    );

    INSERT INTO v_tree_user_profile
    SELECT
        u.id,
        jsonb_build_object(
            'id', u.id,
            'username', u.username,
            'email', u.email,
            'created_at', u.created_at,
            'address', jsonb_build_object(
                'street', a.street,
                'city', a.city,
                'state', a.state,
                'zip', a.zip,
                'country', a.country
            ),
            'billing', jsonb_build_object(
                'card_last4', b.card_last4,
                'card_type', b.card_type,
                'billing_cycle', b.billing_cycle,
                'subscription', jsonb_build_object(
                    'tier', b.subscription_tier,
                    'monthly_cost', b.monthly_cost,
                    'active', true
                )
            ),
            'preferences', jsonb_build_object(
                'ui', jsonb_build_object(
                    'theme', p.theme,
                    'language', p.language
                ),
                'notifications', jsonb_build_object(
                    'enabled', p.notifications_enabled,
                    'email_frequency', p.email_frequency,
                    'timezone', p.timezone
                )
            )
        ) AS data
    FROM bench_tree_users u
    LEFT JOIN bench_tree_addresses a ON a.user_id = u.id
    LEFT JOIN bench_tree_billing b ON b.user_id = u.id
    LEFT JOIN bench_tree_preferences p ON p.user_id = u.id;

    RAISE NOTICE 'Tree composition test data generated successfully:';
    RAISE NOTICE '  - 1000 user profiles';
    RAISE NOTICE '  - 4-level deep JSONB trees';
    RAISE NOTICE '  - Paths: address.*, billing.subscription.*, preferences.ui.*, preferences.notifications.*';
END;
$$ LANGUAGE plpgsql;

-- Execute generator
SELECT generate_tree_composition_data();

-- Verify deep structure
\echo 'Sample user profile (deep JSONB tree):'
SELECT jsonb_pretty(data)
FROM v_tree_user_profile
WHERE id = 1
LIMIT 1;

\echo ''
\echo 'Nested paths to test:'
SELECT
    'address.city' AS path,
    data#>'{address,city}' AS value
FROM v_tree_user_profile WHERE id = 1
UNION ALL
SELECT
    'billing.subscription.tier',
    data#>'{billing,subscription,tier}'
FROM v_tree_user_profile WHERE id = 1
UNION ALL
SELECT
    'preferences.ui.theme',
    data#>'{preferences,ui,theme}'
FROM v_tree_user_profile WHERE id = 1
UNION ALL
SELECT
    'preferences.notifications.enabled',
    data#>'{preferences,notifications,enabled}'
FROM v_tree_user_profile WHERE id = 1;
