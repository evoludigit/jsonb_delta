-- Upgrade-path test: install the extension at 0.1.0 and upgrade it to 0.2.0.
-- Requires sql/jsonb_delta--0.1.0.sql and sql/jsonb_delta--0.1.0--0.2.0.sql to
-- be present in the server's SHAREDIR/extension directory (see justfile
-- `test-upgrade` and the CI "Test extension upgrade path" step).
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
    FROM pg_extension e
    JOIN pg_depend d ON d.refobjid = e.oid AND d.classid = 'pg_proc'::regclass
    WHERE e.extname = 'jsonb_delta';
    IF n <> 16 THEN
        RAISE EXCEPTION 'expected 16 functions after upgrade, found %', n;
    END IF;
END $$;

-- The upgraded functions must actually resolve and run.
SELECT jsonb_merge_shallow('{"a": 1}'::jsonb, '{"b": 2}'::jsonb) AS merge_works;
SELECT jsonb_deep_merge('{"a": {"b": 1}}'::jsonb, '{"a": {"c": 2}}'::jsonb) AS deep_merge_works;
-- jsonb_apply_changeset is new in 0.2.0; the upgrade must ship it too.
SELECT jsonb_apply_changeset('{"a": 1}'::jsonb, '[{"op": "set", "path": "b", "value": 2}]'::jsonb) AS apply_changeset_works;

DROP EXTENSION jsonb_delta;

\echo 'upgrade path 0.1.0 -> 0.2.0 OK'
