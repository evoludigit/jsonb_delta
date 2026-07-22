-- Upgrade-path test: install the extension at 0.1.0 and walk it up the
-- released chain 0.1.0 -> 0.2.0 -> 0.3.0 -> 0.3.1.
-- Requires sql/jsonb_delta--0.1.0.sql plus every upgrade script
-- (--0.1.0--0.2.0.sql, --0.2.0--0.3.0.sql, --0.3.0--0.3.1.sql) to be present in the server's
-- SHAREDIR/extension directory (see justfile `test-upgrade` and the CI
-- "Test extension upgrade path" step).
-- Run with: psql -v ON_ERROR_STOP=1 -f test/upgrade_path_test.sql

\set ON_ERROR_STOP on

DROP EXTENSION IF EXISTS jsonb_delta;

-- A real 0.1.0 install, not the default version.
CREATE EXTENSION jsonb_delta VERSION '0.1.0';

DO $$
DECLARE
    v text;
BEGIN
    SELECT extversion INTO v FROM pg_extension WHERE extname = 'jsonb_delta';
    IF v <> '0.1.0' THEN
        RAISE EXCEPTION 'expected a 0.1.0 install to start from, got %', v;
    END IF;
END $$;

-- Hop 1: 0.1.0 -> 0.2.0 (adds jsonb_apply_changeset).
ALTER EXTENSION jsonb_delta UPDATE TO '0.2.0';

DO $$
DECLARE
    v text;
    n int;
BEGIN
    SELECT extversion INTO v FROM pg_extension WHERE extname = 'jsonb_delta';
    IF v <> '0.2.0' THEN
        RAISE EXCEPTION 'ALTER EXTENSION UPDATE left extversion at %, expected 0.2.0', v;
    END IF;

    SELECT count(*) INTO n
    FROM pg_depend d
    JOIN pg_proc p ON p.oid = d.objid
    JOIN pg_extension e ON e.oid = d.refobjid
    WHERE d.deptype = 'e' AND e.extname = 'jsonb_delta';
    IF n <> 16 THEN
        RAISE EXCEPTION 'expected 16 functions after 0.2.0 upgrade, found %', n;
    END IF;
END $$;

-- Hop 2: 0.2.0 -> 0.3.0 (binary rewrite; same contract, re-points every function).
ALTER EXTENSION jsonb_delta UPDATE TO '0.3.0';

DO $$
DECLARE
    v text;
    n int;
BEGIN
    SELECT extversion INTO v FROM pg_extension WHERE extname = 'jsonb_delta';
    IF v <> '0.3.0' THEN
        RAISE EXCEPTION 'ALTER EXTENSION UPDATE left extversion at %, expected 0.3.0', v;
    END IF;

    SELECT count(*) INTO n
    FROM pg_depend d
    JOIN pg_proc p ON p.oid = d.objid
    JOIN pg_extension e ON e.oid = d.refobjid
    WHERE d.deptype = 'e' AND e.extname = 'jsonb_delta';
    IF n <> 16 THEN
        RAISE EXCEPTION 'expected 16 functions after 0.3.0 upgrade, found %', n;
    END IF;
END $$;

-- Hop 3: 0.3.0 -> 0.3.1 (packaging-only release; no catalog change).
ALTER EXTENSION jsonb_delta UPDATE TO '0.3.1';

DO $$
DECLARE
    v text;
    n int;
BEGIN
    SELECT extversion INTO v FROM pg_extension WHERE extname = 'jsonb_delta';
    IF v <> '0.3.1' THEN
        RAISE EXCEPTION 'ALTER EXTENSION UPDATE left extversion at %, expected 0.3.1', v;
    END IF;

    SELECT count(*) INTO n
    FROM pg_depend d
    JOIN pg_proc p ON p.oid = d.objid
    JOIN pg_extension e ON e.oid = d.refobjid
    WHERE d.deptype = 'e' AND e.extname = 'jsonb_delta';
    IF n <> 16 THEN
        RAISE EXCEPTION 'expected 16 functions after 0.3.1 upgrade, found %', n;
    END IF;
END $$;

-- The upgraded functions must actually resolve and run on the 0.3.1 module.
SELECT jsonb_merge_shallow('{"a": 1}'::jsonb, '{"b": 2}'::jsonb) AS merge_works;
SELECT jsonb_deep_merge('{"a": {"b": 1}}'::jsonb, '{"a": {"c": 2}}'::jsonb) AS deep_merge_works;
SELECT jsonb_apply_changeset('{"a": 1}'::jsonb, '[{"op": "set", "path": "b", "value": 2}]'::jsonb) AS apply_changeset_works;
-- The one symbol pg_tviews depends on (jsonb_delta #12 / fraiseql/pg_tviews#50).
SELECT jsonb_smart_patch_scalar('{"a": 1}'::jsonb, '{"a": 2}'::jsonb) AS smart_patch_scalar_works;

DROP EXTENSION jsonb_delta;

\echo 'upgrade path 0.1.0 -> 0.2.0 -> 0.3.0 -> 0.3.1 OK'
