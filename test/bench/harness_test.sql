-- Self-test for the benchmark harness.
--
-- The harness is the instrument every published number will be measured with, so
-- it needs its own calibration before it is pointed at jsonb_delta. These tests
-- use synthetic arms with a *known* relative cost, so a wrong answer is provable
-- rather than a matter of opinion.
--
-- Run from the repository root:
--   psql -v ON_ERROR_STOP=1 -f test/bench/harness_test.sql
--
-- Exit status is the result: zero means the instrument is trustworthy.

\set ON_ERROR_STOP on
\set QUIET on

\i test/bench/harness.sql

\echo ''
\echo '=== Harness self-test ==='
\echo ''

-- ---------------------------------------------------------------------------
-- Control 1: a deliberately SLOWER "candidate" arm.
--
-- The failure mode this guards against is an instrument that reports a speedup
-- no matter what it is fed. The candidate arm here sleeps; it cannot be faster.
-- ---------------------------------------------------------------------------
SELECT bench.define(
    name        => 'control_slower',
    description => 'Candidate arm sleeps 5ms; must be reported as a regression',
    setup_sql   => 'SELECT 1',
    native_sql  => 'SELECT 1',
    delta_sql   => 'SELECT pg_sleep(0.005)',
    verify_sql  => $$SELECT 'constant'$$,
    n_trials    => 10,
    n_warmup    => 2
);

-- ---------------------------------------------------------------------------
-- Control 2: a deliberately FASTER candidate arm.
--
-- The mirror image: an instrument stuck reporting "no difference" would pass
-- control 1 and fail here.
-- ---------------------------------------------------------------------------
SELECT bench.define(
    name        => 'control_faster',
    description => 'Baseline arm sleeps 5ms; must be reported as a speedup',
    setup_sql   => 'SELECT 1',
    native_sql  => 'SELECT pg_sleep(0.005)',
    delta_sql   => 'SELECT 1',
    verify_sql  => $$SELECT 'constant'$$,
    n_trials    => 10,
    n_warmup    => 2
);

-- ---------------------------------------------------------------------------
-- Control 3: arms that disagree on output.
--
-- This is the one that matters most for #15. A "speedup" between two arms that
-- do not compute the same thing is not a speedup, and the harness must refuse
-- to report a ratio rather than publish a fast wrong answer.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS bench_selftest_target (id int primary key, data jsonb);
TRUNCATE bench_selftest_target;
INSERT INTO bench_selftest_target VALUES (1, '{"v": 0}');

SELECT bench.define(
    name        => 'control_mismatch',
    description => 'Arms write different values; harness must withhold a ratio',
    setup_sql   => $$UPDATE bench_selftest_target SET data = '{"v": 0}' WHERE id = 1$$,
    native_sql  => $$UPDATE bench_selftest_target SET data = '{"v": 1}' WHERE id = 1$$,
    delta_sql   => $$UPDATE bench_selftest_target SET data = '{"v": 2}' WHERE id = 1$$,
    verify_sql  => $$SELECT data::text FROM bench_selftest_target WHERE id = 1$$,
    n_trials    => 10,
    n_warmup    => 2
);

-- ---------------------------------------------------------------------------
-- Control 4: arms that agree on output via different routes.
--
-- Guards the opposite error: an over-strict comparison that flags every pair as
-- mismatched would make the harness useless, and would do so silently.
-- ---------------------------------------------------------------------------
SELECT bench.define(
    name        => 'control_agree',
    description => 'Arms reach the same state by different SQL; must compare equal',
    setup_sql   => $$UPDATE bench_selftest_target SET data = '{"v": 0}' WHERE id = 1$$,
    native_sql  => $$UPDATE bench_selftest_target SET data = jsonb_set(data, '{v}', '9') WHERE id = 1$$,
    delta_sql   => $$UPDATE bench_selftest_target SET data = '{"v": 9}'::jsonb WHERE id = 1$$,
    verify_sql  => $$SELECT data::text FROM bench_selftest_target WHERE id = 1$$,
    n_trials    => 10,
    n_warmup    => 2
);

-- ---------------------------------------------------------------------------
-- Controls 5 and 6: in-memory expression arms.
--
-- #15 distinguishes stored-table updates from in-memory expressions, so the
-- harness has to measure both. An in-memory arm mutates nothing, so there is no
-- database state for a shared verify_sql to read — the arm's own return value is
-- the output. Passing verify_sql => '' selects that mode, and it must police
-- correctness exactly as strictly as the stored-table mode does.
-- ---------------------------------------------------------------------------
SELECT bench.define(
    name        => 'control_inmem_agree',
    description => 'Equivalent in-memory expressions; must compare equal',
    native_sql  => $$SELECT jsonb_set('{"v": 0}'::jsonb, '{v}', '7')$$,
    delta_sql   => $$SELECT '{"v": 7}'::jsonb$$,
    verify_sql  => '',
    n_trials    => 10,
    n_warmup    => 2
);

SELECT bench.define(
    name        => 'control_inmem_mismatch',
    description => 'Differing in-memory expressions; harness must withhold a ratio',
    native_sql  => $$SELECT '{"v": 7}'::jsonb$$,
    delta_sql   => $$SELECT '{"v": 8}'::jsonb$$,
    verify_sql  => '',
    n_trials    => 10,
    n_warmup    => 2
);

\set QUIET off

-- ---------------------------------------------------------------------------
-- Assertions
-- ---------------------------------------------------------------------------
DO $selftest$
DECLARE
    r           bench.result;
    tolerance   CONSTANT numeric := 0.5;  -- generous: this is a sanity check, not a timing test
BEGIN
    -- Control 1: slower candidate must be reported as slower.
    r := bench.run('control_slower');
    IF NOT r.outputs_match THEN
        RAISE EXCEPTION 'control_slower: arms produce identical output but harness reported a mismatch';
    END IF;
    IF r.speedup IS NULL THEN
        RAISE EXCEPTION 'control_slower: harness withheld a ratio for matching arms';
    END IF;
    IF r.speedup >= 1.0 THEN
        RAISE EXCEPTION 'control_slower: sleeping arm reported as % x faster - harness cannot detect a regression', r.speedup;
    END IF;
    RAISE NOTICE 'control_slower      speedup=% (expected << 1)  OK', round(r.speedup, 3);

    -- Control 2: faster candidate must be reported as faster.
    r := bench.run('control_faster');
    IF r.speedup IS NULL OR r.speedup <= 1.0 THEN
        RAISE EXCEPTION 'control_faster: 5ms-vs-nothing reported as % - harness cannot detect a speedup', r.speedup;
    END IF;
    RAISE NOTICE 'control_faster      speedup=% (expected >> 1)  OK', round(r.speedup, 3);

    -- Control 3: mismatched arms must not yield a ratio.
    r := bench.run('control_mismatch');
    IF r.outputs_match THEN
        RAISE EXCEPTION 'control_mismatch: arms wrote different values but harness called them equal';
    END IF;
    IF r.speedup IS NOT NULL THEN
        RAISE EXCEPTION 'control_mismatch: harness reported speedup % for arms that disagree', r.speedup;
    END IF;
    RAISE NOTICE 'control_mismatch    speedup withheld, mismatch flagged  OK';

    -- Control 4: agreeing arms must compare equal.
    r := bench.run('control_agree');
    IF NOT r.outputs_match THEN
        RAISE EXCEPTION 'control_agree: arms reach identical state but harness flagged a mismatch (native=% delta=%)',
            r.native_digest, r.delta_digest;
    END IF;
    RAISE NOTICE 'control_agree       outputs compared equal  OK';

    -- Control 5: in-memory arms that agree.
    r := bench.run('control_inmem_agree');
    IF NOT r.outputs_match THEN
        RAISE EXCEPTION 'control_inmem_agree: equivalent expressions flagged as mismatched (native=% delta=%)',
            r.native_digest, r.delta_digest;
    END IF;
    IF r.native_digest IS NULL THEN
        RAISE EXCEPTION 'control_inmem_agree: harness captured no output for an in-memory arm';
    END IF;
    RAISE NOTICE 'control_inmem_agree in-memory arms captured and compared equal  OK';

    -- Control 6: in-memory arms that disagree.
    r := bench.run('control_inmem_mismatch');
    IF r.outputs_match THEN
        RAISE EXCEPTION 'control_inmem_mismatch: differing expressions called equal (both %)', r.native_digest;
    END IF;
    IF r.speedup IS NOT NULL THEN
        RAISE EXCEPTION 'control_inmem_mismatch: harness reported speedup % for arms that disagree', r.speedup;
    END IF;
    RAISE NOTICE 'control_inmem_mismatch  speedup withheld, mismatch flagged  OK';

    r := bench.run('control_agree');

    -- Statistical shape: the harness must report ordered order statistics over
    -- the requested number of trials, with warm-up excluded from them.
    IF r.n_trials <> 10 THEN
        RAISE EXCEPTION 'control_agree: reported n_trials=% but 10 were requested', r.n_trials;
    END IF;
    IF NOT (r.native_min_ms <= r.native_median_ms AND r.native_median_ms <= r.native_p95_ms) THEN
        RAISE EXCEPTION 'control_agree: native order statistics out of order (min=% median=% p95=%)',
            r.native_min_ms, r.native_median_ms, r.native_p95_ms;
    END IF;
    IF NOT (r.delta_min_ms <= r.delta_median_ms AND r.delta_median_ms <= r.delta_p95_ms) THEN
        RAISE EXCEPTION 'control_agree: delta order statistics out of order (min=% median=% p95=%)',
            r.delta_min_ms, r.delta_median_ms, r.delta_p95_ms;
    END IF;
    RAISE NOTICE 'order statistics    min <= median <= p95 over % trials  OK', r.n_trials;

    -- Trials must be isolated: a scenario whose arms mutate a table has to leave
    -- that table as it found it, or trial N is not measuring what trial 1 did.
    IF (SELECT data FROM bench_selftest_target WHERE id = 1) <> '{"v": 0}'::jsonb THEN
        RAISE EXCEPTION 'trial isolation: mutating scenario leaked state (target now %)',
            (SELECT data FROM bench_selftest_target WHERE id = 1);
    END IF;
    RAISE NOTICE 'trial isolation     mutations rolled back between trials  OK';
END
$selftest$;

DROP TABLE IF EXISTS bench_selftest_target;

\echo ''
\echo '✅ Harness self-test passed'
